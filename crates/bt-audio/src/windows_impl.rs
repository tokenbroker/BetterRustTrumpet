//! COM side of the engine. Everything here runs on the worker thread.
use crate::{
    AppSession, AudioError, Device, Peaks, Snapshot,
    notify::{self, DeviceWatcher, Listener, PolicyFactory, VolumeWatcher},
};
use std::{cell::RefCell, collections::HashMap, ffi::c_void, sync::Arc, sync::mpsc, thread};
use windows::{
    Win32::{
        Devices::FunctionDiscovery::{PKEY_Device_DeviceDesc, PKEY_Device_FriendlyName},
        Foundation::{CloseHandle, PROPERTYKEY},
        Media::Audio::{
            AudioSessionStateExpired, DEVICE_STATE_ACTIVE, Endpoints::IAudioEndpointVolume,
            Endpoints::IAudioEndpointVolumeCallback, Endpoints::IAudioMeterInformation,
            IAudioSessionControl2, IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator,
            IMMNotificationClient, ISimpleAudioVolume, MMDeviceEnumerator, eConsole, eMultimedia,
            eRender,
        },
        Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW},
        System::{
            Com::{
                CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
                CoUninitialize, STGM_READ,
            },
            Threading::{
                OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                QueryFullProcessImageNameW,
            },
        },
    },
    core::{GUID, HSTRING, Interface, PCWSTR, PWSTR},
};

/// DEVPKEY_Device_EnumeratorName ("USB", "HDAUDIO"), as a property-store key.
const DEVICE_ENUMERATOR_NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
    pid: 24,
};

#[allow(non_snake_case)]
mod policy {
    use windows::{
        Win32::Media::Audio::ERole,
        core::{HRESULT, IUnknown, IUnknown_Vtbl, PCWSTR, interface},
    };
    /// The undocumented policy-config interface EarTrumpet uses to change the
    /// default endpoint (Windows 7+). Only `SetDefaultEndpoint` is called; the
    /// preceding slots exist to keep the vtable layout.
    #[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    pub unsafe trait IPolicyConfig: IUnknown {
        fn GetMixFormat(&self) -> HRESULT;
        fn GetDeviceFormat(&self) -> HRESULT;
        fn ResetDeviceFormat(&self) -> HRESULT;
        fn SetDeviceFormat(&self) -> HRESULT;
        fn GetProcessingPeriod(&self) -> HRESULT;
        fn SetProcessingPeriod(&self) -> HRESULT;
        fn GetShareMode(&self) -> HRESULT;
        fn SetShareMode(&self) -> HRESULT;
        fn GetPropertyValue(&self) -> HRESULT;
        fn SetPropertyValue(&self) -> HRESULT;
        fn SetDefaultEndpoint(&self, device_id: PCWSTR, role: ERole) -> HRESULT;
        fn SetEndpointVisibility(&self) -> HRESULT;
    }

    /// # Safety
    /// COM must be initialized on this thread and `id` must point at a
    /// NUL-terminated endpoint ID that outlives the call.
    pub unsafe fn set_default_endpoint(policy: &IPolicyConfig, id: PCWSTR, role: ERole) -> HRESULT {
        // SAFETY: forwarded from the caller's contract above.
        unsafe { policy.SetDefaultEndpoint(id, role) }
    }
}
use policy::IPolicyConfig;
const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

impl From<windows::core::Error> for AudioError {
    fn from(error: windows::core::Error) -> Self {
        AudioError::Com(format!("{} ({:?})", error.message(), error.code()))
    }
}

type Job = Box<dyn FnOnce(&Backend) + Send>;

#[derive(Clone)]
pub struct Engine {
    jobs: mpsc::Sender<Job>,
}

