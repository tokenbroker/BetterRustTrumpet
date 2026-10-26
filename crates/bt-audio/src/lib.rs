//! Windows Core Audio for the BetterTrumpet port (preview scope): active
//! render endpoints, per-app session groups, volume, mute and peak meters.
//!
//! All COM objects live on one dedicated multithreaded-apartment worker
//! thread; callers talk to it through [`AudioEngine`], so no COM pointer ever
//! crosses a thread boundary. Default-device switching, routing and change
//! notifications are later milestone-2 work (PARITY A01–A05).
use serde::Serialize;
use thiserror::Error;

#[cfg(windows)]
mod windows_impl;

#[derive(Clone, Debug, Error)]
pub enum AudioError {
    #[error("Core Audio call failed: {0}")]
    Com(String),
    #[error("audio device not found")]
    DeviceNotFound,
    #[error("audio engine stopped")]
    Stopped,
    #[error("Core Audio is only available on Windows")]
    Unsupported,
}

/// One application on a device: every session with the same key (process
/// image path, or the system-sounds session) is adjusted together.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSession {
    pub key: String,
    pub display_name: String,
    pub executable: Option<String>,
    pub process_ids: Vec<u32>,
    pub is_system_sounds: bool,
    pub volume: f32,
    pub muted: bool,
    pub peak: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub volume: f32,
    pub muted: bool,
    pub peak: f32,
    pub apps: Vec<AppSession>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub devices: Vec<Device>,
}

/// Handle to the Core Audio worker thread. Cheap to clone and `Send + Sync`.
#[derive(Clone)]
pub struct AudioEngine {
    #[cfg(windows)]
    inner: windows_impl::Engine,
}

impl AudioEngine {
    pub fn start() -> Result<Self, AudioError> {
        #[cfg(windows)]
        {
            Ok(Self {
                inner: windows_impl::Engine::start()?,
            })
        }
        #[cfg(not(windows))]
        {
            Err(AudioError::Unsupported)
        }
    }
    /// Active render endpoints, default first, with their app sessions.
    pub fn snapshot(&self) -> Result<Snapshot, AudioError> {
        #[cfg(windows)]
        return self.inner.snapshot();
        #[cfg(not(windows))]
        Err(AudioError::Unsupported)
    }
    pub fn set_device_volume(&self, device_id: &str, volume: f32) -> Result<(), AudioError> {
        #[cfg(windows)]
        return self
            .inner
            .set_device_volume(device_id, volume.clamp(0.0, 1.0));
        #[cfg(not(windows))]
        {
            let _ = (device_id, volume);
            Err(AudioError::Unsupported)
        }
    }
    pub fn set_device_mute(&self, device_id: &str, muted: bool) -> Result<(), AudioError> {
        #[cfg(windows)]
        return self.inner.set_device_mute(device_id, muted);
        #[cfg(not(windows))]
        {
            let _ = (device_id, muted);
            Err(AudioError::Unsupported)
        }
    }
    pub fn set_app_volume(
        &self,
        device_id: &str,
        app_key: &str,
        volume: f32,
    ) -> Result<(), AudioError> {
        #[cfg(windows)]
        return self
            .inner
            .set_app_volume(device_id, app_key, volume.clamp(0.0, 1.0));
        #[cfg(not(windows))]
        {
            let _ = (device_id, app_key, volume);
            Err(AudioError::Unsupported)
        }
    }
    pub fn set_app_mute(
        &self,
        device_id: &str,
        app_key: &str,
        muted: bool,
    ) -> Result<(), AudioError> {
        #[cfg(windows)]
        return self.inner.set_app_mute(device_id, app_key, muted);
        #[cfg(not(windows))]
        {
            let _ = (device_id, app_key, muted);
            Err(AudioError::Unsupported)
        }
    }
}
