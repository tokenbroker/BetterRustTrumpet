//! CLI/CliHandler.cs rule language, matching and snapshot planning. These DTOs
//! contain no live audio handles; the engine applies returned changes later.
use crate::{
    cli::is_flag,
    identity::filename_without_extension,
    text::{
        invariant_lower, is_blank, ordinal_compare, ordinal_contains, ordinal_ignore_case,
        ordinal_ignore_case_optional, ordinal_key, ordinal_starts_with,
    },
};
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::HashSet};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Assignment {
    pub query: Option<String>,
    pub volume: Option<i32>,
    pub is_muted: Option<bool>,
    pub is_others: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Spec {
    pub keep: Vec<Assignment>,
    pub others: Option<Assignment>,
    pub capture_all_devices: bool,
    pub apply_apps_only: bool,
}
impl Default for Spec {
    fn default() -> Self {
        Self {
            keep: vec![],
            others: None,
            capture_all_devices: true,
            apply_apps_only: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct App {
    pub app_id: Option<String>,
    pub exe_name: Option<String>,
    pub display_name: Option<String>,
    pub process_id: i32,
    pub volume: i32,
    pub is_muted: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Device {
    pub id: Option<String>,
    pub display_name: Option<String>,
    pub apps: Option<Vec<Option<App>>>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Collection {
    pub all_devices: Option<Vec<Option<Device>>>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct AppMatch {
    pub query: Option<String>,
    pub match_type: String,
    pub score: i32,
    pub app_id: Option<String>,
    pub exe_name: Option<String>,
    pub display_name: Option<String>,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub process_id: i32,
    pub volume: i32,
    pub is_muted: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct PlanChange {
    pub scope: String,
    pub rule: String,
    pub query: Option<String>,
    pub match_type: String,
    pub app_id: Option<String>,
    pub exe_name: Option<String>,
    pub display_name: Option<String>,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub process_id: i32,
    pub volume_before: i32,
    pub volume_after: i32,
    pub is_muted_before: bool,
    pub is_muted_after: bool,
}

pub const USAGE: &str = "usage: rule-preview --keep APP=VOL [--keep APP=VOL] [--others VOL] [--apps-only] [--current-device]";

fn target(value: &str, assignment: &mut Assignment) -> Result<(), String> {
    if is_blank(Some(value)) {
        return Err("missing assignment value".into());
    }
    let normalized = invariant_lower(value.trim());
    match normalized.as_str() {
        "mute" | "muted" => assignment.is_muted = Some(true),
        "unmute" | "unmuted" => assignment.is_muted = Some(false),
        _ => {
            // int.TryParse(NumberStyles.Integer): ASCII digits/whitespace and
            // trailing NUL compatibility. Non-en-US number-format contracts
            // still need implementation/fixtures before the CLI gate closes.
            let numeric = normalized
                .trim_end_matches('%')
                .trim_end_matches('\0')
                .trim_matches(|c: char| c == ' ' || ('\t'..='\r').contains(&c));
            let volume = numeric
                .parse::<i32>()
                .ok()
                .filter(|n| (0..=100).contains(n))
                .ok_or_else(|| "volume must be 0-100, mute, or unmute".to_owned())?;
            assignment.volume = Some(volume);
            assignment.is_muted = Some(false);
        }
    }
    Ok(())
}

pub fn parse_assignment(value: &str, is_others: bool) -> Result<Assignment, String> {
    let mut assignment = Assignment {
        is_others,
        ..Default::default()
    };
    if is_blank(Some(value)) {
        return Err("empty assignment".into());
    }
    if is_others && !value.contains('=') {
        target(value, &mut assignment)?;
        return Ok(assignment);
    }
    let (query, state) = value
        .split_once('=')
        .filter(|(query, _)| !is_blank(Some(query)))
        .ok_or_else(|| "assignment must be APP=VOL, APP=mute, or APP=unmute".to_owned())?;
    let query = query.trim();
    assignment.is_others |= ordinal_ignore_case(query, "others") || query == "*";
    assignment.query = Some(query.into());
    target(state, &mut assignment)?;
    Ok(assignment)
}

pub fn parse(args: &[String]) -> Result<Spec, String> {
    if args.is_empty() {
        return Err(USAGE.into());
    }
    let mut spec = Spec::default();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        index += 1;
        if is_flag(arg, "--apps-only") {
            spec.apply_apps_only = true;
        } else if is_flag(arg, "--all-devices") {
            spec.capture_all_devices = true;
        } else if is_flag(arg, "--current-device") {
            spec.capture_all_devices = false;
        } else if is_flag(arg, "--keep") || is_flag(arg, "--app") {
            let value = args
                .get(index)
                .ok_or_else(|| format!("{arg} requires APP=VOL"))?;
            index += 1;
            spec.keep.push(parse_assignment(value, false)?);
        } else if is_flag(arg, "--others") {
            let value = args.get(index).ok_or("--others requires VOL")?;
            index += 1;
            spec.others = Some(parse_assignment(value, true)?);
        } else if let Ok(assignment) = parse_assignment(arg, false) {
            if assignment.is_others {
                return Err(format!("unknown rule argument: {arg}"));
            }
            spec.keep.push(assignment);
        } else {
            return Err(format!("unknown rule argument: {arg}"));
        }
    }
    if spec.keep.is_empty() && spec.others.is_none() {
        return Err("rule needs at least one --keep APP=VOL or --others VOL".into());
    }
    Ok(spec)
}

fn token(value: Option<&str>) -> String {
    if is_blank(value) {
        String::new()
    } else {
        invariant_lower(&filename_without_extension(
            value.unwrap_or_default().trim(),
        ))
    }
}

pub fn create_match(device: &Device, app: &App, query: Option<&str>) -> AppMatch {
    let normalized = token(query);
    let exe = filename_without_extension(app.exe_name.as_deref().unwrap_or_default());
    let candidates = [
        (app.exe_name.as_deref(), "exe"),
        (Some(exe.as_str()), "exeName"),
        (app.display_name.as_deref(), "displayName"),
        (app.app_id.as_deref(), "appId"),
    ];
    let mut best_score = 0;
    let mut best_type = "none";
    for (value, kind) in candidates {
        let value = token(value);
        if is_blank(Some(&value)) || is_blank(Some(&normalized)) {
            continue;
        }
        let score = if ordinal_ignore_case(&value, &normalized) {
            100
        } else if ordinal_starts_with(&value, &normalized) {
            80
        } else if ordinal_contains(&value, &normalized) {
            60
        } else {
            0
        };
        if score > best_score {
            best_score = score;
            best_type = kind;
        }
    }
    AppMatch {
        query: query.map(str::to_owned),
        match_type: best_type.into(),
        score: best_score,
        app_id: app.app_id.clone(),
        exe_name: app.exe_name.clone(),
        display_name: app.display_name.clone(),
        device_id: device.id.clone(),
        device_name: device.display_name.clone(),
        process_id: app.process_id,
        volume: app.volume,
        is_muted: app.is_muted,
    }
}

impl Collection {
    fn devices(&self) -> impl Iterator<Item = &Device> {
        self.all_devices.iter().flatten().flatten()
    }
}

pub fn find_matches(collection: Option<&Collection>, query: Option<&str>) -> Vec<AppMatch> {
    if is_blank(query) {
        return vec![];
    }
    let mut matches: Vec<_> = collection
        .into_iter()
        .flat_map(Collection::devices)
        .flat_map(|device| {
            device
                .apps
                .iter()
                .flatten()
                .flatten()
                .map(move |app| create_match(device, app, query))
        })
        .filter(|m| m.score > 0)
        .collect();
    matches.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| match (&a.display_name, &b.display_name) {
                (None, None) => Ordering::Equal,
                (None, _) => Ordering::Less,
                (_, None) => Ordering::Greater,
                (Some(a), Some(b)) => ordinal_compare(a, b),
            })
    });
    matches
}

fn match_key(app_id: Option<&str>, exe: Option<&str>, pid: i32, device: Option<&str>) -> String {
    app_id.map(str::to_owned).unwrap_or_else(|| {
        format!(
            "{}:{pid}:{}",
            exe.unwrap_or_default(),
            device.unwrap_or_default()
        )
    })
}
fn change(m: AppMatch, assignment: &Assignment, rule: &str) -> PlanChange {
    PlanChange {
        scope: "app".into(),
        rule: rule.into(),
        query: assignment.query.clone(),
        match_type: m.match_type,
        app_id: m.app_id,
        exe_name: m.exe_name,
        display_name: m.display_name,
        device_id: m.device_id,
        device_name: m.device_name,
        process_id: m.process_id,
        volume_before: m.volume,
        volume_after: assignment.volume.unwrap_or(m.volume),
        is_muted_before: m.is_muted,
        is_muted_after: assignment.is_muted.unwrap_or(m.is_muted),
    }
}

pub fn build_plan(collection: &Collection, spec: &Spec) -> Vec<PlanChange> {
    let mut changes = Vec::new();
    let mut keep_ids = HashSet::new();
    for assignment in &spec.keep {
        for m in find_matches(Some(collection), assignment.query.as_deref()) {
            keep_ids.insert(ordinal_key(&match_key(
                m.app_id.as_deref(),
                m.exe_name.as_deref(),
                m.process_id,
                m.device_id.as_deref(),
            )));
            changes.push(change(m, assignment, "keep"));
        }
    }
    if let Some(others) = &spec.others {
        for device in collection.devices() {
            for app in device.apps.iter().flatten().flatten() {
                let m = create_match(
                    device,
                    app,
                    app.exe_name.as_deref().or(app.display_name.as_deref()),
                );
                if !keep_ids.contains(&ordinal_key(&match_key(
                    m.app_id.as_deref(),
                    m.exe_name.as_deref(),
                    m.process_id,
                    m.device_id.as_deref(),
                ))) {
                    changes.push(change(m, others, "others"));
                }
            }
        }
    }
    let mut seen = HashSet::new();
    changes.retain(|c| {
        seen.insert(ordinal_key(&match_key(
            c.app_id.as_deref(),
            c.exe_name.as_deref(),
            c.process_id,
            c.device_id.as_deref(),
        )))
    });
    changes
}

pub fn warnings(spec: &Spec, plan: &[PlanChange]) -> Vec<String> {
    spec.keep
        .iter()
        .filter(|keep| {
            !plan.iter().any(|p| {
                p.rule == "keep"
                    && ordinal_ignore_case_optional(p.query.as_deref(), keep.query.as_deref())
            })
        })
        .map(|keep| {
            format!(
                "App not found: {}",
                keep.query.as_deref().unwrap_or_default()
            )
        })
        .collect()
}

/// Original matching intentionally ignores DeviceId during application.
pub fn apply_plan(collection: &mut Collection, plan: &[PlanChange]) {
    for change in plan {
        for app in collection
            .all_devices
            .iter_mut()
            .flatten()
            .flatten()
            .flat_map(|d| d.apps.iter_mut().flatten().flatten())
        {
            let matched = if !is_blank(change.app_id.as_deref()) {
                ordinal_ignore_case_optional(app.app_id.as_deref(), change.app_id.as_deref())
            } else {
                ordinal_ignore_case_optional(app.exe_name.as_deref(), change.exe_name.as_deref())
                    && app.process_id == change.process_id
            };
            if matched {
                app.volume = change.volume_after;
                app.is_muted = change.is_muted_after;
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResponse {
    pub ok: bool,
    pub apply_apps_only: bool,
    pub capture_scope: String,
    pub changes: Vec<PlanChange>,
    pub apps_matched: usize,
    pub warnings: Vec<String>,
}
pub fn preview(collection: &Collection, spec: &Spec) -> PreviewResponse {
    let changes = build_plan(collection, spec);
    let apps_matched = changes
        .iter()
        .map(|p| {
            p.app_id.clone().unwrap_or_else(|| {
                format!(
                    "{}:{}",
                    p.exe_name.as_deref().unwrap_or_default(),
                    p.process_id
                )
            })
        })
        .collect::<HashSet<_>>()
        .len();
    PreviewResponse {
        ok: true,
        apply_apps_only: spec.apply_apps_only,
        capture_scope: if spec.capture_all_devices {
            "AllDevices"
        } else {
            "CurrentDevice"
        }
        .into(),
        apps_matched,
        warnings: warnings(spec, &changes),
        changes,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResponse {
    pub ok: bool,
    pub apps_changed: usize,
    pub changes: Vec<PlanChange>,
    pub warnings: Vec<String>,
}
pub fn apply(collection: &mut Collection, spec: &Spec) -> ApplyResponse {
    let changes = build_plan(collection, spec);
    apply_plan(collection, &changes);
    ApplyResponse {
        ok: true,
        apps_changed: changes.len(),
        warnings: warnings(spec, &changes),
        changes,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedMatch {
    pub score: i32,
    pub match_type: String,
    pub exe_name: Option<String>,
    pub display_name: Option<String>,
    pub app_id: Option<String>,
    pub device: Option<String>,
    pub device_id: Option<String>,
    pub process_id: i32,
    pub volume: i32,
    pub is_muted: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResolveResponse {
    pub query: String,
    pub matches: Vec<ResolvedMatch>,
}
pub fn resolve(collection: Option<&Collection>, queries: &[String]) -> Vec<ResolveResponse> {
    queries
        .iter()
        .map(|query| ResolveResponse {
            query: query.clone(),
            matches: find_matches(collection, Some(query))
                .into_iter()
                .map(|m| ResolvedMatch {
                    score: m.score,
                    match_type: m.match_type,
                    exe_name: m.exe_name,
                    display_name: m.display_name,
                    app_id: m.app_id,
                    device: m.device_name,
                    device_id: m.device_id,
                    process_id: m.process_id,
                    volume: m.volume,
                    is_muted: m.is_muted,
                })
                .collect(),
        })
        .collect()
}

pub fn preview_command(
    collection: Option<&Collection>,
    args: &[String],
) -> Result<PreviewResponse, String> {
    let collection = collection.ok_or("audio not ready")?;
    Ok(preview(collection, &parse(args)?))
}

pub fn apply_command(
    collection: Option<&mut Collection>,
    args: &[String],
) -> Result<ApplyResponse, String> {
    let collection = collection.ok_or("audio not ready")?;
    Ok(apply(collection, &parse(args)?))
}

pub fn resolve_command(
    collection: Option<&Collection>,
    queries: &[String],
) -> Result<Vec<ResolveResponse>, String> {
    if queries.is_empty() {
        return Err("usage: resolve-apps QUERY [QUERY...]".into());
    }
    let collection = collection.ok_or("audio not ready")?;
    Ok(resolve(Some(collection), queries))
}
