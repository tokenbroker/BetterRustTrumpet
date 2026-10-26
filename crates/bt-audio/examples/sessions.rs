//! Prints active output devices and their app sessions every second.
//! Run with `cargo run -p bt-audio --example sessions`.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = bt_audio::AudioEngine::start()?;
    loop {
        let snapshot = engine.snapshot()?;
        println!("----");
        for device in &snapshot.devices {
            println!(
                "{}{} vol={:.0}% muted={} peak={:.2}",
                if device.is_default { "* " } else { "  " },
                device.name,
                device.volume * 100.0,
                device.muted,
                device.peak
            );
            for app in &device.apps {
                println!(
                    "      {:<28} vol={:>3.0}% muted={:<5} peak={:.2} pids={:?}",
                    app.display_name,
                    app.volume * 100.0,
                    app.muted,
                    app.peak,
                    app.process_ids
                );
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
