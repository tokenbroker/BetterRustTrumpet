//! AppSettings folder-rule persistence: `FolderVolumeRuleEntriesJson` is read
//! with `DeserializeObject<List<FolderVolumeRuleEntry>>`, normalized, and
//! rewritten with `SerializeObject` after each change. Any load error,
//! including a single bad element, yields an empty list (the original catches
//! it). Pinned by folder_rules_reference.json.
use crate::{
    dotnet_datetime,
    folders::{self, PathError, Rule, WindowsPathResolver},
    newtonsoft::{self, Date, DateKind, LocalZone, Token},
    text::{is_blank, ordinal_ignore_case, ordinal_key},
};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq)]
pub struct FolderRule {
    pub id: Option<String>,
    pub folder_path: Option<String>,
    pub volume_percent: i32,
    pub created_at_utc: Date,
}
const MIN_DATE: Date = Date {
    ticks: 0,
    kind: DateKind::Unspecified,
};
impl Default for FolderRule {
    fn default() -> Self {
        Self {
            id: None,
            folder_path: None,
            volume_percent: 0,
            created_at_utc: MIN_DATE,
        }
    }
}

/// Native boundaries: path resolution, local time zone, the local date used
/// by `DateTime.TryParse`, the UTC clock and GUID allocation (`ToString("N")`).
pub struct Environment<'a, R: ?Sized, Z: ?Sized> {
    pub resolver: &'a R,
    pub zone: &'a Z,
    pub today_ticks: i64,
    pub now_utc_ticks: i64,
    pub new_id: &'a mut dyn FnMut() -> String,
}

fn rule(
    token: &Token,
    zone: &(impl LocalZone + ?Sized),
    today: i64,
) -> Result<FolderRule, newtonsoft::Error> {
    let Token::Object(members) = token else {
        return Err(newtonsoft::Error);
    };
    let mut value = FolderRule::default();
    let names = ["Id", "FolderPath", "VolumePercent", "CreatedAtUtc"];
    for (name, member) in members {
        match newtonsoft::member_index(&names, name) {
            Some(0) => value.id = newtonsoft::typed_string(member)?,
            Some(1) => value.folder_path = newtonsoft::typed_string(member)?,
            Some(2) => {
                value.volume_percent = newtonsoft::typed_int32(member)?.ok_or(newtonsoft::Error)?
            }
            Some(_) => {
                value.created_at_utc = newtonsoft::typed_datetime(member, zone, |text| {
                    dotnet_datetime::try_parse(text, today, zone)
                })?
                .ok_or(newtonsoft::Error)?;
            }
            None => newtonsoft::check_read(member, zone)?,
        }
    }
    Ok(value)
}

/// `DeserializeObject<List<FolderVolumeRuleEntry>>`; `Ok(None)` is a null list.
pub fn deserialize(
    json: &str,
    zone: &(impl LocalZone + ?Sized),
    today_ticks: i64,
) -> Result<Option<Vec<Option<FolderRule>>>, newtonsoft::Error> {
    let Some(root) = newtonsoft::parse_typed_document(json, zone)? else {
        return Ok(None);
    };
    let Token::Array(items) = root else {
        return Err(newtonsoft::Error);
    };
    items
        .iter()
        .map(|item| match item {
            Token::Null | Token::Undefined => Ok(None),
            item => rule(item, zone, today_ticks).map(Some),
        })
        .collect::<Result<_, _>>()
        .map(Some)
}

fn optional(value: Option<&str>) -> String {
    value.map_or_else(|| "null".into(), newtonsoft::quote)
}
fn entry_json(rule: &FolderRule) -> String {
    format!(
        "{{\"Id\":{},\"FolderPath\":{},\"VolumePercent\":{},\"CreatedAtUtc\":{}}}",
        optional(rule.id.as_deref()),
        optional(rule.folder_path.as_deref()),
        rule.volume_percent,
        newtonsoft::quote(&newtonsoft::format_date(&rule.created_at_utc))
    )
}
/// `SerializeObject` of a list that may contain null elements.
pub fn serialize_optional(rules: &[Option<FolderRule>]) -> String {
    let items: Vec<String> = rules
        .iter()
        .map(|r| r.as_ref().map_or_else(|| "null".into(), entry_json))
        .collect();
    format!("[{}]", items.join(","))
}
pub fn serialize(rules: &[FolderRule]) -> String {
    let items: Vec<String> = rules.iter().map(entry_json).collect();
    format!("[{}]", items.join(","))
}

