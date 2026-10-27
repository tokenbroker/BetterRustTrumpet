//! Change notifications and per-app routing. COM callbacks arrive on
//! system threads; they only forward a signal and never touch engine state.
#![allow(non_snake_case)]
use std::{ffi::c_void, sync::Arc};
use windows::{
    Win32::{
        Foundation::PROPERTYKEY,
        Media::Audio::{
            AUDIO_VOLUME_NOTIFICATION_DATA, AudioSessionDisconnectReason, AudioSessionState,
            DEVICE_STATE, EDataFlow, ERole,
            Endpoints::{IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl},
            IAudioSessionControl, IAudioSessionEvents, IAudioSessionEvents_Impl,
            IAudioSessionNotification, IAudioSessionNotification_Impl, IMMNotificationClient,
            IMMNotificationClient_Impl,
        },
    },
    core::{BOOL, GUID, HRESULT, IUnknown, IUnknown_Vtbl, PCWSTR, Ref, implement, interface},
};

/// What changed; the app re-reads state rather than trusting payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// A device was added/removed/enabled/disabled or the default changed.
    Devices,
    /// An endpoint's volume or mute changed.
    Volume,
    /// An app session appeared, ended, or its volume or mute changed.
    Sessions,
}

pub type Listener = Arc<dyn Fn(Change) + Send + Sync>;

#[implement(IMMNotificationClient)]
pub struct DeviceWatcher {
    pub listener: Listener,
    /// Called on device changes so endpoint callbacks are re-registered.
    pub rewatch: Arc<dyn Fn() + Send + Sync>,
}

impl DeviceWatcher_Impl {
    fn devices(&self) -> windows::core::Result<()> {
        (self.listener)(Change::Devices);
        (self.rewatch)();
        Ok(())
    }
}

impl IMMNotificationClient_Impl for DeviceWatcher_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> windows::core::Result<()> {
        self.devices()
    }
    fn OnDeviceAdded(&self, _: &PCWSTR) -> windows::core::Result<()> {
        self.devices()
    }
    fn OnDeviceRemoved(&self, _: &PCWSTR) -> windows::core::Result<()> {
        self.devices()
    }
    fn OnDefaultDeviceChanged(
        &self,
        _: EDataFlow,
        _: ERole,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        (self.listener)(Change::Devices);
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}

#[implement(IAudioEndpointVolumeCallback)]
pub struct VolumeWatcher {
    pub listener: Listener,
}

