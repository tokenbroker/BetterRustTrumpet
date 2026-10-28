//! CliHandler's preset projections (ListProfiles, SaveProfile) over stored
//! VolumeProfilesJson.
mod support;
use bt_core::{cli_dispatch, cli_presets, models::VolumeProfile, profiles};
use support::Central;

const STORED: &str = r#"[
 {"Id":"a1","Name":"Night Mode","Slug":"night-mode","CreatedAt":"2026-10-01 21:00",
  "CaptureScope":1,"IncludeDeviceVolumes":true,"IncludeAppVolumes":false,"RouteApps":true,
  "DefaultDevices":{"Playback":{"DeviceId":"{spk}","DisplayName":"Speakers"},
                    "Recording":{"DeviceId":"{mic}","DisplayName":null}},
  "Devices":[{"DeviceId":"{spk}","DisplayName":"Speakers","Volume":20,"IncludeVolume":true,
              "Apps":[{"ExeName":"spotify","Volume":10}]}]},
 {"Name":"Calm","Devices":[]}
]"#;

fn presets() -> Vec<VolumeProfile> {
    let mut n = 0;
    profiles::parse_legacy(STORED, &Central, &mut || {
        n += 1;
        format!("id{n}")
    })
}

#[test]
fn list_projects_every_preset_in_order() {
    let list = cli_presets::list(&presets(), Some("A1"), |p| {
        format!("summary of {}", p.name.as_deref().unwrap_or_default())
    });
    assert_eq!(
        cli_dispatch::json(&list),
        concat!(
            r#"[{"name":"Night Mode","slug":"night-mode","devices":1,"apps":1,"createdAt":"2026-10-01 21:00","#,
            r#""summary":"summary of Night Mode","includeDeviceVolumes":true,"includeAppVolumes":false,"routeApps":true,"#,
            r#""defaults":{"output":"Speakers","mic":"{mic}"},"lastApplied":true},"#,
            r#"{"name":"Calm","slug":"calm","devices":0,"apps":0,"createdAt":null,"summary":"summary of Calm","#,
            r#""includeDeviceVolumes":true,"includeAppVolumes":true,"routeApps":true,"defaults":{},"lastApplied":false}]"#
        )
    );
}

#[test]
fn save_answers_the_stored_preset() {
    let mut night = presets().remove(0);
    night.include_device_volumes = false;
    assert_eq!(
        cli_dispatch::json(&cli_presets::saved(&night, "s".into())),
        concat!(
            r#"{"ok":true,"preset":"Night Mode","slug":"night-mode","captureScope":"AllDevices","#,
            r#""applyAppsOnly":true,"includeAppVolumes":false,"routeApps":true,"#,
            r#""defaults":{"output":"Speakers","mic":"{mic}"},"devices":1,"apps":1,"summary":"s"}"#
        )
    );
}