/// `NormalizeFolderPath`; the original maps ArgumentException and
/// NotSupportedException to "" and lets other resolver errors escape.
fn normalize_path(
    path: Option<&str>,
    resolver: &(impl WindowsPathResolver + ?Sized),
) -> Result<String, PathError> {
    folders::normalize_folder(path, &Resolver(resolver))
}
struct Resolver<'a, R: ?Sized>(&'a R);
impl<R: WindowsPathResolver + ?Sized> WindowsPathResolver for Resolver<'_, R> {
    fn full_path(&self, value: &str) -> Result<String, PathError> {
        self.0.full_path(value)
    }
}

/// Result of a mutating call: whether the store value was written and
/// whether `FolderVolumeRulesChanged` fired.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub saved: bool,
    pub changed: bool,
}
const CHANGED: Outcome = Outcome {
    saved: true,
    changed: true,
};

/// The original's in-memory folder-rule state over one stored string. The
/// store adapter persists `stored()` after any outcome with `saved`.
#[derive(Clone, Debug, Default)]
pub struct FolderRuleStore {
    stored: Option<String>,
    loaded: Option<Vec<FolderRule>>,
}

impl FolderRuleStore {
    /// `stored` is the bag's string value for the key, if present.
    pub fn new(stored: Option<String>) -> Self {
        Self {
            stored,
            loaded: None,
        }
    }
    pub fn stored(&self) -> Option<&str> {
        self.stored.as_deref()
    }
    fn rules<R: WindowsPathResolver + ?Sized, Z: LocalZone + ?Sized>(
        &mut self,
        env: &mut Environment<'_, R, Z>,
    ) -> &mut Vec<FolderRule> {
        if self.loaded.is_none() {
            let json = self.stored.as_deref().unwrap_or("[]");
            let loaded = Self::load(json, env).unwrap_or_default();
            self.loaded = Some(loaded);
        }
        self.loaded.as_mut().expect("loaded")
    }
    fn load<R: WindowsPathResolver + ?Sized, Z: LocalZone + ?Sized>(
        json: &str,
        env: &mut Environment<'_, R, Z>,
    ) -> Option<Vec<FolderRule>> {
        let entries = deserialize(json, env.zone, env.today_ticks)
            .ok()?
            .unwrap_or_default();
        let mut known = HashSet::new();
        let mut normalized = vec![];
        for entry in entries.into_iter().flatten() {
            let folder = normalize_path(entry.folder_path.as_deref(), env.resolver).ok()?;
            if folder.is_empty() || !known.insert(ordinal_key(&folder)) {
                continue;
            }
            normalized.push(FolderRule {
                id: Some(if is_blank(entry.id.as_deref()) {
                    (env.new_id)()
                } else {
                    entry.id.unwrap_or_default()
                }),
                folder_path: Some(folder),
                volume_percent: entry.volume_percent.clamp(0, 100),
                created_at_utc: if entry.created_at_utc.ticks == 0 {
                    Date {
                        ticks: env.now_utc_ticks,
                        kind: DateKind::Utc,
                    }
                } else {
                    entry.created_at_utc
                },
            });
        }
        Some(normalized)
    }
    fn save(&mut self) -> Outcome {
        self.stored = Some(serialize(self.loaded.as_deref().unwrap_or_default()));
        CHANGED
    }
    /// `FolderVolumeRulesJson` getter.
    pub fn rules_json<R: WindowsPathResolver + ?Sized, Z: LocalZone + ?Sized>(
        &mut self,
        env: &mut Environment<'_, R, Z>,
    ) -> String {
        serialize(self.rules(env))
    }
    /// `FolderVolumeRulesJson` setter: always notifies, reloads lazily.
    pub fn set_json(&mut self, value: Option<&str>) -> Outcome {
        self.stored = Some(if is_blank(value) {
            "[]".into()
        } else {
            value.unwrap_or_default().into()
        });
        self.loaded = None;
        CHANGED
    }
    /// `GetFolderVolumeRules`: a copy ordered by creation time (stable; .NET
    /// DateTime comparison ignores the kind).
    pub fn sorted<R: WindowsPathResolver + ?Sized, Z: LocalZone + ?Sized>(
        &mut self,
        env: &mut Environment<'_, R, Z>,
    ) -> Vec<FolderRule> {
        let mut rules = self.rules(env).clone();
        rules.sort_by_key(|rule| rule.created_at_utc.ticks);
        rules
    }
    pub fn add<R: WindowsPathResolver + ?Sized, Z: LocalZone + ?Sized>(
        &mut self,
        folder: Option<&str>,
        volume_percent: i32,
        env: &mut Environment<'_, R, Z>,
    ) -> Result<Outcome, PathError> {
        let folder = normalize_path(folder, env.resolver)?;
        if folder.is_empty() {
            return Ok(Outcome::default());
        }
        let volume = volume_percent.clamp(0, 100);
        let now = env.now_utc_ticks;
        let rules = self.rules(env);
        match rules.iter_mut().find(|r| {
            r.folder_path
                .as_deref()
                .is_some_and(|p| ordinal_ignore_case(p, &folder))
        }) {
            Some(existing) if existing.volume_percent == volume => return Ok(Outcome::default()),
            Some(existing) => existing.volume_percent = volume,
            None => {
                let id = (env.new_id)();
                self.rules(env).push(FolderRule {
                    id: Some(id),
                    folder_path: Some(folder),
                    volume_percent: volume,
                    created_at_utc: Date {
                        ticks: now,
                        kind: DateKind::Utc,
                    },
                });
            }
        }
        Ok(self.save())
    }
    pub fn update<R: WindowsPathResolver + ?Sized, Z: LocalZone + ?Sized>(
        &mut self,
        id: Option<&str>,
        folder: Option<&str>,
        volume_percent: i32,
        env: &mut Environment<'_, R, Z>,
    ) -> Result<Outcome, PathError> {
        if is_blank(id) {
            return Ok(Outcome::default());
        }
        let folder = normalize_path(folder, env.resolver)?;
        if folder.is_empty() {
            return Ok(Outcome::default());
        }
        let volume = volume_percent.clamp(0, 100);
        let Some(rule) = self.rules(env).iter_mut().find(|r| r.id.as_deref() == id) else {
            return Ok(Outcome::default());
        };
        let same_folder = rule
            .folder_path
            .as_deref()
            .is_some_and(|p| ordinal_ignore_case(p, &folder));
        if same_folder && rule.volume_percent == volume {
            return Ok(Outcome::default());
        }
        rule.folder_path = Some(folder);
        rule.volume_percent = volume;
        Ok(self.save())
    }
    pub fn remove<R: WindowsPathResolver + ?Sized, Z: LocalZone + ?Sized>(
        &mut self,
        id: Option<&str>,
        env: &mut Environment<'_, R, Z>,
    ) -> Outcome {
        if is_blank(id) {
            return Outcome::default();
        }
        let rules = self.rules(env);
        let before = rules.len();
        rules.retain(|r| r.id.as_deref() != id);
        if rules.len() == before {
            return Outcome::default();
        }
        self.save()
    }
    /// `TryGetFolderVolume`: the matched volume, if any.
    pub fn try_match<R: WindowsPathResolver + ?Sized, Z: LocalZone + ?Sized>(
        &mut self,
        executable: Option<&str>,
        env: &mut Environment<'_, R, Z>,
    ) -> Result<Option<i32>, PathError> {
        let rules: Vec<Rule> = self
            .rules(env)
            .iter()
            .map(|r| Rule {
                folder_path: r.folder_path.clone(),
                volume_percent: r.volume_percent,
                created_at_ticks: r.created_at_utc.ticks,
            })
            .collect();
        folders::try_match(executable, &rules, &Resolver(env.resolver))
    }
}
