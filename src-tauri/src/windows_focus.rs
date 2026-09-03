use tauri::{
    plugin::{Builder, TauriPlugin},
    AppHandle, Runtime,
};
use windows_sys::Win32::{
    Foundation::{GetLastError, HWND},
    System::Threading::{AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId},
    UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, BringWindowToTop, FindWindowW, GetForegroundWindow,
        GetWindowThreadProcessId, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
    },
};

const ASFW_ANY: u32 = u32::MAX;

/// Registers the Windows-only foreground permission handoff plugin.
///
/// This must be registered before the single-instance plugin. On a secondary
/// launch, the plugin grants the primary process permission to call
/// SetForegroundWindow before the single-instance plugin sends WM_COPYDATA and
/// exits the secondary process.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::<R>::new("windows-foreground-permission")
        .setup(|app, _api| {
            let _ = grant_primary_instance_foreground_permission(app);
            Ok(())
        })
        .build()
}

/// Grants the primary instance permission to activate its main window.
///
/// The single-instance plugin owns a hidden window whose class and title are
/// derived from the application identifier. A primary process has no such
/// window when this plugin runs; that is the normal first-launch path.
pub fn grant_primary_instance_foreground_permission<R: Runtime>(app: &AppHandle<R>) -> bool {
    let identifier = app.config().identifier.as_str();
    let class_name = encode_wide(&format!("{identifier}-sic"));
    let window_name = encode_wide(&format!("{identifier}-siw"));
    let hwnd = unsafe { FindWindowW(class_name.as_ptr(), window_name.as_ptr()) };

    if hwnd.is_null() {
        log::debug!(
            "windows-focus:foreground-permission-skipped identifier={identifier} reason=target-window-not-found"
        );
        return false;
    }

    let mut primary_pid = 0_u32;
    let target_thread = unsafe { GetWindowThreadProcessId(hwnd, &mut primary_pid) };
    if target_thread == 0 {
        let win32_error = unsafe { GetLastError() };
        log::warn!(
            "windows-focus:foreground-permission-failed identifier={identifier} reason=get-window-thread-process-id-failed win32_error={win32_error}"
        );
        return false;
    }
    if primary_pid == 0 {
        log::warn!(
            "windows-focus:foreground-permission-failed identifier={identifier} reason=target-pid-zero"
        );
        return false;
    }

    let current_pid = unsafe { GetCurrentProcessId() };
    if primary_pid == current_pid {
        log::warn!(
            "windows-focus:foreground-permission-failed identifier={identifier} reason=target-is-current-process pid={current_pid}"
        );
        return false;
    }

    if unsafe { AllowSetForegroundWindow(primary_pid) } == 0 {
        let win32_error = unsafe { GetLastError() };
        log::warn!(
            "windows-focus:foreground-permission-failed identifier={identifier} primary_pid={primary_pid} reason=allow-set-foreground-window-failed win32_error={win32_error}"
        );
        return false;
    }

    log::debug!(
        "windows-focus:foreground-permission-granted identifier={identifier} primary_pid={primary_pid}"
    );
    true
}

pub fn is_notification_activation_source(source: &str) -> bool {
    source.contains("notification")
        || source == "single-instance-native-action"
        || source == "startup-native-action"
}

/// Activates a window and verifies that Windows made it the foreground window.
///
/// Notification activations use only the native synchronous calls. Ordinary
/// tray activations retain the existing thread-attachment fallback so their
/// behavior remains unchanged.
pub fn force_foreground_window(hwnd: HWND, source: &str) -> bool {
    if hwnd.is_null() {
        log::warn!("windows-focus:foreground-failed source={source} reason=null-hwnd");
        return false;
    }

    let notification_source = is_notification_activation_source(source);
    let activated = if notification_source {
        activate_window_native(hwnd, source)
    } else {
        activate_window_with_thread_attachment(hwnd, source)
    };

    log::debug!(
        "windows-focus:foreground source={source} notification_source={} activated={activated}",
        notification_source
    );
    activated
}

fn activate_window_native(hwnd: HWND, source: &str) -> bool {
    let show_command = if unsafe { IsIconic(hwnd) } != 0 {
        SW_RESTORE
    } else {
        SW_SHOW
    };
    unsafe {
        // These calls are intentionally synchronous. The permission handoff
        // happens immediately before WM_COPYDATA delivery.
        let _ = ShowWindow(hwnd, show_command);
    }

    let brought_to_top = unsafe { BringWindowToTop(hwnd) != 0 };
    let set_foreground = unsafe { SetForegroundWindow(hwnd) != 0 };
    let foreground_error = if set_foreground {
        None
    } else {
        Some(unsafe { GetLastError() })
    };
    let activated = unsafe { GetForegroundWindow() } == hwnd;

    if !brought_to_top {
        log::debug!("windows-focus:bring-to-top-failed source={source} reason=win32-call-failed");
    }
    if !set_foreground || !activated {
        log::warn!(
            "windows-focus:foreground-activation-failed source={source} set_foreground={set_foreground} verified={activated} win32_error={foreground_error:?}"
        );
    }
    activated
}

fn activate_window_with_thread_attachment(hwnd: HWND, source: &str) -> bool {
    if unsafe { AllowSetForegroundWindow(ASFW_ANY) } == 0 {
        let win32_error = unsafe { GetLastError() };
        log::debug!(
            "windows-focus:legacy-permission-failed source={source} reason=allow-set-foreground-window-failed win32_error={win32_error}"
        );
    }

    let foreground = unsafe { GetForegroundWindow() };
    let current_thread = unsafe { GetCurrentThreadId() };
    let target_thread = unsafe { GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) };
    let foreground_thread = if foreground.is_null() {
        0
    } else {
        unsafe { GetWindowThreadProcessId(foreground, std::ptr::null_mut()) }
    };
    let attached = current_thread != foreground_thread
        && foreground_thread != 0
        && unsafe { AttachThreadInput(current_thread, foreground_thread, 1) != 0 };
    let target_attached = current_thread != target_thread
        && target_thread != 0
        && unsafe { AttachThreadInput(current_thread, target_thread, 1) != 0 };

    let activated = activate_window_native(hwnd, source);

    if attached {
        unsafe {
            AttachThreadInput(current_thread, foreground_thread, 0);
        }
    }
    if target_attached {
        unsafe {
            AttachThreadInput(current_thread, target_thread, 0);
        }
    }

    activated
}

fn encode_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::is_notification_activation_source;

    #[test]
    fn identifies_notification_activation_sources() {
        for source in [
            "notification-click",
            "notification-click-open-file",
            "notification-click-show-in-folder",
            "single-instance-native-action",
            "startup-native-action",
        ] {
            assert!(is_notification_activation_source(source));
        }
        assert!(!is_notification_activation_source("tray-left-click"));
    }
}
