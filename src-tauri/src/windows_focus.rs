use std::{
    ffi::OsString,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    time::Duration,
};

use windows::{
    core::{IUnknown, Interface, VARIANT},
    Win32::{
        System::Com::{
            CoAllowSetForegroundWindow, CoCreateInstance, CoTaskMemFree, IServiceProvider,
            CLSCTX_LOCAL_SERVER,
        },
        UI::Shell::{
            IFolderView, IPersistFolder2, IShellBrowser, IShellView, IShellWindows, IWebBrowser2,
            SHGetPathFromIDListEx, SID_STopLevelBrowser, ShellWindows, GPFIDL_DEFAULT,
            SVUIA_ACTIVATE_FOCUS,
        },
    },
};
use windows_sys::Win32::{
    Foundation::{GetLastError, HWND},
    System::Threading::GetCurrentThreadId,
    UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, FlashWindowEx, GetForegroundWindow, GetGUIThreadInfo,
        GetWindowThreadProcessId, IsChild, IsIconic, SetForegroundWindow, ShowWindow,
        ShowWindowAsync, FLASHWINFO, FLASHW_STOP, GUITHREADINFO, SW_RESTORE, SW_SHOW,
    },
};

const EXPLORER_FOCUS_ATTEMPTS: usize = 8;
const EXPLORER_FOCUS_RETRY_DELAY: Duration = Duration::from_millis(50);

/// Preserve the COM notification's foreground permission for this process before
/// returning to the shell. This grants no permission to unrelated processes.
pub fn retain_notification_foreground_permission() -> bool {
    let granted = unsafe { AllowSetForegroundWindow(std::process::id()) != 0 };
    let error = if granted {
        0
    } else {
        unsafe { GetLastError() }
    };
    log::info!(
        "windows-focus:notification-permission pid={} granted={granted} error={error}",
        std::process::id()
    );
    granted
}

/// Request foreground ownership without synthetic input or input-queue attachment.
/// Content focus must be checked separately after the target view is activated.
pub fn force_foreground_window(hwnd: HWND, source: &str) -> bool {
    if hwnd.is_null() {
        return false;
    }
    let before = unsafe { GetForegroundWindow() };
    let show = if unsafe { IsIconic(hwnd) != 0 } {
        SW_RESTORE
    } else {
        SW_SHOW
    };
    unsafe {
        if GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) == GetCurrentThreadId() {
            ShowWindow(hwnd, show);
        } else {
            ShowWindowAsync(hwnd, show);
        }
    }
    let accepted = before == hwnd || unsafe { SetForegroundWindow(hwnd) != 0 };
    let after = unsafe { GetForegroundWindow() };
    let foreground = after == hwnd;
    log::info!("windows-focus:foreground source={source} target={hwnd:?} before={before:?} after={after:?} accepted={accepted} foreground={foreground}");
    foreground
}

pub fn foreground_input_focus_belongs_to(hwnd: HWND) -> bool {
    if hwnd.is_null() || unsafe { GetForegroundWindow() } != hwnd {
        return false;
    }
    let mut info: GUITHREADINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
    if unsafe { GetGUIThreadInfo(0, &mut info) } == 0 {
        return false;
    }
    let belongs = |target: HWND| {
        target == hwnd || (!target.is_null() && unsafe { IsChild(hwnd, target) != 0 })
    };
    belongs(info.hwndActive) && belongs(info.hwndFocus) && unsafe { GetForegroundWindow() } == hwnd
}

pub fn confirm_foreground_focus(hwnd: HWND, source: &str) -> bool {
    let focused = foreground_input_focus_belongs_to(hwnd);
    if focused {
        let flash = FLASHWINFO {
            cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
            hwnd,
            dwFlags: FLASHW_STOP,
            uCount: 0,
            dwTimeout: 0,
        };
        unsafe { FlashWindowEx(&flash) };
    }
    log::info!("windows-focus:confirmed source={source} target={hwnd:?} focused={focused}");
    focused
}

/// Called from the file-reveal worker. Reuse one STA and ShellWindows object
/// for all retries. Never fall back to an unrelated folder.
pub fn focus_file_manager_window_for_dir(dir: &Path, source: &str) -> bool {
    let key = normalized_path_key(dir);
    let result = (|| {
        let _com = crate::windows_toast::Apartment::new()?;
        focus_file_manager_on_sta(&key, source)
    })();
    match result {
        Ok(focused) => focused,
        Err(error) => {
            log::warn!("windows-focus:explorer-failed source={source} error={error}");
            false
        }
    }
}

