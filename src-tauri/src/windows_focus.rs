use std::mem;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VK_LMENU,
};
use windows_sys::Win32::{
    Foundation::HWND,
    System::Threading::{AttachThreadInput, GetCurrentThreadId},
    UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId,
        IsIconic, SetForegroundWindow, ShowWindowAsync, SW_RESTORE, SW_SHOW,
    },
};

const ASFW_ANY: u32 = u32::MAX;
const ALT_INPUT_COUNT: u32 = 2;

pub fn force_foreground_window(hwnd: HWND, source: &str) -> bool {
    if hwnd.is_null() {
        log::warn!("windows-focus:foreground-failed source={source} reason=null-hwnd");
        return false;
    }

    let _ = unsafe { AllowSetForegroundWindow(ASFW_ANY) };
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

    let show_command = if unsafe { IsIconic(hwnd) != 0 } {
        SW_RESTORE
    } else {
        SW_SHOW
    };
    let _ = unsafe { ShowWindowAsync(hwnd, show_command) };
    let _ = unsafe { BringWindowToTop(hwnd) };
    let _ = unsafe { SetForegroundWindow(hwnd) };

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

    let mut activated = unsafe { GetForegroundWindow() } == hwnd;
    let mut alt_pulse_sent = false;
    if !activated && is_notification_activation_source(source) {
        alt_pulse_sent = send_alt_key_pulse(source);
        if alt_pulse_sent {
            let _ = unsafe { BringWindowToTop(hwnd) };
            let _ = unsafe { SetForegroundWindow(hwnd) };
            activated = unsafe { GetForegroundWindow() } == hwnd;
        }
    }

    log::debug!(
        "windows-focus:foreground source={source} activated={activated} alt_pulse_sent={alt_pulse_sent}"
    );
    activated
}

fn is_notification_activation_source(source: &str) -> bool {
    source.contains("notification") || source == "single-instance-native-action"
}

// A user-originated notification activation can still be blocked by the
// Windows foreground lock. A single Alt press/release grants the normal
// foreground transfer permission without changing the window's z-order.
fn send_alt_key_pulse(source: &str) -> bool {
    let scan_code =
        match u16::try_from(unsafe { MapVirtualKeyW(u32::from(VK_LMENU), MAPVK_VK_TO_VSC) }) {
            Ok(scan_code) if scan_code != 0 => scan_code,
            _ => {
                log::warn!(
                    "windows-focus:alt-pulse-failed source={source} reason=scan-code-unavailable"
                );
                return false;
            }
        };
    let input_size = match i32::try_from(mem::size_of::<INPUT>()) {
        Ok(input_size) => input_size,
        Err(_) => {
            log::warn!("windows-focus:alt-pulse-failed source={source} reason=input-size-overflow");
            return false;
        }
    };

    let mut inputs: [INPUT; 2] = unsafe { mem::zeroed() };
    inputs[0].r#type = INPUT_KEYBOARD;
    inputs[0].Anonymous = INPUT_0 {
        ki: KEYBDINPUT {
            wVk: VK_LMENU,
            wScan: scan_code,
            dwFlags: KEYEVENTF_EXTENDEDKEY,
            time: 0,
            dwExtraInfo: 0,
        },
    };
    inputs[1].r#type = INPUT_KEYBOARD;
    inputs[1].Anonymous = INPUT_0 {
        ki: KEYBDINPUT {
            wVk: VK_LMENU,
            wScan: scan_code,
            dwFlags: KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP,
            time: 0,
            dwExtraInfo: 0,
        },
    };

    let sent = unsafe { SendInput(ALT_INPUT_COUNT, inputs.as_ptr(), input_size) };
    let success = sent == ALT_INPUT_COUNT;
    if !success {
        log::warn!(
            "windows-focus:alt-pulse-failed source={source} reason=send-input-failed sent={sent} expected={ALT_INPUT_COUNT}"
        );
    }
    success
}
