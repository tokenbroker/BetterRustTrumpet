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
mod notify;
#[cfg(windows)]
mod windows_impl;

/// What changed, from Windows' own notifications (no polling).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// A device was added/removed/enabled/disabled or the default changed.
    Devices,
    /// An endpoint's volume or mute changed.
    Volume,
}

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
    /// Output the app is routed to, when not following the default.
    pub routed_device_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    /// Device description and enumerator ("Headset Earphone", "USB"), which
    /// the original's tray tooltip shows; empty when Windows has none.
    pub description: String,
    pub enumerator: String,
    pub is_default: bool,
    pub volume: f32,
    pub muted: bool,
    pub peak: f32,
    pub apps: Vec<AppSession>,
}

/// Left/right peak levels of a device (`app_key` = `None`) or an app.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Peaks {
    pub device_id: String,
    pub app_key: Option<String>,
    pub channels: [f32; 2],
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
    /// Devices only (no app sessions): cheap enough for background polling.
    pub fn devices(&self) -> Result<Snapshot, AudioError> {
        #[cfg(windows)]
        return self.inner.devices();
        #[cfg(not(windows))]
        Err(AudioError::Unsupported)
    }
    /// Current left/right peaks for the devices and apps found by the last
    /// [`snapshot`](Self::snapshot); far cheaper than a snapshot, for meters.
    pub fn peaks(&self) -> Result<Vec<Peaks>, AudioError> {
        #[cfg(windows)]
        return self.inner.peaks();
        #[cfg(not(windows))]
        Err(AudioError::Unsupported)
    }
    /// Makes `device_id` the default output for the console and multimedia
    /// roles, as the original does.
    pub fn set_default_device(&self, device_id: &str) -> Result<(), AudioError> {
        #[cfg(windows)]
        return self.inner.set_default_device(device_id);
        #[cfg(not(windows))]
        {
            let _ = device_id;
            Err(AudioError::Unsupported)
        }
    }
    /// Calls `listener` from a system thread whenever devices, the default
    /// device or an endpoint's volume/mute change.
    pub fn watch(
        &self,
        listener: impl Fn(Change) + Send + Sync + 'static,
    ) -> Result<(), AudioError> {
        #[cfg(windows)]
        return self.inner.watch(std::sync::Arc::new(move |c| {
            listener(match c {
                notify::Change::Devices => Change::Devices,
                notify::Change::Volume => Change::Volume,
            })
        }));
        #[cfg(not(windows))]
        {
            let _ = listener;
            Err(AudioError::Unsupported)
        }
    }
    /// Routes an app (all its processes) to `device_id`, or back to the
    /// default output with `None`.
    pub fn set_app_device(&self, app_key: &str, device_id: Option<&str>) -> Result<(), AudioError> {
        #[cfg(windows)]
        return self.inner.set_app_device(app_key, device_id);
        #[cfg(not(windows))]
        {
            let _ = (app_key, device_id);
            Err(AudioError::Unsupported)
        }
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
