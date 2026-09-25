#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::io::{stdin, stdout};
use std::process::ExitCode;

use rayburst_browser_launcher::{run_session, write_error_response};

fn activate() -> std::io::Result<()> {
    let launcher = std::env::current_exe()?;
    let directory = launcher.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Launcher has no parent directory",
        )
    })?;
    #[cfg(windows)]
    {
        activate_windows(&directory.join("rayburst.exe"))
    }
    #[cfg(target_os = "macos")]
    {
        // LaunchServices opens this exact bundle, not whichever app owns a URL scheme.
        let bundle = directory
            .parent()
            .and_then(std::path::Path::parent)
            .filter(|path| path.extension().is_some_and(|ext| ext == "app"))
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Launcher is outside an application bundle",
                )
            })?;
        let status = std::process::Command::new("/usr/bin/open")
            .arg("-a")
            .arg(bundle)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "LaunchServices exited with {status}"
            )))
        }
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        // AppImage registration places a persistent symlink beside the copied host.
        let application = directory.join("rayburst").canonicalize()?;
        std::process::Command::new(application)
            .env_remove("EGL_PLATFORM")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn()?;
        Ok(())
    }
}

#[cfg(windows)]
fn activate_windows(application: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        System::Com::{
            CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
        },
        UI::{
            Shell::{ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW},
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    };
    if !application.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "The paired Rayburst executable is missing",
        ));
    }
    let file: Vec<u16> = application
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: This one-shot host owns the thread; all Shell strings outlive the call.
    unsafe {
        let result = CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        );
        if result < 0 {
            return Err(std::io::Error::other(format!(
                "COM initialization failed: 0x{result:08x}"
            )));
        }
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC;
        info.lpFile = file.as_ptr();
        info.nShow = SW_SHOWNORMAL;
        let result = if ShellExecuteExW(&mut info) == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        };
        CoUninitialize();
        result
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input = stdin().lock();
    let mut output = stdout().lock();
    match run_session(&args, &mut input, &mut output, || {
        activate().map_err(|error| {
            eprintln!("Desktop activation failed: {error}");
            error.to_string()
        })
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = write_error_response(&mut output, &error);
            eprintln!("Native messaging request failed: {}", error.code());
            ExitCode::FAILURE
        }
    }
}
