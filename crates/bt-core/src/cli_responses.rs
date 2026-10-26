//! CLI wire DTOs from CliHandler.cs. Live commands populate these types; no
//! serialization key here is inferred from the older CLI help comments.
use crate::{cli, cli_rules::PlanChange};
use serde::{Deserialize, Serialize};
use serde_json::Value;

macro_rules! response {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name { $(pub $field: $ty),* }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[error("{error}")]
pub struct ErrorResponse {
    pub error: String,
}
impl From<String> for ErrorResponse {
    fn from(error: String) -> Self {
        Self { error }
    }
}
impl From<&str> for ErrorResponse {
    fn from(error: &str) -> Self {
        Self {
            error: error.into(),
        }
    }
}
response!(PingResponse {
    status: String,
    version: String
});
response!(DeviceApp {id:Option<String>,name:Option<String>,exe_name:Option<String>,volume:i32,is_muted:bool});
response!(DeviceResponse {id:Option<String>,name:Option<String>,volume:i32,is_muted:bool,is_default:bool,apps:Vec<DeviceApp>});
response!(AppResponse {exe_name:Option<String>,display_name:Option<String>,volume:i32,is_muted:bool,device:Option<String>,process_id:i32});
response!(GetVolumeResponse {device:Option<String>,volume:i32,is_muted:bool});
response!(DefaultResponse {id:Option<String>,name:Option<String>,volume:i32,is_muted:bool});
response!(SetDefaultResponse {ok:bool,name:Option<String>,id:Option<String>});
response!(RouteResponse {ok:bool,app:Option<String>,device:Option<String>,processes_routed:i32});

/// Exactly one target key is present. Null names remain JSON null.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Target {
    Device(Option<String>),
    App(Option<String>),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetVolumeResponse {
    pub ok: bool,
    #[serde(flatten)]
    pub target: Target,
    pub volume: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<i32>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MuteResponse {
    pub ok: bool,
    #[serde(flatten)]
    pub target: Target,
    pub is_muted: bool,
}

response!(WatchApp {exe_name:Option<String>,name:Option<String>,volume:i32,is_muted:bool,process_id:i32});
response!(WatchDevice {name:Option<String>,volume:i32,is_muted:bool,is_default:bool,app_count:usize,apps:Vec<WatchApp>});
response!(WatchResponse {timestamp:String,devices:Vec<WatchDevice>});
response!(DoctorResponse {ok:bool,version:String,audio_ready:bool,device_manager_ready:bool,settings_ready:bool,default_device:Option<String>,devices:usize,apps:usize,presets:usize});
response!(UpdateResponse {
    update_available: bool,
    current_version: String,
    latest_version: String,
    release_url: String,
    last_checked: String
});
/// DescribeDefaults visits AllRoles in this order and omits unset roles.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultNames {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_calls: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mic_calls: Option<String>,
}
response!(ProfileResponse {name:Option<String>,slug:String,devices:usize,apps:usize,created_at:Option<String>,summary:String,include_device_volumes:bool,include_app_volumes:bool,route_apps:bool,defaults:DefaultNames,last_applied:bool});
response!(AppliedDefaultResponse {role:String,name:Option<String>,id:Option<String>});
response!(ApplyProfileResponse {ok:bool,preset:Option<String>,slug:String,defaults_applied:Vec<AppliedDefaultResponse>,devices_applied:i32,devices_missing:Vec<String>,apps_applied:i32,apps_missing:i32,apps_routed:i32,summary:String,warnings:Vec<String>});
response!(SaveResponse {ok:bool,preset:Option<String>,slug:Option<String>,capture_scope:String,apply_apps_only:bool,include_app_volumes:bool,route_apps:bool,defaults:DefaultNames,devices:usize,apps:usize,summary:String});
// PlanChange preserves PascalCase nested keys.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePresetResponse {
    pub ok: bool,
    pub preset: Option<String>,
    pub slug: Option<String>,
    pub capture_scope: String,
    pub apply_apps_only: bool,
    pub apps_changed: usize,
    pub apps_captured: usize,
    pub changes: Vec<PlanChange>,
    pub warnings: Vec<String>,
}
response!(DeleteResponse {ok:bool,preset:Option<String>});
response!(SettingsPathResponse {
    ok: bool,
    path: String
});
response!(BatchResult {
    command: String,
    response: Value
});
response!(BatchResponse {ok:bool,results:Vec<BatchResult>});

pub fn ping(version: Option<&str>) -> PingResponse {
    PingResponse {
        status: "ok".into(),
        version: version.unwrap_or("unknown").into(),
    }
}

/// Split/quote and execute sequentially. The callback is the installed command
/// dispatcher, keeping native audio and recursive dispatch out of the model.
pub fn batch(
    args: &[String],
    mut execute: impl FnMut(&str) -> String,
) -> Result<BatchResponse, ErrorResponse> {
    if args.is_empty() {
        return Err("usage: batch <command...> [<command...>]".into());
    }
    let commands = cli::split_batch(args);
    if commands.is_empty() {
        return Err("usage: batch <command...> [<command...>]".into());
    }
    let mut results = Vec::with_capacity(commands.len());
    let mut ok = true;
    for command in commands {
        let command = command
            .iter()
            .map(|s| cli::quote_if_needed(s))
            .collect::<Vec<_>>()
            .join(" ");
        let raw = execute(&command);
        // Actual ProcessCommand responses are strict JSON or the caught raw
        // fallback. Broader Newtonsoft JToken input syntax remains a decoder gate.
        let response = serde_json::from_str::<Value>(&raw).unwrap_or(Value::String(raw));
        if response
            .as_object()
            .is_some_and(|object| object.contains_key("error"))
        {
            ok = false;
        }
        results.push(BatchResult { command, response });
    }
    Ok(BatchResponse { ok, results })
}

/// Raw counts mirror Doctor: null device slots count as devices, and null app
/// slots count as apps. Use source sequence counts, not filtered list DTO counts.
pub struct HealthAudio<'a> {
    pub default_device: Option<&'a str>,
    pub device_app_counts: Option<&'a [Option<usize>]>,
}
pub struct HealthSettings<'a> {
    pub profiles_json: Option<&'a str>,
}
pub fn doctor(
    version: Option<&str>,
    audio: Option<HealthAudio<'_>>,
    manager_ready: bool,
    settings: Option<HealthSettings<'_>>,
) -> DoctorResponse {
    let presets = settings
        .as_ref()
        .and_then(|s| s.profiles_json)
        .and_then(|json| {
            serde_json::from_str::<Option<Vec<Option<crate::models::VolumeProfile>>>>(json).ok()
        })
        .flatten()
        .map_or(0, |p| p.len());
    DoctorResponse {
        ok: audio.is_some() && manager_ready,
        version: version.unwrap_or("unknown").into(),
        audio_ready: audio.is_some(),
        device_manager_ready: manager_ready,
        settings_ready: settings.is_some(),
        default_device: audio
            .as_ref()
            .and_then(|a| a.default_device)
            .map(str::to_owned),
        devices: audio
            .as_ref()
            .and_then(|a| a.device_app_counts)
            .map_or(0, |d| d.len()),
        apps: audio
            .as_ref()
            .and_then(|a| a.device_app_counts)
            .map_or(0, |d| d.iter().flatten().sum()),
        presets,
    }
}