impl Engine {
    pub fn start() -> Result<Self, AudioError> {
        let (jobs, inbox) = mpsc::channel::<Job>();
        let jobs_for_backend = jobs.clone();
        let (ready, started) = mpsc::channel();
        thread::Builder::new()
            .name("bt-audio-com".into())
            .spawn(move || {
                let apartment = match Apartment::enter() {
                    Ok(apartment) => apartment,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                match Backend::new(jobs_for_backend) {
                    Ok(backend) => {
                        let _ = ready.send(Ok(()));
                        for job in inbox {
                            job(&backend);
                        }
                    }
                    Err(error) => {
                        let _ = ready.send(Err(error));
                    }
                }
                drop(apartment);
            })
            .map_err(|e| AudioError::Com(e.to_string()))?;
        started.recv().map_err(|_| AudioError::Stopped)??;
        Ok(Self { jobs })
    }
    fn call<T: Send + 'static>(
        &self,
        work: impl FnOnce(&Backend) -> Result<T, AudioError> + Send + 'static,
    ) -> Result<T, AudioError> {
        let (reply, answer) = mpsc::channel();
        self.jobs
            .send(Box::new(move |backend| {
                let _ = reply.send(work(backend));
            }))
            .map_err(|_| AudioError::Stopped)?;
        answer.recv().map_err(|_| AudioError::Stopped)?
    }
    pub fn snapshot(&self) -> Result<Snapshot, AudioError> {
        self.call(|b| b.snapshot(true))
    }
    pub fn devices(&self) -> Result<Snapshot, AudioError> {
        self.call(|b| b.snapshot(false))
    }
    pub fn peaks(&self) -> Result<Vec<Peaks>, AudioError> {
        self.call(|b| Ok(b.peaks()))
    }
    pub fn set_default_device(&self, id: &str) -> Result<(), AudioError> {
        let id = id.to_owned();
        self.call(move |b| b.set_default(&id))
    }
    pub fn watch(&self, listener: Listener) -> Result<(), AudioError> {
        self.call(move |b| b.watch(listener))
    }
    pub fn set_app_device(&self, key: &str, device_id: Option<&str>) -> Result<(), AudioError> {
        let (key, device_id) = (key.to_owned(), device_id.map(str::to_owned));
        self.call(move |b| b.set_app_device(&key, device_id.as_deref()))
    }
    pub fn set_device_volume(&self, id: &str, volume: f32) -> Result<(), AudioError> {
        let id = id.to_owned();
        self.call(move |b| {
            b.endpoint(&id, |e| unsafe {
                // SAFETY: `e` is a live endpoint interface on this COM thread; a
                // null event-context GUID is documented as allowed.
                e.SetMasterVolumeLevelScalar(volume, std::ptr::null())
            })
        })
    }
    pub fn set_device_mute(&self, id: &str, muted: bool) -> Result<(), AudioError> {
        let id = id.to_owned();
        self.call(move |b| {
            b.endpoint(&id, |e| unsafe {
                // SAFETY: as above; the context GUID may be null.
                e.SetMute(muted, std::ptr::null())
            })
        })
    }
    pub fn set_app_volume(&self, id: &str, key: &str, volume: f32) -> Result<(), AudioError> {
        let (id, key) = (id.to_owned(), key.to_owned());
        self.call(move |b| {
            b.sessions(&id, &key, |s| unsafe {
                // SAFETY: `s` is a live session volume interface on this thread;
                // the context GUID may be null.
                s.SetMasterVolume(volume, std::ptr::null())
            })
        })
    }
    pub fn set_app_mute(&self, id: &str, key: &str, muted: bool) -> Result<(), AudioError> {
        let (id, key) = (id.to_owned(), key.to_owned());
        self.call(move |b| {
            b.sessions(&id, &key, |s| unsafe {
                // SAFETY: as above.
                s.SetMute(muted, std::ptr::null())
            })
        })
    }
}

/// Multithreaded COM apartment for the worker thread's lifetime.
struct Apartment;
impl Apartment {
    fn enter() -> Result<Self, AudioError> {
        // SAFETY: called once at the start of the dedicated worker thread,
        // balanced by CoUninitialize in Drop on the same thread.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: matches the successful CoInitializeEx on this thread; all
        // COM objects (the Backend) were dropped before the apartment.
        unsafe { CoUninitialize() };
    }
}

/// Takes ownership of a CoTaskMem-allocated wide string.
fn take_string(value: PWSTR) -> Result<String, AudioError> {
    // SAFETY: `value` was returned by a COM getter that allocates with
    // CoTaskMemAlloc and transfers ownership; it is read once, then freed.
    unsafe {
        let text = value
            .to_string()
            .map_err(|e| AudioError::Com(e.to_string()));
        CoTaskMemFree(Some(value.0 as *const _));
        text
    }
}

fn process_image(pid: u32) -> Option<String> {
    // SAFETY: OpenProcess with limited query rights has no preconditions; the
    // handle is closed below. The buffer outlives the call and `len` holds its
    // capacity in UTF-16 units as the API requires.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(handle);
        result.ok()?;
        Some(String::from_utf16_lossy(&buffer[..len as usize]))
    }
}

