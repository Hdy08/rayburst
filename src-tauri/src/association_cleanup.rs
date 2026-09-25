//! Installer-only removal of obsolete or redundant application registrations.
use std::{io, path::Path};
use winreg::{enums::*, RegKey};

fn command_path(command: &str) -> Option<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{Foundation::LocalFree, UI::Shell::CommandLineToArgvW};
    let command: Vec<u16> = command.encode_utf16().chain(Some(0)).collect();
    let mut count = 0;
    // SAFETY: The input is terminated; Windows allocates the returned array.
    unsafe {
        let arguments = CommandLineToArgvW(command.as_ptr(), &mut count);
        if arguments.is_null() {
            return None;
        }
        let path = if count > 0 {
            let first = *arguments;
            let mut length = 0;
            while *first.add(length) != 0 {
                length += 1;
            }
            Some(std::ffi::OsString::from_wide(std::slice::from_raw_parts(first, length)).into())
        } else {
            None
        };
        LocalFree(arguments.cast());
        path
    }
}

fn read(root: &RegKey, path: &str, name: &str) -> Option<String> {
    root.open_subkey(path)
        .and_then(|key| key.get_value(name))
        .ok()
}

fn removable_icon(icon: &str, application: &Path) -> bool {
    let path = Path::new(icon.strip_suffix(",0").unwrap_or(icon).trim_matches('"'));
    if !path.is_absolute() {
        return false;
    }
    if path
        .to_string_lossy()
        .eq_ignore_ascii_case(&application.to_string_lossy())
    {
        return true;
    }
    let known = path.file_name().is_some_and(|name| {
        ["rayburst.exe", "motrix-next.exe"]
            .iter()
            .any(|expected| name.to_string_lossy().eq_ignore_ascii_case(expected))
    });
    known && matches!(path.try_exists(), Ok(false))
}

fn remove(root: &RegKey, path: &str) -> io::Result<()> {
    match root.delete_subkey_all(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

pub(crate) fn cleanup(directory: &Path) -> io::Result<()> {
    let application = directory.join("rayburst.exe");
    let identity = crate::APP_ID;
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let root = RegKey::predef(hive);
        // A per-user installer cannot clean another installation's machine keys.
        if root
            .open_subkey_with_flags("Software", KEY_READ | KEY_WRITE)
            .is_err()
        {
            continue;
        }
        for old in ["Rayburst", "MotrixNext"] {
            let capabilities = format!("Software\\{old}\\Capabilities");
            if !read(&root, &capabilities, "ApplicationIcon")
                .is_some_and(|icon| removable_icon(&icon, &application))
            {
                continue;
            }
            for scheme in ["rayburst", "magnet", "ed2k", "thunder"] {
                let key = format!("Software\\Classes\\{old}.Url.{scheme}");
                if read(&root, &format!("{key}\\shell\\open\\command"), "")
                    .and_then(|command| command_path(&command))
                    .is_some_and(|path| removable_icon(&path.to_string_lossy(), &application))
                {
                    remove(&root, &key)?;
                }
            }
            // A surviving handler may belong to another installed copy.
            if ["rayburst", "magnet", "ed2k", "thunder"]
                .iter()
                .any(|scheme| {
                    !matches!(
                        root.open_subkey(format!("Software\\Classes\\{old}.Url.{scheme}")),
                        Err(error) if error.kind() == io::ErrorKind::NotFound
                    )
                })
            {
                continue;
            }
            remove(&root, &capabilities)?;
            if let Ok(key) =
                root.open_subkey_with_flags("Software\\RegisteredApplications", KEY_SET_VALUE)
            {
                let _ = key.delete_value(old);
            }
        }
        // Tauri previously used a generic file class rather than our ProgID.
        let command = format!("{} \"%1\"", application.display());
        if read(
            &root,
            "Software\\Classes\\BitTorrent\\shell\\open\\command",
            "",
        )
        .is_some_and(|value| value.eq_ignore_ascii_case(&command))
        {
            remove(&root, "Software\\Classes\\BitTorrent")?;
            if read(&root, "Software\\Classes\\.torrent", "").as_deref() == Some("BitTorrent") {
                root.open_subkey_with_flags("Software\\Classes\\.torrent", KEY_SET_VALUE)?
                    .delete_value("")?;
            }
        }
    }
    let user = RegKey::predef(HKEY_CURRENT_USER);
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let capabilities = format!("Software\\{identity}\\Capabilities");
    let expected = format!("\"{}\",0", application.display());
    if [&user, &machine].iter().all(|root| {
        read(root, &capabilities, "ApplicationIcon")
            .is_some_and(|icon| icon.eq_ignore_ascii_case(&expected))
    }) {
        remove(&user, &capabilities)?;
        if let Ok(key) =
            user.open_subkey_with_flags("Software\\RegisteredApplications", KEY_SET_VALUE)
        {
            let _ = key.delete_value(identity);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_preserves_other_live_installations_and_unknown_applications() {
        let root = tempfile::tempdir().expect("temporary directory");
        let current = root.path().join("current").join("rayburst.exe");
        let other = root.path().join("rayburst.exe");
        std::fs::write(&other, b"fixture").expect("live executable");
        assert!(removable_icon(
            &format!("\"{}\",0", current.display()),
            &current
        ));
        assert!(!removable_icon(&format!("{},0", other.display()), &current));
        assert!(!removable_icon("unknown.exe,0", &current));
        assert_eq!(
            command_path(&format!("\"{}\" \"%1\"", current.display())),
            Some(current.clone())
        );
        assert!(removable_icon(
            &format!("{},0", root.path().join("motrix-next.exe").display()),
            &current
        ));
    }
}
