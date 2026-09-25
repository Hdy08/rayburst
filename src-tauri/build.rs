fn main() {
    println!("cargo:rerun-if-changed=tauri.conf.json");
    let config: serde_json::Value =
        serde_json::from_str(include_str!("tauri.conf.json")).expect("valid Tauri configuration");
    println!(
        "cargo:rustc-env=DESKTOP_APP_ID={}",
        config["identifier"]
            .as_str()
            .expect("application identifier")
    );
    tauri_build::build();
    // Native window tests reuse Tauri's generated Common Controls v6 manifest.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let out_dir = std::env::var("OUT_DIR").expect("Cargo output directory");
        println!("cargo:rustc-link-search=native={out_dir}");
    }
}
