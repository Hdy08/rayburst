fn main() {
    tauri_build::build();

    // Native window tests reuse Tauri's generated Common Controls v6 manifest.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let out_dir = std::env::var("OUT_DIR").expect("Cargo output directory");
        println!("cargo:rustc-link-search=native={out_dir}");
    }

    // On macOS, clear quarantine flags from sidecar binaries so they can execute.
    // This runs AFTER tauri_build::build() which copies sidecars into target/.
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        // Clear source binaries
        let _ = Command::new("xattr").args(["-cr", "binaries/"]).status();
        // Clear copied sidecars in target/debug and target/release.
        let _ = Command::new("sh")
            .args([
                "-c",
                "xattr -cr target/debug/motrix-next-engine* target/release/motrix-next-engine* 2>/dev/null || true",
            ])
            .status();
    }
}