fn focus_file_manager_on_sta(key: &str, source: &str) -> windows::core::Result<bool> {
    let shell_windows: IShellWindows =
        unsafe { CoCreateInstance(&ShellWindows, None::<&IUnknown>, CLSCTX_LOCAL_SERVER)? };
    for attempt in 0..EXPLORER_FOCUS_ATTEMPTS {
        if let Some(browser) = find_file_manager(&shell_windows, key)? {
            let hwnd = unsafe { browser.HWND()? }.0 as HWND;
            let view = matching_active_shell_view(&browser, key)?;
            if view.is_some() && confirm_foreground_focus(hwnd, source) {
                return Ok(true);
            }
            if let Some(view) = view {
                force_foreground_window(hwnd, source);
                let grant = unsafe { CoAllowSetForegroundWindow(&browser, None) };
                log::debug!("windows-focus:explorer-permission source={source} result={grant:?}");
                // Explorer restores its own content focus on the verified tab.
                if let Err(error) = unsafe { view.UIActivate(SVUIA_ACTIVATE_FOCUS.0 as u32) } {
                    log::debug!(
                        "windows-focus:explorer-view-focus-failed source={source} error={error}"
                    );
                }
                if confirm_foreground_focus(hwnd, source) {
                    return Ok(true);
                }
            }
        }
        if attempt + 1 < EXPLORER_FOCUS_ATTEMPTS {
            std::thread::sleep(EXPLORER_FOCUS_RETRY_DELAY);
        }
    }
    Ok(false)
}

fn matching_active_shell_view(
    browser: &IWebBrowser2,
    key: &str,
) -> windows::core::Result<Option<IShellView>> {
    let provider = browser.cast::<IServiceProvider>()?;
    let shell_browser: IShellBrowser = unsafe { provider.QueryService(&SID_STopLevelBrowser)? };
    let view = unsafe { shell_browser.QueryActiveShellView()? };
    let folder_view = view.cast::<IFolderView>()?;
    let folder: IPersistFolder2 = unsafe { folder_view.GetFolder()? };
    let pidl = unsafe { folder.GetCurFolder()? };
    if pidl.is_null() {
        return Ok(None);
    }
    let mut path = vec![0_u16; 32_768];
    let converted = unsafe { SHGetPathFromIDListEx(pidl, &mut path, GPFIDL_DEFAULT) }.as_bool();
    unsafe { CoTaskMemFree(Some(pidl.cast())) };
    let matches = converted
        && path
            .iter()
            .position(|value| *value == 0)
            .is_some_and(|length| {
                normalized_path_key(Path::new(&OsString::from_wide(&path[..length]))) == key
            });
    Ok(matches.then_some(view))
}

fn find_file_manager(
    windows: &IShellWindows,
    key: &str,
) -> windows::core::Result<Option<IWebBrowser2>> {
    let foreground = unsafe { GetForegroundWindow() };
    let mut first_match = None;
    for index in 0..unsafe { windows.Count()? } {
        let browser = unsafe { windows.Item(&VARIANT::from(index)) }
            .ok()
            .and_then(|item| item.cast::<IWebBrowser2>().ok());
        let Some(browser) = browser else { continue };
        let location = unsafe { browser.LocationURL() }
            .ok()
            .and_then(|value| String::try_from(value).ok())
            .and_then(|value| shell_location_url_to_path(&value));
        if location.is_some_and(|path| normalized_path_key(&path) == key) {
            if unsafe { browser.HWND() }.is_ok_and(|hwnd| hwnd.0 as HWND == foreground) {
                return Ok(Some(browser));
            }
            if first_match.is_none() {
                first_match = Some(browser);
            }
        }
    }
    Ok(first_match)
}

fn shell_location_url_to_path(location_url: &str) -> Option<PathBuf> {
    let parsed = url::Url::parse(location_url).ok()?;
    (parsed.scheme() == "file")
        .then(|| parsed.to_file_path().ok())
        .flatten()
}

fn normalized_path_key(path: &Path) -> String {
    let canonical = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let value = canonical.to_string_lossy().replace('/', "\\");
    let value = value
        .strip_prefix(r"\\?\UNC\")
        .map(|path| format!(r"\\{path}"))
        .or_else(|| value.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or(value);
    let value = if value.len() > 3 {
        value.trim_end_matches('\\').to_string()
    } else {
        value
    };
    value.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_file_shell_locations() {
        assert!(shell_location_url_to_path("https://example.com/folder").is_none());
        assert!(shell_location_url_to_path("not a URL").is_none());
        assert_eq!(
            shell_location_url_to_path("file:///C:/Downloads/a%20b"),
            Some(PathBuf::from(r"C:\Downloads\a b"))
        );
    }

    #[test]
    fn no_window_is_not_a_successful_activation() {
        assert!(!force_foreground_window(std::ptr::null_mut(), "test"));
        assert!(!confirm_foreground_focus(std::ptr::null_mut(), "test"));
    }
}
