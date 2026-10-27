//! AppSettings hidden-app/device cache and mutation lifecycles. Reads return
//! copies; writes/notifications are effects for the native destination adapter.
use crate::{
    app_rules::{Effect, Environment},
    models::{HiddenAppEntry, HiddenDeviceEntry},
    newtonsoft,
    rules::normalize_key,
    typed_lists,
};
use serde::Serialize;
use std::cmp::Ordering;

#[derive(Clone, Debug, Default)]
struct Cache<T> {
    raw: Option<String>,
    entries: Option<Vec<T>>,
    effects: Vec<Effect>,
}
impl<T> Cache<T> {
    fn load(&mut self, decode: impl FnOnce(Option<&str>) -> Vec<T>) -> &mut Vec<T> {
        if self.entries.is_none() {
            self.entries = Some(decode(self.raw.as_deref()));
        }
        self.entries.as_mut().unwrap()
    }
    fn stored_json(&self) -> &str {
        self.raw.as_deref().unwrap_or("[]")
    }
    fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }
}
impl<T: Serialize> Cache<T> {
    fn changed(&mut self) {
        let json = serde_json::to_string(self.entries.as_ref().expect("loaded hidden cache"))
            .expect("serializable hidden DTOs");
        self.raw = Some(json.clone());
        self.effects.extend([Effect::Write(json), Effect::Changed]);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AppKey<'a> {
    pub device_id: Option<&'a str>,
    pub app_id: Option<&'a str>,
    pub exe_name: Option<&'a str>,
}
impl AppKey<'_> {
    fn normalized(self) -> Option<(String, String, String)> {
        let device = normalize_key(self.device_id);
        let app = normalize_key(self.app_id);
        let exe = normalize_key(self.exe_name);
        if device.is_empty() || (app.is_empty() && exe.is_empty()) {
            None
        } else {
            Some((device, app, exe))
        }
    }
}
fn exact(entry: &HiddenAppEntry, device: &str, app: &str, exe: &str) -> bool {
    entry.device_id.as_deref() == Some(device)
        && entry.app_id.as_deref() == Some(app)
        && entry.exe_name.as_deref() == Some(exe)
}
fn text(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or_default()
}

#[derive(Clone, Debug, Default)]
pub struct HiddenApps(Cache<HiddenAppEntry>);
impl HiddenApps {
    pub fn new(raw: Option<String>) -> Self {
        Self(Cache {
            raw,
            ..Cache::default()
        })
    }
    pub fn stored_json(&self) -> &str {
        self.0.stored_json()
    }
    pub fn take_effects(&mut self) -> Vec<Effect> {
        self.0.take_effects()
    }
    fn entries(&mut self, env: &Environment<'_>) -> &mut Vec<HiddenAppEntry> {
        self.0
            .load(|raw| typed_lists::load_hidden_apps(raw, env.zone, env.today_ticks))
    }
    pub fn count(&mut self, env: &Environment<'_>) -> usize {
        self.entries(env).len()
    }
    pub fn count_for_device(&mut self, device: Option<&str>, env: &Environment<'_>) -> usize {
        let device = normalize_key(device);
        if device.is_empty() {
            return 0;
        }
        self.entries(env)
            .iter()
            .filter(|e| text(&e.device_id) == device)
            .count()
    }
    pub fn is_hidden(&mut self, key: AppKey<'_>, env: &Environment<'_>) -> bool {
        let Some((device, app, exe)) = key.normalized() else {
            return false;
        };
        self.entries(env).iter().any(|e| {
            text(&e.device_id) == device
                && ((!app.is_empty() && text(&e.app_id) == app)
                    || (!exe.is_empty() && text(&e.exe_name) == exe))
        })
    }
    pub fn hide(&mut self, key: AppKey<'_>, display: Option<&str>, env: &Environment<'_>) {
        let Some((device, app, exe)) = key.normalized() else {
            return;
        };
        let entries = self.entries(env);
        if entries.iter().any(|e| exact(e, &device, &app, &exe)) {
            return;
        }
        // Add compares the three fields directly. Do not normalize/dedupe the
        // entire list here: the legacy delimiter collision occurs on load only.
        entries.push(HiddenAppEntry {
            device_id: Some(device),
            app_id: Some(app),
            exe_name: Some(exe),
            display_name: Some(display.unwrap_or_default().trim().into()),
            hidden_at_utc: newtonsoft::format_date(&env.now_utc),
        });
        self.0.changed();
    }
    pub fn unhide(&mut self, key: AppKey<'_>, env: &Environment<'_>) {
        let Some((device, app, exe)) = key.normalized() else {
            return;
        };
        let entries = self.entries(env);
        let before = entries.len();
        entries.retain(|e| !exact(e, &device, &app, &exe));
        if before != entries.len() {
            self.0.changed();
        }
    }
    pub fn unhide_device(&mut self, device: Option<&str>, env: &Environment<'_>) {
        let device = normalize_key(device);
        if device.is_empty() {
            return;
        }
        let entries = self.entries(env);
        let before = entries.len();
        entries.retain(|e| text(&e.device_id) != device);
        if before != entries.len() {
            self.0.changed();
        }
    }
    pub fn clear(&mut self, env: &Environment<'_>) {
        let entries = self.entries(env);
        if !entries.is_empty() {
            entries.clear();
            self.0.changed();
        }
    }
    pub fn sorted(
        &mut self,
        env: &Environment<'_>,
        mut compare: impl FnMut(&str, &str) -> Ordering,
    ) -> Vec<HiddenAppEntry> {
        let mut entries = self.entries(env).clone();
        sort_apps(&mut entries, &mut compare, true);
        entries
    }
    pub fn for_device(
        &mut self,
        device: Option<&str>,
        env: &Environment<'_>,
        mut compare: impl FnMut(&str, &str) -> Ordering,
    ) -> Vec<HiddenAppEntry> {
        let device = normalize_key(device);
        if device.is_empty() {
            return vec![];
        }
        let mut entries: Vec<_> = self
            .entries(env)
            .iter()
            .filter(|e| text(&e.device_id) == device)
            .cloned()
            .collect();
        sort_apps(&mut entries, &mut compare, false);
        entries
    }
}
fn sort_apps(
    entries: &mut [HiddenAppEntry],
    compare: &mut impl FnMut(&str, &str) -> Ordering,
    include_device: bool,
) {
    entries.sort_by(|a, b| {
        let a = [&a.device_id, &a.display_name, &a.exe_name, &a.app_id];
        let b = [&b.device_id, &b.display_name, &b.exe_name, &b.app_id];
        for i in usize::from(!include_device)..4 {
            let order = compare(text(a[i]), text(b[i]));
            if order != Ordering::Equal {
                return order;
            }
        }
        Ordering::Equal
    });
}