/// The executable's FileDescription (what Task Manager shows), if any.
fn file_description(path: &str) -> Option<String> {
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let path = PCWSTR(wide.as_ptr());
    // SAFETY: `path` is NUL-terminated and outlives every call; `data` is
    // sized by GetFileVersionInfoSizeW; VerQueryValueW returns pointers into
    // `data`, which stays alive while they are read, and lengths in units.
    unsafe {
        let size = GetFileVersionInfoSizeW(path, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(path, None, size, data.as_mut_ptr().cast()).ok()?;
        let query = |sub: &str| -> Option<(*const c_void, u32)> {
            let sub: Vec<u16> = sub.encode_utf16().chain(Some(0)).collect();
            let mut pointer = std::ptr::null_mut();
            let mut length = 0u32;
            VerQueryValueW(
                data.as_ptr().cast(),
                PCWSTR(sub.as_ptr()),
                &mut pointer,
                &mut length,
            )
            .as_bool()
            .then_some((pointer as *const c_void, length))
        };
        let mut languages = vec![];
        if let Some((pointer, bytes)) = query("\\VarFileInfo\\Translation") {
            let pairs = std::slice::from_raw_parts(pointer as *const u16, bytes as usize / 2);
            languages.extend(pairs.chunks_exact(2).map(|p| (p[0], p[1])));
        }
        languages.extend([(0x0409, 0x04b0), (0x0409, 0x04e4), (0x0000, 0x04b0)]);
        for (language, codepage) in languages {
            let sub = format!("\\StringFileInfo\\{language:04x}{codepage:04x}\\FileDescription");
            if let Some((pointer, units)) = query(&sub) {
                let text = std::slice::from_raw_parts(pointer as *const u16, units as usize);
                let text = String::from_utf16_lossy(text);
                let text = text.trim_end_matches('\0').trim();
                if !text.is_empty() {
                    return Some(text.to_owned());
                }
            }
        }
        None
    }
}

fn file_stem(path: &str) -> String {
    let name = path.rsplit(['\\', '/']).next().unwrap_or(path);
    name.strip_suffix(".exe")
        .or_else(|| name.strip_suffix(".EXE"))
        .unwrap_or(name)
        .to_owned()
}

struct SessionInfo {
    key: String,
    display_name: String,
    executable: Option<String>,
    pid: u32,
    system: bool,
    volume: ISimpleAudioVolume,
    meter: IAudioMeterInformation,
}

struct Backend {
    enumerator: IMMDeviceEnumerator,
    jobs: mpsc::Sender<Job>,
    listener: RefCell<Option<Listener>>,
    device_client: RefCell<Option<IMMNotificationClient>>,
    endpoint_watchers: RefCell<Vec<(IAudioEndpointVolume, IAudioEndpointVolumeCallback)>>,
    /// Lazily created; `Some(None)` once creation has failed.
    policy: RefCell<Option<Option<PolicyFactory>>>,
    /// FileDescription per executable path; version resources rarely change.
    descriptions: RefCell<HashMap<String, Option<String>>>,
    /// Meters found by the last full snapshot, read cheaply by `peaks`.
    meters: RefCell<Vec<MeterSource>>,
}

/// A device's or app session's meter, as found by the last full snapshot.
struct MeterSource {
    device_id: String,
    /// `None` for the device itself.
    app_key: Option<String>,
    meter: IAudioMeterInformation,
}

impl Backend {
    fn new(jobs: mpsc::Sender<Job>) -> Result<Self, AudioError> {
        // SAFETY: COM is initialized on this thread (Apartment::enter).
        let enumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
        Ok(Self {
            enumerator,
            jobs,
            listener: RefCell::new(None),
            device_client: RefCell::new(None),
            endpoint_watchers: RefCell::new(vec![]),
            policy: RefCell::new(None),
            descriptions: RefCell::new(HashMap::new()),
            meters: RefCell::new(vec![]),
        })
    }
    fn watch(&self, listener: Listener) -> Result<(), AudioError> {
        let jobs = self.jobs.clone();
        let rewatch = Arc::new(move || {
            let _ = jobs.send(Box::new(|b: &Backend| {
                let _ = b.watch_endpoints();
            }));
        });
        let client: IMMNotificationClient = DeviceWatcher {
            listener: listener.clone(),
            rewatch,
        }
        .into();
        // SAFETY: live enumerator on this thread; the client stays alive in
        // `device_client` until it is unregistered in Drop.
        unsafe {
            self.enumerator
                .RegisterEndpointNotificationCallback(&client)?
        };
        *self.device_client.borrow_mut() = Some(client);
        *self.listener.borrow_mut() = Some(listener);
        self.watch_endpoints()
    }
    /// (Re)registers volume callbacks for every active output.
    fn watch_endpoints(&self) -> Result<(), AudioError> {
        let Some(listener) = self.listener.borrow().clone() else {
            return Ok(());
        };
        for (endpoint, callback) in self.endpoint_watchers.borrow_mut().drain(..) {
            // SAFETY: unregistering a callback registered on this endpoint.
            let _ = unsafe { endpoint.UnregisterControlChangeNotify(&callback) };
        }
        // SAFETY: live enumerator/collection on this thread.
        let collection = unsafe {
            self.enumerator
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?
        };
        let count = unsafe { collection.GetCount()? };
        for i in 0..count {
            // SAFETY: index within GetCount; documented interface activation;
            // the callback is kept alive in `endpoint_watchers`.
            unsafe {
                let device = collection.Item(i)?;
                let endpoint: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
                let callback: IAudioEndpointVolumeCallback = VolumeWatcher {
                    listener: listener.clone(),
                }
                .into();
                if endpoint.RegisterControlChangeNotify(&callback).is_ok() {
                    self.endpoint_watchers
                        .borrow_mut()
                        .push((endpoint, callback));
                }
            }
        }
        Ok(())
    }
    fn with_policy<T>(&self, work: impl FnOnce(&PolicyFactory) -> T) -> Option<T> {
        let mut policy = self.policy.borrow_mut();
        // SAFETY: COM is initialized on this worker thread.
        let factory = policy.get_or_insert_with(|| unsafe { PolicyFactory::create().ok() });
        factory.as_ref().map(work)
    }
    /// The endpoint an app's process is routed to, if any.
    fn routed_device(&self, pid: u32) -> Option<String> {
        self.with_policy(|policy| {
            let mut raw = std::ptr::null_mut();
            // SAFETY: `raw` is writable; on success it holds an owned HSTRING
            // handle, which is taken over (and freed) by the transmute.
            unsafe {
                if policy.get(pid, eRender, eMultimedia, &mut raw).is_err() || raw.is_null() {
                    return None;
                }
                let id: HSTRING = std::mem::transmute(raw);
                Some(notify::unpack_device_id(&id.to_string()))
            }
        })
        .flatten()
        .filter(|id| !id.is_empty())
    }
    /// Routes every process of `key` (on any output) to `device`, or back to
    /// the default when `None`; console and multimedia roles, as the original.
    fn set_app_device(&self, key: &str, device: Option<&str>) -> Result<(), AudioError> {
        let mut pids = vec![];
        // SAFETY: live enumerator/collection on this thread.
        let collection = unsafe {
            self.enumerator
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?
        };
        for i in 0..unsafe { collection.GetCount()? } {
            // SAFETY: index within GetCount.
            let endpoint = unsafe { collection.Item(i)? };
            for session in self.device_sessions(&endpoint)? {
                if session.key == key && session.pid != 0 && !pids.contains(&session.pid) {
                    pids.push(session.pid);
                }
            }
        }
        let packed = device.map(|id| HSTRING::from(notify::pack_device_id(id)));
        let result = self.with_policy(|policy| {
            for &pid in &pids {
                for role in [eMultimedia, eConsole] {
                    // SAFETY: the handle is borrowed from `packed`, which
                    // outlives the call (null clears the routing).
                    let handle: *mut c_void = match &packed {
                        Some(h) => unsafe { std::mem::transmute_copy(h) },
                        None => std::ptr::null_mut(),
                    };
                    unsafe { policy.set(pid, eRender, role, handle) }.ok()?;
                }
            }
            Ok::<(), windows::core::Error>(())
        });
        match result {
            Some(r) => Ok(r?),
            None => Err(AudioError::Com("per-app routing is unavailable".into())),
        }
    }
    fn describe_executable(&self, path: &str) -> Option<String> {
        self.descriptions
            .borrow_mut()
            .entry(path.to_owned())
            .or_insert_with(|| file_description(path))
            .clone()
    }
    /// Console and multimedia roles, as the original's CLI and profiles do.
    fn set_default(&self, id: &str) -> Result<(), AudioError> {
        self.device(id)?;
        let id = HSTRING::from(id);
        // SAFETY: COM is initialized on this thread; the HSTRING outlives the
        // calls; the interface layout matches the shipped Windows component.
        unsafe {
            let policy: IPolicyConfig = CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL)?;
            for role in [eConsole, eMultimedia] {
                policy::set_default_endpoint(&policy, PCWSTR(id.as_ptr()), role).ok()?;
            }
        }
        Ok(())
    }
    fn device(&self, id: &str) -> Result<IMMDevice, AudioError> {
        // SAFETY: the enumerator is a live interface on this thread and the
        // HSTRING outlives the call.
        unsafe { self.enumerator.GetDevice(&HSTRING::from(id)) }
            .map_err(|_| AudioError::DeviceNotFound)
    }
    fn endpoint(
        &self,
        id: &str,
        apply: impl FnOnce(&IAudioEndpointVolume) -> windows::core::Result<()>,
    ) -> Result<(), AudioError> {
        let device = self.device(id)?;
        // SAFETY: activating a documented interface on a live device.
        let endpoint: IAudioEndpointVolume = unsafe { device.Activate(CLSCTX_ALL, None)? };
        Ok(apply(&endpoint)?)
    }
    fn sessions(
        &self,
        id: &str,
        key: &str,
        apply: impl Fn(&ISimpleAudioVolume) -> windows::core::Result<()>,
    ) -> Result<(), AudioError> {
        let device = self.device(id)?;
        for session in self.device_sessions(&device)? {
            if session.key == key {
                apply(&session.volume)?;
            }
        }
        Ok(())
    }
    fn device_sessions(&self, device: &IMMDevice) -> Result<Vec<SessionInfo>, AudioError> {
        let mut out = vec![];
        // SAFETY: all calls are on live interfaces owned by this thread;
        // returned strings are freed by take_string.
        unsafe {
            let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
            let sessions = manager.GetSessionEnumerator()?;
            for i in 0..sessions.GetCount()? {
                let control = sessions.GetSession(i)?;
                if control.GetState()? == AudioSessionStateExpired {
                    continue;
                }
                let control2: IAudioSessionControl2 = control.cast()?;
                let pid = control2.GetProcessId().unwrap_or(0);
                let system = control2.IsSystemSoundsSession().0 == 0;
                let executable = if system { None } else { process_image(pid) };
                let key = if system {
                    "system".to_owned()
                } else {
                    executable
                        .as_deref()
                        .map(str::to_lowercase)
                        .unwrap_or_else(|| format!("pid:{pid}"))
                };
                let reported = control
                    .GetDisplayName()
                    .ok()
                    .and_then(|name| take_string(name).ok())
                    .filter(|name| !name.is_empty() && !name.starts_with('@'));
                let display_name = match (system, reported, &executable) {
                    (true, _, _) => "System sounds".to_owned(),
                    (_, Some(name), _) => name,
                    (_, None, Some(path)) => self
                        .describe_executable(path)
                        .unwrap_or_else(|| file_stem(path)),
                    (_, None, None) => format!("Process {pid}"),
                };
                out.push(SessionInfo {
                    key,
                    display_name,
                    executable,
                    pid,
                    system,
                    volume: control.cast()?,
                    meter: control.cast()?,
                });
            }
        }
        Ok(out)
    }
    fn describe(
        &self,
        device: &IMMDevice,
        default_id: Option<&str>,
        with_apps: bool,
        meters: &mut Vec<MeterSource>,
    ) -> Result<Device, AudioError> {
        // SAFETY: live interfaces on this thread; GetId's string is freed by
        // take_string, the property value is converted then dropped.
        let (id, name, description, enumerator, endpoint, meter) = unsafe {
            let id = take_string(device.GetId()?)?;
            let store = device.OpenPropertyStore(STGM_READ)?;
            let name = store.GetValue(&PKEY_Device_FriendlyName)?.to_string();
            let text = |key: &PROPERTYKEY| {
                store
                    .GetValue(key)
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            };
            let description = text(&PKEY_Device_DeviceDesc);
            let enumerator = text(&DEVICE_ENUMERATOR_NAME);
            let endpoint: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
            let meter: IAudioMeterInformation = device.Activate(CLSCTX_ALL, None)?;
            (id, name, description, enumerator, endpoint, meter)
        };
        let mut apps: Vec<AppSession> = vec![];
        let sessions = if with_apps {
            self.device_sessions(device)?
        } else {
            vec![]
        };
        for session in sessions {
            // SAFETY: live session interfaces on this thread.
            meters.push(MeterSource {
                device_id: id.clone(),
                app_key: Some(session.key.clone()),
                meter: session.meter.clone(),
            });
            let (volume, muted, peak) = unsafe {
                (
                    session.volume.GetMasterVolume().unwrap_or(0.0),
                    session
                        .volume
                        .GetMute()
                        .map(|m| m.as_bool())
                        .unwrap_or(false),
                    session.meter.GetPeakValue().unwrap_or(0.0),
                )
            };
            let routed = if session.system {
                None
            } else {
                self.routed_device(session.pid)
            };
            match apps.iter_mut().find(|a| a.key == session.key) {
                Some(app) => {
                    app.process_ids.push(session.pid);
                    app.peak = app.peak.max(peak);
                }
                None => apps.push(AppSession {
                    key: session.key,
                    display_name: session.display_name,
                    executable: session.executable,
                    process_ids: vec![session.pid],
                    is_system_sounds: session.system,
                    volume,
                    muted,
                    peak,
                    routed_device_id: routed,
                }),
            }
        }
        // System sounds last, others by name, like the original's sort.
        apps.sort_by_key(|a| (a.is_system_sounds, a.display_name.to_lowercase()));
        meters.push(MeterSource {
            device_id: id.clone(),
            app_key: None,
            meter: meter.clone(),
        });
        // SAFETY: live endpoint interfaces on this thread.
        let (volume, muted, peak) = unsafe {
            (
                endpoint.GetMasterVolumeLevelScalar().unwrap_or(0.0),
                endpoint.GetMute().map(|m| m.as_bool()).unwrap_or(false),
                meter.GetPeakValue().unwrap_or(0.0),
            )
        };
        Ok(Device {
            is_default: default_id == Some(id.as_str()),
            id,
            name,
            description,
            enumerator,
            volume,
            muted,
            peak,
            apps,
        })
    }
    fn snapshot(&self, with_apps: bool) -> Result<Snapshot, AudioError> {
        // SAFETY: live enumerator on this thread; strings freed by take_string.
        let (default_id, collection) = unsafe {
            let default_id = self
                .enumerator
                .GetDefaultAudioEndpoint(eRender, eMultimedia)
                .ok()
                .and_then(|d| d.GetId().ok())
                .and_then(|id| take_string(id).ok());
            (
                default_id,
                self.enumerator
                    .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?,
            )
        };
        let mut devices = vec![];
        let mut meters = vec![];
        // SAFETY: live collection on this thread.
        let count = unsafe { collection.GetCount()? };
        for i in 0..count {
            // SAFETY: index is within GetCount.
            let device = unsafe { collection.Item(i)? };
            if let Ok(device) =
                self.describe(&device, default_id.as_deref(), with_apps, &mut meters)
            {
                devices.push(device);
            }
        }
        if with_apps {
            *self.meters.borrow_mut() = meters;
        }
        devices.sort_by_key(|d| !d.is_default);
        Ok(Snapshot { devices })
    }
}

