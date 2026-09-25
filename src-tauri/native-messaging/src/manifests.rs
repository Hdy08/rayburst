use rayburst_browser_launcher::{chromium_manifest_json, firefox_manifest_json};
use std::path::Path;

fn write_if_changed(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    match std::fs::read(path) {
        Ok(existing) if existing == contents => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::write(path, contents)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args_os()
        .nth(1)
        .ok_or("manifest output directory is required")?;
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory)?;
    let launcher = Path::new(r"..\..\rayburst-browser-launcher.exe");
    write_if_changed(
        &directory.join("chromium.json"),
        &chromium_manifest_json(launcher)?,
    )?;
    write_if_changed(
        &directory.join("firefox.json"),
        &firefox_manifest_json(launcher)?,
    )?;
    Ok(())
}
