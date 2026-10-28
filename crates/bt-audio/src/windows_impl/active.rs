//! App sessions that are producing sound right now (AudioSessionStateActive):
//! what the legacy media-player fallback (LegacyMediaPlayerService) reads.
use super::{Backend, Engine};
use crate::{ActiveSession, AudioError};
use windows::Win32::Media::Audio::{DEVICE_STATE_ACTIVE, eRender};

impl Backend {
    /// Active sessions of every active output, in endpoint then session
    /// order (FindActiveLegacyPlayer's iteration order).
    fn active_sessions(&self) -> Result<Vec<ActiveSession>, AudioError> {
        let mut out = vec![];
        // SAFETY: live enumerator/collection on this thread.
        let collection = unsafe {
            self.enumerator
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?
        };
        for i in 0..unsafe { collection.GetCount()? } {
            // SAFETY: index within GetCount.
            let endpoint = unsafe { collection.Item(i)? };
            for session in self.device_sessions(&endpoint)? {
                if !session.active || session.system {
                    continue;
                }
                if let Some(executable) = session.executable {
                    out.push(ActiveSession {
                        executable,
                        display_name: session.display_name,
                        process_id: session.pid,
                    });
                }
            }
        }
        Ok(out)
    }
}

impl Engine {
    pub fn active_sessions(&self) -> Result<Vec<ActiveSession>, AudioError> {
        self.call(Backend::active_sessions)
    }
}
