//! Re-applies the current default output as default: exercises the
//! IPolicyConfig path without changing the user's configuration.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = bt_audio::AudioEngine::start()?;
    let before = engine.devices()?;
    let default = before
        .devices
        .iter()
        .find(|d| d.is_default)
        .ok_or("no default output device")?;
    engine.set_default_device(&default.id)?;
    let after = engine.devices()?;
    let still = after.devices.iter().find(|d| d.is_default).map(|d| &d.id);
    println!(
        "re-applied default: {} (unchanged: {})",
        default.name,
        still == Some(&default.id)
    );
    assert!(engine.set_default_device("{not-a-device}").is_err());
    println!("unknown device rejected");
    Ok(())
}
