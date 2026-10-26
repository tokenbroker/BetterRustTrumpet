//! COM side of the engine. Everything here runs on the worker thread.
use crate::{AppSession, AudioError, Device, Snapshot};
use std::{sync::mpsc, thread};
use windows::{
    Win32::{
        Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        Foundation::CloseHandle,
        Media::Audio::{
            AudioSessionStateExpired, DEVICE_STATE_ACTIVE, Endpoints::IAudioEndpointVolume,
            Endpoints::IAudioMeterInformation, IAudioSessionControl2, IAudioSessionManager2,
            IMMDevice, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, eMultimedia,
            eRender,
        },
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
    core::{HSTRING, Interface, PWSTR},
};

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
                match Backend::new() {
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
        self.call(Backend::snapshot)
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
}

impl Backend {
    fn new() -> Result<Self, AudioError> {
        // SAFETY: COM is initialized on this thread (Apartment::enter).
        let enumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
        Ok(Self { enumerator })
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
        for session in Self::device_sessions(&device)? {
            if session.key == key {
                apply(&session.volume)?;
            }
        }
        Ok(())
    }
    fn device_sessions(device: &IMMDevice) -> Result<Vec<SessionInfo>, AudioError> {
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
                    (_, None, Some(path)) => file_stem(path),
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
    fn describe(&self, device: &IMMDevice, default_id: Option<&str>) -> Result<Device, AudioError> {
        // SAFETY: live interfaces on this thread; GetId's string is freed by
        // take_string, the property value is converted then dropped.
        let (id, name, endpoint, meter) = unsafe {
            let id = take_string(device.GetId()?)?;
            let store = device.OpenPropertyStore(STGM_READ)?;
            let name = store.GetValue(&PKEY_Device_FriendlyName)?.to_string();
            let endpoint: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
            let meter: IAudioMeterInformation = device.Activate(CLSCTX_ALL, None)?;
            (id, name, endpoint, meter)
        };
        let mut apps: Vec<AppSession> = vec![];
        for session in Self::device_sessions(device)? {
            // SAFETY: live session interfaces on this thread.
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
                }),
            }
        }
        // System sounds last, others by name, like the original's sort.
        apps.sort_by_key(|a| (a.is_system_sounds, a.display_name.to_lowercase()));
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
            volume,
            muted,
            peak,
            apps,
        })
    }
    fn snapshot(&self) -> Result<Snapshot, AudioError> {
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
        // SAFETY: live collection on this thread.
        let count = unsafe { collection.GetCount()? };
        for i in 0..count {
            // SAFETY: index is within GetCount.
            let device = unsafe { collection.Item(i)? };
            if let Ok(device) = self.describe(&device, default_id.as_deref()) {
                devices.push(device);
            }
        }
        devices.sort_by_key(|d| !d.is_default);
        Ok(Snapshot { devices })
    }
}
