//! VolumeUndoService.cs behavior with an injected wall-clock timestamp.
//! The app owns applying returned actions to live audio objects.
use std::collections::VecDeque;

pub const MAX_STACK_DEPTH: usize = 50;
pub const COALESCE_WINDOW_MS: i64 = 500;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub id: String,
    pub display_name: String,
    pub is_device: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Volume { old: i32, new: i32 },
    Mute { old: bool, new: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeAction {
    pub target: Target,
    pub change: Change,
    /// Milliseconds in the app's wall-clock domain, matching DateTime.Now.
    pub timestamp_ms: i64,
}

#[derive(Default, Debug)]
pub struct UndoHistory {
    undo: VecDeque<VolumeAction>,
    redo: Vec<VolumeAction>,
    replaying: bool,
    revision: u64,
}

impl UndoHistory {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_count(&self) -> usize {
        self.undo.len()
    }
    pub fn redo_count(&self) -> usize {
        self.redo.len()
    }
    /// Incremented whenever the C# service would raise StateChanged.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn begin_undo_redo(&mut self) {
        self.replaying = true;
    }
    pub fn end_undo_redo(&mut self) {
        self.replaying = false;
    }

    pub fn record_volume_change(
        &mut self,
        target: Target,
        old: i32,
        new: i32,
        timestamp_ms: i64,
    ) -> bool {
        if self.replaying || old == new {
            return false;
        }
        if let Some(last) = self.undo.back_mut() {
            // The source compares TargetId only, not IsDevice or display name,
            // and a backwards wall-clock step also meets its <500ms test.
            if last.target.id == target.id
                && (timestamp_ms as i128 - last.timestamp_ms as i128) < COALESCE_WINDOW_MS as i128
            {
                if let Change::Volume { new: last_new, .. } = &mut last.change {
                    *last_new = new;
                    last.timestamp_ms = timestamp_ms;
                    self.redo.clear();
                    self.changed();
                    return true;
                }
            }
        }
        self.push(VolumeAction {
            target,
            change: Change::Volume { old, new },
            timestamp_ms,
        });
        true
    }

    pub fn record_mute_change(
        &mut self,
        target: Target,
        old: bool,
        new: bool,
        timestamp_ms: i64,
    ) -> bool {
        if self.replaying || old == new {
            return false;
        }
        self.push(VolumeAction {
            target,
            change: Change::Mute { old, new },
            timestamp_ms,
        });
        true
    }

    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    fn push(&mut self, action: VolumeAction) {
        self.undo.push_back(action);
        if self.undo.len() > MAX_STACK_DEPTH {
            self.undo.pop_front();
        }
        self.redo.clear();
        self.changed();
    }

    pub fn undo(&mut self) -> Option<VolumeAction> {
        let action = self.undo.pop_back()?;
        self.redo.push(action.clone());
        self.changed();
        Some(action)
    }

    pub fn redo(&mut self) -> Option<VolumeAction> {
        let action = self.redo.pop()?;
        self.undo.push_back(action.clone());
        self.changed();
        Some(action)
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.changed();
    }

    pub fn undo_description(&self) -> Option<String> {
        self.undo.back().map(|a| description("Undo", a))
    }

    pub fn redo_description(&self) -> Option<String> {
        self.redo.last().map(|a| description("Redo", a))
    }
}

fn description(verb: &str, action: &VolumeAction) -> String {
    let name = &action.target.display_name;
    match action.change {
        Change::Volume { old, new } => format!("{verb} volume {name} ({old}→{new})"),
        Change::Mute { new, .. } => {
            format!("{verb} {} {name}", if new { "mute" } else { "unmute" })
        }
    }
}
