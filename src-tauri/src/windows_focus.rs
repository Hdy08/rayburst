use windows_sys::Win32::{
    Foundation::HWND,
    System::Threading::{AttachThreadInput, GetCurrentThreadId},
    UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId,
        IsIconic, SetForegroundWindow, SetWindowPos, ShowWindowAsync, SwitchToThisWindow,
        HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE, SW_SHOW,
    },
};

const ASFW_ANY: u32 = u32::MAX;

pub fn allow_set_foreground_window_any(source: &str) -> bool {
    let ok = unsafe { AllowSetForegroundWindow(ASFW_ANY) != 0 };
    log::debug!("windows-focus:allow-set-foreground source={source} ok={ok}");
    ok
}

/// Activates a window using the synchronous Win32 path used by the working
/// notification implementation. The caller must be handling a user-originated
/// activation while the foreground permission is still available.
pub fn force_foreground_window(hwnd: HWND, source: &str) -> bool {
    if hwnd.is_null() {
        log::warn!("windows-focus:foreground-failed source={source} reason=null-hwnd");
        return false;
    }

    allow_set_foreground_window_any(source);

    let foreground_before = unsafe { GetForegroundWindow() };
    let current_thread = unsafe { GetCurrentThreadId() };
    let target_thread = unsafe { GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) };
    let foreground_thread = if foreground_before.is_null() {
        0
    } else {
        unsafe { GetWindowThreadProcessId(foreground_before, std::ptr::null_mut()) }
    };

    let attached_foreground = attach_thread_input(current_thread, foreground_thread);
    let attached_target = attach_thread_input(current_thread, target_thread);

    let show_command = if unsafe { IsIconic(hwnd) != 0 } {
        SW_RESTORE
    } else {
        SW_SHOW
    };
    let _ = unsafe { ShowWindowAsync(hwnd, show_command) };
    let _ = unsafe { BringWindowToTop(hwnd) };
    let _ = unsafe { SetForegroundWindow(hwnd) };

    // SetForegroundWindow can still be rejected by the foreground lock. These
    // synchronous fallbacks mirror the proven a009 path without synthesizing
    // keyboard input in the user's active application.
    if unsafe { GetForegroundWindow() } != hwnd {
        unsafe {
            SwitchToThisWindow(hwnd, 1);
        }
        if unsafe { GetForegroundWindow() } != hwnd {
            let _ = pulse_topmost(hwnd);
            let _ = unsafe { SetForegroundWindow(hwnd) };
        }
    }

    detach_thread_input(current_thread, target_thread, attached_target);
    detach_thread_input(current_thread, foreground_thread, attached_foreground);

    let activated = unsafe { GetForegroundWindow() } == hwnd;
    log::info!(
        "windows-focus:foreground source={source} activated={activated} target_thread={target_thread} foreground_thread={foreground_thread}"
    );
    activated
}

fn attach_thread_input(current_thread: u32, other_thread: u32) -> bool {
    if current_thread == 0 || other_thread == 0 || current_thread == other_thread {
        return false;
    }
    unsafe { AttachThreadInput(current_thread, other_thread, 1) != 0 }
}

fn detach_thread_input(current_thread: u32, other_thread: u32, attached: bool) {
    if attached {
        unsafe {
            AttachThreadInput(current_thread, other_thread, 0);
        }
    }
}

fn pulse_topmost(hwnd: HWND) -> bool {
    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW;
    let topmost_ok = unsafe { SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, flags) != 0 };
    let notopmost_ok = unsafe { SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, flags) != 0 };
    topmost_ok && notopmost_ok
}
