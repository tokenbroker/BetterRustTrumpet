//! Default endpoints per role, active endpoints of either flow and the
//! endpoint form factor: what volume profiles (VolumeProfileService's
//! default-device slots, AudioEndpointHelper) and the tray icon read.
use super::{Backend, Engine, policy, take_string};
use crate::{AudioError, Endpoint, Role};
use windows::{
    Win32::{
        Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        Media::Audio::{
            DEVICE_STATE, DEVICE_STATE_ACTIVE, EDataFlow, ERole, IMMDevice,
            PKEY_AudioEndpoint_FormFactor, eCapture, eCommunications, eConsole, eMultimedia,
            eRender,
        },
        System::Com::{CLSCTX_ALL, CoCreateInstance, STGM_READ},
    },
    core::{HSTRING, PCWSTR},
};

fn flow(capture: bool) -> EDataFlow {
    if capture { eCapture } else { eRender }
}

/// AudioEndpointHelper's mapping: Playback/Recording switch the console and
/// multimedia roles, the communications slots only their own.
fn roles(role: Role) -> &'static [ERole] {
    match role {
        Role::Playback | Role::Recording => &[eMultimedia, eConsole],
        Role::PlaybackCommunications | Role::RecordingCommunications => &[eCommunications],
    }
}

/// The role read back for a slot (PrimaryRole in the original).
fn primary(role: Role) -> ERole {
    match role {
        Role::Playback | Role::Recording => eMultimedia,
        _ => eCommunications,
    }
}

impl Backend {
    fn endpoint_info(&self, device: &IMMDevice) -> Result<Endpoint, AudioError> {
        // SAFETY: live device on this thread; GetId's string is freed by
        // take_string and the property value is converted then dropped.
        unsafe {
            let id = take_string(device.GetId()?)?;
            let store = device.OpenPropertyStore(STGM_READ)?;
            let name = store
                .GetValue(&PKEY_Device_FriendlyName)
                .map(|v| v.to_string())
                .unwrap_or_default();
            Ok(Endpoint { id, name })
        }
    }

    fn active_endpoints(&self, capture: bool) -> Result<Vec<Endpoint>, AudioError> {
        // SAFETY: live enumerator on this thread; indices within GetCount.
        unsafe {
            let collection = self
                .enumerator
                .EnumAudioEndpoints(flow(capture), DEVICE_STATE_ACTIVE)?;
            let mut out = vec![];
            for i in 0..collection.GetCount()? {
                if let Ok(endpoint) = self.endpoint_info(&collection.Item(i)?) {
                    out.push(endpoint);
                }
            }
            Ok(out)
        }
    }

    fn default_endpoint(&self, role: Role) -> Option<Endpoint> {
        // SAFETY: live enumerator on this thread.
        let device = unsafe {
            self.enumerator
                .GetDefaultAudioEndpoint(flow(role.is_capture()), primary(role))
                .ok()?
        };
        self.endpoint_info(&device).ok()
    }

    fn is_active(&self, id: &str) -> bool {
        // SAFETY: live enumerator on this thread; the HSTRING outlives the call.
        unsafe {
            self.enumerator
                .GetDevice(&HSTRING::from(id))
                .and_then(|d| d.GetState())
                .is_ok_and(|state: DEVICE_STATE| state == DEVICE_STATE_ACTIVE)
        }
    }

    fn set_default_role(&self, id: &str, role: Role) -> Result<(), AudioError> {
        if !self.is_active(id) {
            return Err(AudioError::DeviceNotFound);
        }
        let id = HSTRING::from(id);
        // SAFETY: COM is initialized on this thread; the HSTRING outlives the
        // calls; the interface layout matches the shipped Windows component.
        unsafe {
            let config: policy::IPolicyConfig =
                CoCreateInstance(&super::CLSID_POLICY_CONFIG, None, CLSCTX_ALL)?;
            for &erole in roles(role) {
                policy::set_default_endpoint(&config, PCWSTR(id.as_ptr()), erole).ok()?;
            }
        }
        Ok(())
    }

    fn form_factor(&self, id: &str) -> Option<u32> {
        // SAFETY: live enumerator on this thread; the property value is
        // converted then dropped.
        unsafe {
            let device = self.enumerator.GetDevice(&HSTRING::from(id)).ok()?;
            let store = device.OpenPropertyStore(STGM_READ).ok()?;
            let value = store.GetValue(&PKEY_AudioEndpoint_FormFactor).ok()?;
            u32::try_from(&value).ok()
        }
    }
}

impl Engine {
    pub fn endpoints(&self, capture: bool) -> Result<Vec<Endpoint>, AudioError> {
        self.call(move |b| b.active_endpoints(capture))
    }
    pub fn default_endpoint(&self, role: Role) -> Result<Option<Endpoint>, AudioError> {
        self.call(move |b| Ok(b.default_endpoint(role)))
    }
    pub fn is_active(&self, id: &str) -> Result<bool, AudioError> {
        let id = id.to_owned();
        self.call(move |b| Ok(b.is_active(&id)))
    }
    pub fn set_default_role(&self, id: &str, role: Role) -> Result<(), AudioError> {
        let id = id.to_owned();
        self.call(move |b| b.set_default_role(&id, role))
    }
    pub fn form_factor(&self, id: &str) -> Result<Option<u32>, AudioError> {
        let id = id.to_owned();
        self.call(move |b| Ok(b.form_factor(&id)))
    }
}
