//! ProcessCommand request/response mapping over a fake provider (no audio,
//! no pipe, no UI).
use bt_core::cli_dispatch::{self, App, Device, Provider};
use serde_json::{Value, json};

#[derive(Default)]
struct Fake {
    devices: Option<Vec<Device>>,
    calls: Vec<String>,
    profiles: Option<Option<String>>,
    default_error: Option<String>,
    processes: usize,
}

impl Provider for Fake {
    fn version(&self) -> Option<String> {
        Some("9.9.9".into())
    }
    fn devices(&mut self) -> Option<Vec<Device>> {
        self.devices.clone()
    }
    fn set_device_volume(&mut self, device: &Device, volume: i32) {
        self.calls.push(format!("device-volume {} {volume}", device.id));
        self.device(&device.id).volume = volume;
    }
    fn set_device_mute(&mut self, device: &Device, muted: bool) {
        self.calls.push(format!("device-mute {} {muted}", device.id));
        self.device(&device.id).is_muted = muted;
    }
    fn set_app_volume(&mut self, device: &Device, app: &App, volume: i32) {
        self.calls
            .push(format!("app-volume {} {} {volume}", device.id, app.key));
        self.app(&device.id, &app.key).volume = volume;
    }
    fn set_app_mute(&mut self, device: &Device, app: &App, muted: bool) {
        self.calls
            .push(format!("app-mute {} {} {muted}", device.id, app.key));
        self.app(&device.id, &app.key).is_muted = muted;
    }
    fn set_default_device(&mut self, device: &Device) -> Result<(), String> {
        self.calls.push(format!("default {}", device.id));
        self.default_error.clone().map_or(Ok(()), Err)
    }
    fn route_app(&mut self, app: &App, device: &Device) -> bool {
        self.calls.push(format!("route {} {}", app.key, device.id));
        true
    }
    fn route_processes(&mut self, name: &str, device: &Device) -> usize {
        self.calls.push(format!("route-processes {name} {}", device.id));
        self.processes
    }
    fn profiles_json(&self) -> Option<Option<String>> {
        self.profiles.clone()
    }
    fn local_now_ticks(&self) -> i64 {
        // 2026-10-28 13:05:09.042 local.
        621_355_968_000_000_000
            + 20_754 * 864_000_000_000
            + ((13 * 3600 + 5 * 60 + 9) * 1000 + 42) * 10_000
    }
}

impl Fake {
    fn device(&mut self, id: &str) -> &mut Device {
        let devices = self.devices.as_mut().unwrap();
        devices.iter_mut().find(|d| d.id == id).unwrap()
    }
    fn app(&mut self, device: &str, key: &str) -> &mut App {
        let device = self.device(device);
        device.apps.iter_mut().find(|a| a.key == key).unwrap()
    }
}

fn app(name: &str, exe: &str, pid: i32, volume: i32) -> App {
    App {
        app_id: format!("C:\\Apps\\{exe}.exe"),
        key: format!("key-{exe}-{pid}"),
        exe_name: exe.into(),
        display_name: name.into(),
        process_id: pid,
        volume,
        is_muted: false,
    }
}

fn fake() -> Fake {
    Fake {
        devices: Some(vec![
            Device {
                id: "{speakers}".into(),
                name: "Speakers (Realtek Audio)".into(),
                volume: 50,
                is_muted: false,
                is_default: true,
                apps: vec![app("Spotify", "Spotify", 10, 80), app("Discord", "Discord", 20, 60)],
            },
            Device {
                id: "{headset}".into(),
                name: "Headset Earphone".into(),
                volume: 30,
                is_muted: true,
                is_default: false,
                apps: vec![app("Spotify", "Spotify", 11, 40)],
            },
        ]),
        profiles: Some(Some("[]".into())),
        ..Fake::default()
    }
}

fn run(provider: &mut Fake, line: &str) -> Value {
    let raw = cli_dispatch::process(provider, line);
    serde_json::from_str(&raw).unwrap_or_else(|_| panic!("not JSON: {raw}"))
}

/// Raw output, to check property order as well as values.
fn raw(provider: &mut Fake, line: &str) -> String {
    cli_dispatch::process(provider, line)
}

