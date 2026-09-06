//! Explicit, interactive test. Never run by ordinary cargo test or CI.
//! Run with `cargo test --lib interactive_notification_focus_probe -- --ignored --nocapture`.

use super::*;
use std::cell::Cell;
use windows::{
    core::HSTRING,
    Data::Xml::Dom::XmlDocument,
    UI::Notifications::{ToastNotification, ToastNotificationManager},
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Input::KeyboardAndMouse::SetFocus,
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetDlgItem,
            GetMessageW, PostMessageW, PostQuitMessage, RegisterClassW, SetTimer, TranslateMessage,
            UnregisterClassW, ES_AUTOHSCROLL, MSG, WM_APP, WM_CLOSE, WM_DESTROY, WM_TIMER,
            WNDCLASSW, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        },
    },
};

thread_local! {
    static RESULTS: Cell<(u32, u32)> = const { Cell::new((0, 0)) };
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP => {
            let focused = if wparam == 1 {
                crate::windows_focus::force_foreground_window(hwnd, "probe-main");
                SetFocus(GetDlgItem(hwnd, 1));
                crate::windows_focus::confirm_foreground_focus(hwnd, "probe-main")
            } else {
                let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
                let opened = tauri::async_runtime::block_on(crate::commands::show_item_in_dir(
                    path.to_string_lossy().into_owned(),
                ))
                .is_ok();
                opened
                    && crate::windows_focus::focus_file_manager_window_for_dir(
                        path.parent().expect("manifest parent"),
                        "probe-folder",
                    )
            };
            eprintln!(
                "NATIVE_FOCUS_PROBE action={} focused={focused}",
                if wparam == 1 { "main" } else { "folder" }
            );
            RESULTS.with(|results| {
                let (main, folder) = results.get();
                results.set((
                    main + u32::from(wparam == 1 && focused),
                    folder + u32::from(wparam == 2 && focused),
                ));
            });
            0
        }
        WM_TIMER | WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[test]
#[ignore = "interactive Windows notification foreground/focus verification"]
fn interactive_notification_focus_probe() -> Result<()> {
    let _com = Apartment::new()?;
    let identity = Identity {
        app_id: "com.motrix.next.focus-probe".into(),
        clsid: GUID::from_u128(0x27332965_781a_42d3_916a_768297c75f84),
    };
    let class = windows_sys::core::w!("MotrixNotificationFocusProbe");
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let wc = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class,
        ..unsafe { std::mem::zeroed() }
    };
    assert_ne!(unsafe { RegisterClassW(&wc) }, 0);
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class,
            windows_sys::core::w!("Motrix notification focus test"),
            WS_OVERLAPPEDWINDOW,
            160,
            160,
            560,
            180,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    assert!(!hwnd.is_null());
    let hwnd_value = hwnd as isize;
    unsafe {
        CreateWindowExW(
            0,
            windows_sys::core::w!("EDIT"),
            windows_sys::core::w!(""),
            WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL as u32,
            20,
            30,
            490,
            32,
            hwnd,
            1usize as _,
            instance,
            std::ptr::null(),
        );
        SetTimer(hwnd, 1, 120_000, None);
    }
    let registration = Registration::new(
        &identity,
        Arc::new(move |action| {
            let action = match action.as_str() {
                "main" => 1,
                "folder" => 2,
                _ => return,
            };
            let granted = crate::windows_focus::retain_notification_foreground_permission();
            eprintln!("NATIVE_FOCUS_PROBE callback permission={granted}");
            unsafe { PostMessageW(hwnd_value as HWND, WM_APP, action, 0) };
        }),
    )?;
    let xml = XmlDocument::new()?;
    xml.LoadXml(&HSTRING::from(r#"<toast launch="main" activationType="foreground" duration="long"><visual><binding template="ToastGeneric"><text>Motrix notification focus test</text><text>Native COM activation</text></binding></visual><actions><action content="Open test window" arguments="main" activationType="foreground"/><action content="Show test folder" arguments="folder" activationType="foreground"/></actions></toast>"#))?;
    let notifier =
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(&identity.app_id))?;
    // Four independent notifications allow both unopened and already-open targets.
    for index in 0..4 {
        let toast = ToastNotification::CreateToastNotification(&xml)?;
        toast.SetTag(&HSTRING::from(index.to_string()))?;
        notifier.Show(&toast)?;
    }
    eprintln!("NATIVE_FOCUS_PROBE ready: click two window and two folder actions; close test window when finished (120s timeout).");
    let mut message: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    drop(registration);
    identity.unregister(&std::env::current_exe().map_err(registry_error)?)?;
    ToastNotificationManager::History()?.ClearWithId(&HSTRING::from(&identity.app_id))?;
    // These two fixed keys belong exclusively to this test, not to Motrix.
    let root = RegKey::predef(HKEY_CURRENT_USER);
    root.delete_subkey(identity.app_key())
        .map_err(registry_error)?;
    root.delete_subkey(format!(
        r"Software\Classes\CLSID\{}",
        identity.clsid_string()
    ))
    .map_err(registry_error)?;
    unsafe { UnregisterClassW(class, instance) };
    let (main, folder) = RESULTS.with(Cell::get);
    assert!(
        main >= 2 && folder >= 2,
        "native verification incomplete: main={main}, folder={folder}"
    );
    Ok(())
}