#[derive(Clone, Debug, Default)]
pub struct HiddenDevices(Cache<HiddenDeviceEntry>);
impl HiddenDevices {
    pub fn new(raw: Option<String>) -> Self {
        Self(Cache {
            raw,
            ..Cache::default()
        })
    }
    pub fn stored_json(&self) -> &str {
        self.0.stored_json()
    }
    pub fn take_effects(&mut self) -> Vec<Effect> {
        self.0.take_effects()
    }
    fn entries(&mut self, env: &Environment<'_>) -> &mut Vec<HiddenDeviceEntry> {
        self.0
            .load(|raw| typed_lists::load_hidden_devices(raw, env.zone, env.today_ticks))
    }
    pub fn count(&mut self, env: &Environment<'_>) -> usize {
        self.entries(env).len()
    }
    pub fn is_hidden(&mut self, device: Option<&str>, env: &Environment<'_>) -> bool {
        let device = normalize_key(device);
        !device.is_empty()
            && self
                .entries(env)
                .iter()
                .any(|e| text(&e.device_id) == device)
    }
    pub fn hide(&mut self, device: Option<&str>, display: Option<&str>, env: &Environment<'_>) {
        let device = normalize_key(device);
        if device.is_empty() {
            return;
        }
        let entries = self.entries(env);
        if entries.iter().any(|e| text(&e.device_id) == device) {
            return;
        }
        entries.push(HiddenDeviceEntry {
            device_id: Some(device),
            display_name: Some(display.unwrap_or_default().trim().into()),
            hidden_at_utc: newtonsoft::format_date(&env.now_utc),
        });
        self.0.changed();
    }
    pub fn unhide(&mut self, device: Option<&str>, env: &Environment<'_>) {
        let device = normalize_key(device);
        if device.is_empty() {
            return;
        }
        let entries = self.entries(env);
        let before = entries.len();
        entries.retain(|e| text(&e.device_id) != device);
        if before != entries.len() {
            self.0.changed();
        }
    }
    pub fn clear(&mut self, env: &Environment<'_>) {
        let entries = self.entries(env);
        if !entries.is_empty() {
            entries.clear();
            self.0.changed();
        }
    }
    pub fn sorted(
        &mut self,
        env: &Environment<'_>,
        mut compare: impl FnMut(&str, &str) -> Ordering,
    ) -> Vec<HiddenDeviceEntry> {
        let mut entries = self.entries(env).clone();
        entries.sort_by(|a, b| {
            compare(text(&a.display_name), text(&b.display_name))
                .then_with(|| compare(text(&a.device_id), text(&b.device_id)))
        });
        entries
    }
}