#[test]
fn empty_and_ping() {
    let mut p = fake();
    assert_eq!(raw(&mut p, "  "), r#"{"error":"empty command"}"#);
    assert_eq!(raw(&mut p, "\"\""), r#"{"error":"empty command"}"#);
    assert_eq!(
        raw(&mut p, "--PING"),
        r#"{"status":"ok","version":"9.9.9"}"#
    );
    assert_eq!(
        cli_dispatch::ping("ping", None).as_deref(),
        Some(r#"{"status":"ok","version":"unknown"}"#)
    );
    assert_eq!(cli_dispatch::ping("list-devices", None), None);
}

#[test]
fn listing_commands_keep_the_original_shapes() {
    let mut p = fake();
    assert_eq!(
        raw(&mut p, "list-devices"),
        concat!(
            r#"[{"id":"{speakers}","name":"Speakers (Realtek Audio)","volume":50,"isMuted":false,"isDefault":true,"apps":["#,
            r#"{"id":"C:\\Apps\\Spotify.exe","name":"Spotify","exeName":"Spotify","volume":80,"isMuted":false},"#,
            r#"{"id":"C:\\Apps\\Discord.exe","name":"Discord","exeName":"Discord","volume":60,"isMuted":false}]},"#,
            r#"{"id":"{headset}","name":"Headset Earphone","volume":30,"isMuted":true,"isDefault":false,"apps":["#,
            r#"{"id":"C:\\Apps\\Spotify.exe","name":"Spotify","exeName":"Spotify","volume":40,"isMuted":false}]}]"#
        )
    );
    // Deduplicated by exe name across devices.
    assert_eq!(
        run(&mut p, "apps"),
        json!([
            {"exeName":"Spotify","displayName":"Spotify","volume":80,"isMuted":false,"device":"Speakers (Realtek Audio)","processId":10},
            {"exeName":"Discord","displayName":"Discord","volume":60,"isMuted":false,"device":"Speakers (Realtek Audio)","processId":20}
        ])
    );
    assert_eq!(
        run(&mut p, "get-default"),
        json!({"id":"{speakers}","name":"Speakers (Realtek Audio)","volume":50,"isMuted":false})
    );
    assert_eq!(
        run(&mut p, "volume get --device head"),
        json!({"device":"Headset Earphone","volume":30,"isMuted":true})
    );
    assert_eq!(
        raw(&mut p, "watch"),
        concat!(
            r#"{"timestamp":"2026-10-28T13:05:09.042Z","devices":[{"name":"Speakers (Realtek Audio)","volume":50,"isMuted":false,"isDefault":true,"appCount":2,"apps":["#,
            r#"{"exeName":"Spotify","name":"Spotify","volume":80,"isMuted":false,"processId":10},"#,
            r#"{"exeName":"Discord","name":"Discord","volume":60,"isMuted":false,"processId":20}]},"#,
            r#"{"name":"Headset Earphone","volume":30,"isMuted":true,"isDefault":false,"appCount":1,"apps":["#,
            r#"{"exeName":"Spotify","name":"Spotify","volume":40,"isMuted":false,"processId":11}]}]}"#
        )
    );
    assert!(p.calls.is_empty());
}

#[test]
fn volume_and_mute_set_only_changed_values() {
    let mut p = fake();
    assert_eq!(
        raw(&mut p, "set-volume 75"),
        r#"{"ok":true,"device":"Speakers (Realtek Audio)","volume":75}"#
    );
    assert_eq!(
        raw(&mut p, "set-volume +30"),
        r#"{"ok":true,"device":"Speakers (Realtek Audio)","volume":100,"delta":30}"#
    );
    // Unchanged: no setter call.
    assert_eq!(
        run(&mut p, "set-volume 100"),
        json!({"ok":true,"device":"Speakers (Realtek Audio)","volume":100})
    );
    assert_eq!(
        raw(&mut p, "volume discord 67"),
        r#"{"ok":true,"app":"Discord","volume":67}"#
    );
    assert_eq!(
        run(&mut p, "set-volume -50 --app spot"),
        json!({"ok":true,"app":"Spotify","volume":30,"delta":-50})
    );
    assert_eq!(
        raw(&mut p, "mute discord"),
        r#"{"ok":true,"app":"Discord","isMuted":true}"#
    );
    assert_eq!(
        run(&mut p, "toggle-mute --device {headset}"),
        json!({"ok":true,"device":"Headset Earphone","isMuted":false})
    );
    assert_eq!(
        run(&mut p, "unmute"),
        json!({"ok":true,"device":"Speakers (Realtek Audio)","isMuted":false})
    );
    assert_eq!(
        p.calls,
        [
            "device-volume {speakers} 75",
            "device-volume {speakers} 100",
            "app-volume {speakers} key-Discord-20 67",
            "app-volume {speakers} key-Spotify-10 30",
            "app-mute {speakers} key-Discord-20 true",
            "device-mute {headset} false",
        ]
    );
}

#[test]
fn errors_use_the_original_messages() {
    let mut p = fake();
    for (line, message) in [
        (
            "set-volume",
            "usage: set-volume VALUE [--device ID] [--app EXENAME]",
        ),
        (
            "set-volume 101",
            "volume must be 0-100 (or +N/-N for relative)",
        ),
        ("set-volume +x", "invalid relative volume value"),
        ("set-volume 5 --app nothing", "app not found: nothing"),
        ("mute --device nothing", "device not found"),
        ("set-default", "usage: set-default DEVICE_NAME"),
        ("set-default Nothing Here", "device not found: Nothing Here"),
        ("set-device spotify", "usage: set-device APP_EXE DEVICE_NAME"),
        (
            "set-device nothing.exe headset",
            "app not found or no processes running: nothing.exe",
        ),
        ("resolve-apps", "usage: resolve-apps QUERY [QUERY...]"),
        (
            "batch",
            "usage: batch <command...> [<command...>]",
        ),
    ] {
        assert_eq!(run(&mut p, line), json!({ "error": message }), "{line}");
    }
    let mut none = Fake::default();
    for line in ["list-devices", "list-apps", "get-default", "watch", "rule preview --keep a=1"] {
        assert_eq!(run(&mut none, line), json!({"error":"audio not ready"}), "{line}");
    }
    assert_eq!(run(&mut none, "get-volume"), json!({"error":"device not found"}));
    assert_eq!(run(&mut none, "mute spotify"), json!({"error":"app not found: spotify"}));
}

#[test]
fn default_device_and_routing() {
    let mut p = fake();
    assert_eq!(
        raw(&mut p, "device headset"),
        r#"{"ok":true,"name":"Headset Earphone","id":"{headset}"}"#
    );
    p.default_error = Some("Windows did not apply the default device for eConsole: X".into());
    assert_eq!(
        run(&mut p, "set-default speakers"),
        json!({"error":"Windows did not apply the default device for eConsole: X"})
    );
    assert_eq!(
        raw(&mut p, "set-device spotify headset"),
        r#"{"ok":true,"app":"Spotify","device":"Headset Earphone","processesRouted":2}"#
    );
    p.processes = 3;
    assert_eq!(
        run(&mut p, "set-device notepad.exe speakers"),
        json!({"ok":true,"app":"notepad","device":"Speakers (Realtek Audio)","processesRouted":3})
    );
    assert_eq!(
        p.calls,
        [
            "default {headset}",
            "default {speakers}",
            "route key-Spotify-10 {headset}",
            "route key-Spotify-11 {headset}",
            "route-processes notepad {speakers}",
        ]
    );
}

#[test]
fn rules_apply_live_changes_and_keep_pascal_case_plans() {
    let mut p = fake();
    let preview = run(&mut p, "rule preview --keep discord=90 --others 10");
    assert_eq!(preview["ok"], json!(true));
    assert_eq!(preview["appsMatched"], json!(2));
    assert_eq!(preview["changes"][0]["Rule"], json!("keep"));
    assert!(p.calls.is_empty());
    let applied = run(&mut p, "rule apply --keep discord=90 --others mute");
    assert_eq!(applied["appsChanged"], json!(2));
    // Q25: rows match by AppId on every device, so the headset's Spotify
    // takes the speakers row's VolumeAfter (80) as well.
    assert_eq!(
        p.calls,
        [
            "app-mute {speakers} key-Spotify-10 true",
            "app-volume {speakers} key-Discord-20 90",
            "app-volume {headset} key-Spotify-11 80",
            "app-mute {headset} key-Spotify-11 true",
        ]
    );
    let resolved = run(&mut p, "resolve-apps spot");
    assert_eq!(resolved[0]["query"], json!("spot"));
    assert_eq!(resolved[0]["matches"].as_array().unwrap().len(), 2);
}

#[test]
fn batch_keeps_nested_property_order_and_error_rule() {
    let mut p = fake();
    assert_eq!(
        raw(&mut p, "batch --set-volume 67 --app discord get-default --mute --app nothing"),
        concat!(
            r#"{"ok":false,"results":["#,
            r#"{"command":"set-volume 67 --app discord","response":{"ok":true,"app":"Discord","volume":67}},"#,
            r#"{"command":"get-default","response":{"id":"{speakers}","name":"Speakers (Realtek Audio)","volume":50,"isMuted":false}},"#,
            r#"{"command":"mute --app nothing","response":{"error":"app not found: nothing"}}]}"#
        )
    );
}

#[test]
fn doctor_counts_and_unported_commands() {
    let mut p = fake();
    p.profiles = Some(Some(r#"[{"Name":"a"},{"Name":"b"}]"#.into()));
    assert_eq!(
        raw(&mut p, "doctor"),
        r#"{"ok":true,"version":"9.9.9","audioReady":true,"deviceManagerReady":true,"settingsReady":true,"defaultDevice":"Speakers (Realtek Audio)","devices":2,"apps":3,"presets":2}"#
    );
    assert_eq!(
        run(&mut Fake::default(), "doctor"),
        json!({"ok":false,"version":"9.9.9","audioReady":false,"deviceManagerReady":true,"settingsReady":false,"defaultDevice":null,"devices":0,"apps":0,"presets":0})
    );
    for (line, command) in [
        ("presets", "presets"),
        ("profile list", "presets"),
        ("mode Focus", "apply"),
        ("focus", "apply"),
        ("preset next", "preset-next"),
        ("save x", "save"),
        ("update check", "check-update"),
        ("settings export", "export-settings"),
    ] {
        assert_eq!(
            run(&mut p, line),
            json!({ "error": format!("{command} is not available in BetterTrumpet Rust yet") }),
            "{line}"
        );
    }
}

#[test]
fn extension_handles_unported_commands_when_present() {
    struct WithProfiles(Fake);
    impl Provider for WithProfiles {
        fn version(&self) -> Option<String> {
            self.0.version()
        }
        fn devices(&mut self) -> Option<Vec<Device>> {
            self.0.devices()
        }
        fn set_device_volume(&mut self, d: &Device, v: i32) {
            self.0.set_device_volume(d, v)
        }
        fn set_device_mute(&mut self, d: &Device, m: bool) {
            self.0.set_device_mute(d, m)
        }
        fn set_app_volume(&mut self, d: &Device, a: &App, v: i32) {
            self.0.set_app_volume(d, a, v)
        }
        fn set_app_mute(&mut self, d: &Device, a: &App, m: bool) {
            self.0.set_app_mute(d, a, m)
        }
        fn set_default_device(&mut self, d: &Device) -> Result<(), String> {
            self.0.set_default_device(d)
        }
        fn route_app(&mut self, a: &App, d: &Device) -> bool {
            self.0.route_app(a, d)
        }
        fn route_processes(&mut self, n: &str, d: &Device) -> usize {
            self.0.route_processes(n, d)
        }
        fn profiles_json(&self) -> Option<Option<String>> {
            self.0.profiles_json()
        }
        fn local_now_ticks(&self) -> i64 {
            0
        }
        fn extension(&mut self, command: &str, args: &[String]) -> Option<String> {
            Some(format!("{command}:{}", args.join("|")))
        }
    }
    let mut p = WithProfiles(fake());
    assert_eq!(cli_dispatch::process(&mut p, "Night Mode"), "apply:Night|Mode");
    assert_eq!(cli_dispatch::process(&mut p, "preset load a b"), "apply:a|b");
}

#[test]
fn watch_timestamp_is_local_time_with_literal_z() {
    assert_eq!(cli_dispatch::watch_timestamp(0), "0001-01-01T00:00:00.000Z");
}