impl IAudioEndpointVolumeCallback_Impl for VolumeWatcher_Impl {
    fn OnNotify(&self, _: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows::core::Result<()> {
        (self.listener)(Change::Volume);
        Ok(())
    }
}

/// New sessions on one device; their own events are registered by the
/// worker (`rewatch`), never from inside this callback.
#[implement(IAudioSessionNotification)]
pub struct SessionCreatedWatcher {
    pub listener: Listener,
    pub rewatch: Arc<dyn Fn() + Send + Sync>,
}

impl IAudioSessionNotification_Impl for SessionCreatedWatcher_Impl {
    fn OnSessionCreated(&self, _: Ref<IAudioSessionControl>) -> windows::core::Result<()> {
        (self.rewatch)();
        (self.listener)(Change::Sessions);
        Ok(())
    }
}

/// One session's volume, mute and lifetime changes.
#[implement(IAudioSessionEvents)]
pub struct SessionWatcher {
    pub listener: Listener,
}

impl IAudioSessionEvents_Impl for SessionWatcher_Impl {
    fn OnDisplayNameChanged(&self, _: &PCWSTR, _: *const GUID) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnIconPathChanged(&self, _: &PCWSTR, _: *const GUID) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnSimpleVolumeChanged(&self, _: f32, _: BOOL, _: *const GUID) -> windows::core::Result<()> {
        (self.listener)(Change::Sessions);
        Ok(())
    }
    fn OnChannelVolumeChanged(
        &self,
        _: u32,
        _: *const f32,
        _: u32,
        _: *const GUID,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnGroupingParamChanged(&self, _: *const GUID, _: *const GUID) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnStateChanged(&self, _: AudioSessionState) -> windows::core::Result<()> {
        (self.listener)(Change::Sessions);
        Ok(())
    }
    fn OnSessionDisconnected(&self, _: AudioSessionDisconnectReason) -> windows::core::Result<()> {
        (self.listener)(Change::Sessions);
        Ok(())
    }
}

/// Undocumented `Windows.Media.Internal.AudioPolicyConfig` factory used by
/// EarTrumpet for per-app default endpoints. The ABI is IInspectable; its
/// three slots are declared explicitly so the later methods keep their
/// offsets. Windows 10 21H2+ and earlier builds use different IIDs with the
/// same layout.
macro_rules! policy_factory {
    ($name:ident, $iid:literal) => {
        #[interface($iid)]
        pub unsafe trait $name: IUnknown {
            fn GetIids(&self) -> HRESULT;
            fn GetRuntimeClassName(&self) -> HRESULT;
            fn GetTrustLevel(&self) -> HRESULT;
            fn AddCtxVolumeChange(&self) -> HRESULT;
            fn RemoveCtxVolumeChanged(&self) -> HRESULT;
            fn AddRingerVibrateStateChanged(&self) -> HRESULT;
            fn RemoveRingerVibrateStateChange(&self) -> HRESULT;
            fn SetVolumeGroupGainForId(&self) -> HRESULT;
            fn GetVolumeGroupGainForId(&self) -> HRESULT;
            fn GetActiveVolumeGroupForEndpointId(&self) -> HRESULT;
            fn GetVolumeGroupsForEndpoint(&self) -> HRESULT;
            fn GetCurrentVolumeContext(&self) -> HRESULT;
            fn SetVolumeGroupMuteForId(&self) -> HRESULT;
            fn GetVolumeGroupMuteForId(&self) -> HRESULT;
            fn SetRingerVibrateState(&self) -> HRESULT;
            fn GetRingerVibrateState(&self) -> HRESULT;
            fn SetPreferredChatApplication(&self) -> HRESULT;
            fn ResetPreferredChatApplication(&self) -> HRESULT;
            fn GetPreferredChatApplication(&self) -> HRESULT;
            fn GetCurrentChatApplications(&self) -> HRESULT;
            fn AddChatContextChanged(&self) -> HRESULT;
            fn RemoveChatContextChanged(&self) -> HRESULT;
            fn SetPersistedDefaultAudioEndpoint(
                &self,
                process_id: u32,
                flow: EDataFlow,
                role: ERole,
                device_id: *mut c_void,
            ) -> HRESULT;
            fn GetPersistedDefaultAudioEndpoint(
                &self,
                process_id: u32,
                flow: EDataFlow,
                role: ERole,
                device_id: *mut *mut c_void,
            ) -> HRESULT;
            fn ClearAllPersistedApplicationDefaultEndpoints(&self) -> HRESULT;
        }
    };
}
policy_factory!(IPolicyFactory21H2, "ab3d4648-e242-459f-b02f-541c70306324");
policy_factory!(
    IPolicyFactoryDownlevel,
    "2a59116d-6c4f-45e0-a74f-707e3fef9258"
);

pub enum PolicyFactory {
    Modern(IPolicyFactory21H2),
    Downlevel(IPolicyFactoryDownlevel),
}

impl PolicyFactory {
    /// # Safety
    /// COM/WinRT must be initialized on the calling thread.
    pub unsafe fn create() -> windows::core::Result<Self> {
        use windows::Win32::System::WinRT::RoGetActivationFactory;
        use windows::core::{HSTRING, Interface};
        let class = HSTRING::from("Windows.Media.Internal.AudioPolicyConfig");
        // SAFETY: forwarded caller contract; the HSTRING outlives the call.
        let factory: IUnknown = unsafe { RoGetActivationFactory(&class)? };
        match factory.cast::<IPolicyFactory21H2>() {
            Ok(modern) => Ok(Self::Modern(modern)),
            Err(_) => Ok(Self::Downlevel(factory.cast()?)),
        }
    }
    /// # Safety
    /// `device_id` is a valid HSTRING handle or null (null clears).
    pub unsafe fn set(
        &self,
        pid: u32,
        flow: EDataFlow,
        role: ERole,
        device_id: *mut c_void,
    ) -> HRESULT {
        // SAFETY: forwarded caller contract; the factory is a live interface.
        unsafe {
            match self {
                Self::Modern(f) => f.SetPersistedDefaultAudioEndpoint(pid, flow, role, device_id),
                Self::Downlevel(f) => {
                    f.SetPersistedDefaultAudioEndpoint(pid, flow, role, device_id)
                }
            }
        }
    }
    /// # Safety
    /// `out` must be writable; on success it receives an owned HSTRING handle.
    pub unsafe fn get(
        &self,
        pid: u32,
        flow: EDataFlow,
        role: ERole,
        out: *mut *mut c_void,
    ) -> HRESULT {
        // SAFETY: forwarded caller contract; the factory is a live interface.
        unsafe {
            match self {
                Self::Modern(f) => f.GetPersistedDefaultAudioEndpoint(pid, flow, role, out),
                Self::Downlevel(f) => f.GetPersistedDefaultAudioEndpoint(pid, flow, role, out),
            }
        }
    }
}

const MMDEVAPI_TOKEN: &str = r"\\?\SWD#MMDEVAPI#";
const RENDER_INTERFACE: &str = "#{e6327cad-dcec-4949-ae8a-991e976a79d2}";

/// Endpoint ID → the device-interface path the policy factory expects.
pub fn pack_device_id(id: &str) -> String {
    format!("{MMDEVAPI_TOKEN}{id}{RENDER_INTERFACE}")
}
pub fn unpack_device_id(id: &str) -> String {
    let id = id.strip_prefix(MMDEVAPI_TOKEN).unwrap_or(id);
    id.strip_suffix(RENDER_INTERFACE).unwrap_or(id).to_owned()
}

#[allow(dead_code)]
fn _assert_types(_: IMMNotificationClient, _: IAudioEndpointVolumeCallback) {}
