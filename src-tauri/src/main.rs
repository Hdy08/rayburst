#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    {
        let mut args = std::env::args_os().skip(1);
        if args.next().as_deref() == Some(std::ffi::OsStr::new("--prepare-install")) {
            let result = match (args.next(), args.next()) {
                (Some(directory), None) => {
                    rayburst_lib::prepare_install(std::path::Path::new(&directory))
                }
                _ => Err("Expected one installation directory".into()),
            };
            if let Err(error) = result {
                eprintln!("{error}");
                std::process::exit(1);
            }
            return;
        }
    }
    rayburst_lib::run()
}
