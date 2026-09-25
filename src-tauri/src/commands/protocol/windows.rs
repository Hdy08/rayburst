//! Effective Windows Shell protocol associations.
use crate::error::AppError;
use std::os::windows::ffi::OsStringExt;
use windows_sys::Win32::UI::Shell::{
    AssocQueryStringW, SHChangeNotify, ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, SHCNE_ASSOCCHANGED,
    SHCNF_IDLIST,
};

pub struct Handler {
    pub path: Option<std::path::PathBuf>,
    pub unavailable: bool,
}

fn missing_handler(result: i32) -> Result<Handler, AppError> {
    match result as u32 {
        0x80070483 => Ok(Handler {
            path: None,
            unavailable: false,
        }),
        0x800401f5 | 0x80070002 | 0x80070003 => Ok(Handler {
            path: None,
            unavailable: true,
        }),
        _ => Err(AppError::Protocol(format!(
            "Shell association query failed: 0x{result:08x}"
        ))),
    }
}

pub fn handler(protocol: &str) -> Result<Handler, AppError> {
    let scheme: Vec<u16> = protocol.encode_utf16().chain(Some(0)).collect();
    let mut output = vec![0u16; 32768];
    let mut length = output.len() as u32;
    // SAFETY: Both strings are terminated and the output buffer has length capacity.
    let result = unsafe {
        AssocQueryStringW(
            if protocol.starts_with('.') {
                0
            } else {
                ASSOCF_IS_PROTOCOL
            },
            ASSOCSTR_EXECUTABLE,
            scheme.as_ptr(),
            std::ptr::null(),
            output.as_mut_ptr(),
            &mut length,
        )
    };
    if result == 0 {
        let length = output
            .iter()
            .position(|ch| *ch == 0)
            .unwrap_or(output.len());
        let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(&output[..length]));
        let chooser = std::env::var_os("SystemRoot").is_some_and(|root| {
            ["System32", "SysWOW64"].iter().any(|directory| {
                path.to_string_lossy().eq_ignore_ascii_case(
                    &std::path::Path::new(&root)
                        .join(directory)
                        .join("OpenWith.exe")
                        .to_string_lossy(),
                )
            })
        });
        return Ok(Handler {
            path: (!chooser).then_some(path),
            unavailable: false,
        });
    }
    missing_handler(result)
}

pub fn is_default(protocol: &str) -> Result<bool, AppError> {
    let expected = std::env::current_exe()?;
    Ok(handler(protocol)?.path.is_some_and(|path| {
        dunce::simplified(&path)
            .to_string_lossy()
            .eq_ignore_ascii_case(&dunce::simplified(&expected).to_string_lossy())
    }))
}

/// Open the installed application's page without creating another registration.
pub fn settings_url(app: &tauri::AppHandle) -> Result<String, AppError> {
    use winreg::{
        enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE},
        RegKey,
    };
    let identity = &app.config().identifier;
    let expected = format!(
        "\"{}\" \"%1\"",
        dunce::simplified(&std::env::current_exe()?).display()
    );
    for (hive, parameter) in [
        (HKEY_LOCAL_MACHINE, "registeredAppMachine"),
        (HKEY_CURRENT_USER, "registeredAppUser"),
    ] {
        let root = RegKey::predef(hive);
        let command = root
            .open_subkey(format!(
                "Software\\Classes\\{identity}.rayburst\\shell\\open\\command"
            ))
            .and_then(|key| key.get_value::<String, _>(""));
        if command.is_ok_and(|value| value.eq_ignore_ascii_case(&expected)) {
            return Ok(format!(
                "ms-settings:defaultapps?{parameter}={}",
                urlencoding::encode(identity)
            ));
        }
    }
    Ok("ms-settings:defaultapps".into())
}

/// Notify the Shell after a completed registration, rather than refreshing icons alone.
pub fn notify_changed() {
    // SAFETY: SHCNE_ASSOCCHANGED takes no item pointers.
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST,
            std::ptr::null(),
            std::ptr::null(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::missing_handler;
    #[test]
    fn missing_applications_are_repairable_but_access_errors_are_not_hidden() {
        for result in [0x800401f5u32, 0x80070002, 0x80070003] {
            assert!(
                missing_handler(result as i32)
                    .expect("missing application")
                    .unavailable
            );
        }
        assert!(
            !missing_handler(0x80070483u32 as i32)
                .expect("unassigned")
                .unavailable
        );
        assert!(missing_handler(0x80070005u32 as i32).is_err());
    }
}