impl Backend {
    /// Left/right peaks of every meter the last full snapshot found; an app
    /// with several sessions reports the loudest per channel. Mono meters
    /// report the same value twice.
    fn peaks(&self) -> Vec<Peaks> {
        let mut out: Vec<Peaks> = vec![];
        for source in self.meters.borrow().iter() {
            // SAFETY: a live meter interface on this thread; the buffer holds
            // as many floats as the channel count passed with it.
            let channels = unsafe {
                let count = source.meter.GetMeteringChannelCount().unwrap_or(0);
                let mut values = vec![0f32; count.max(1) as usize];
                if count == 0 || source.meter.GetChannelsPeakValues(&mut values).is_err() {
                    let peak = source.meter.GetPeakValue().unwrap_or(0.0);
                    [peak, peak]
                } else {
                    [values[0], *values.get(1).unwrap_or(&values[0])]
                }
            };
            match out
                .iter_mut()
                .find(|p| p.device_id == source.device_id && p.app_key == source.app_key)
            {
                Some(p) => {
                    p.channels[0] = p.channels[0].max(channels[0]);
                    p.channels[1] = p.channels[1].max(channels[1]);
                }
                None => out.push(Peaks {
                    device_id: source.device_id.clone(),
                    app_key: source.app_key.clone(),
                    channels,
                }),
            }
        }
        out
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        // SAFETY: unregistering callbacks registered on this thread, before
        // the apartment is torn down.
        unsafe {
            if let Some(client) = self.device_client.borrow_mut().take() {
                let _ = self
                    .enumerator
                    .UnregisterEndpointNotificationCallback(&client);
            }
            for (endpoint, callback) in self.endpoint_watchers.borrow_mut().drain(..) {
                let _ = endpoint.UnregisterControlChangeNotify(&callback);
            }
        }
    }
}
