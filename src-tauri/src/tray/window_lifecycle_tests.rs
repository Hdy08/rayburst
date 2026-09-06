//! Native regression probe; no application plugins, notifications or user data.
//! Run explicitly with `cargo test --lib tauri_window_activation_preserves_lifecycle -- --ignored --nocapture`.

use super::*;
use std::time::{Duration, Instant};

// Tauri links its Common Controls v6 manifest only into application binaries.
// The native test executable needs the same resource for Tao's dialog imports.
#[cfg(target_env = "msvc")]
#[link(name = "resource.lib", kind = "dylib", modifiers = "+verbatim")]
extern "C" {}

struct WindowProbe {
    app: tauri::App,
    profile: tempfile::TempDir,
}

impl WindowProbe {
    fn new() -> Self {
        let mut context = tauri::generate_context!();
        context.config_mut().identifier = "com.motrix.next.window-lifecycle-probe".into();
        context.config_mut().app.windows.clear();
        let app = tauri::Builder::default()
            .any_thread()
            .on_window_event(|window, event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    cancel_pending_main_window_activation();
                    window.hide().expect("hide after close request");
                }
            })
            .build(context)
            .expect("build isolated Tauri runtime");
        let mut probe = Self {
            app,
            profile: tempfile::tempdir().expect("temporary WebView profile"),
        };
        probe.settle(30);
        probe.create_window();
        probe
    }

    fn create_window(&mut self) {
        WebviewWindowBuilder::new(
            &self.app,
            "main",
            WebviewUrl::External("about:blank".parse().expect("blank URL")),
        )
        .title("Motrix window lifecycle test")
        .data_directory(self.profile.path().join("webview"))
        .inner_size(560.0, 360.0)
        .transparent(true)
        .decorations(false)
        .visible(false)
        .build()
        .expect("create hidden Tauri WebView");
        self.settle(100);
        assert!(!self.window().is_visible().expect("initial visibility"));
    }

    fn window(&self) -> tauri::WebviewWindow {
        self.app.get_webview_window("main").expect("probe window")
    }

    fn activate(&mut self) {
        // This probe checks lifecycle, not COM foreground permission. A normal
        // test process can be denied foreground ownership by the user's desktop.
        assert!(matches!(
            activate_main_window(self.app.handle(), "lifecycle-probe"),
            WindowActivationOutcome::Activated | WindowActivationOutcome::Pending
        ));
        self.settle(80);
        assert!(self.window().is_visible().expect("activated visibility"));
        assert!(!self.window().is_minimized().expect("activated state"));
    }

    fn retry(&self) {
        let generation = cancel_pending_main_window_activation();
        retry_main_window_activation(self.app.handle(), "lifecycle-probe", generation);
    }

    // Pump actual Tao messages during bounded animation waits, with sleep to
    // avoid the busy loop that motivated run_iteration's deprecation.
    #[allow(deprecated)]
    fn settle(&mut self, millis: u64) {
        let deadline = Instant::now() + Duration::from_millis(millis);
        while Instant::now() < deadline {
            self.app.run_iteration(|_, event| {
                if let tauri::RunEvent::ExitRequested { api, .. } = event {
                    api.prevent_exit();
                }
            });
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for WindowProbe {
    fn drop(&mut self) {
        cancel_pending_main_window_activation();
        for window in self.app.webview_windows().values() {
            let _ = window.destroy();
        }
        self.settle(100);
        self.app.cleanup_before_exit();
    }
}

#[test]
#[ignore = "requires a Windows desktop and WebView2; manipulates isolated native windows"]
fn tauri_window_activation_preserves_lifecycle() {
    let mut probe = WindowProbe::new();
    let window = probe.window();

    // Reproduce the old bug without changing production code: Windows can see
    // the HWND, but Tao still has VISIBLE=false and ignores the hide request.
    crate::windows_focus::force_foreground_window(
        window.hwnd().expect("probe HWND").0 as _,
        "lifecycle-negative-control",
    );
    probe.settle(80);
    assert!(window.is_visible().expect("native show"));
    window.hide().expect("unsynchronized hide");
    probe.settle(80);
    assert!(window.is_visible().expect("reproduce ignored hide"));
    window.maximize().expect("unsynchronized maximize");
    probe.settle(80);
    assert!(!window.is_visible().expect("reproduce unexpected hide"));
    eprintln!("WINDOW_LIFECYCLE_PROBE reproduced stale Tao visibility");

    probe.activate();
    assert!(window.is_maximized().expect("preserve maximized state"));
    for _ in 0..3 {
        window.unmaximize().expect("restore window");
        probe.settle(80);
        assert!(window.is_visible().expect("restore stays visible"));
        assert!(!window.is_maximized().expect("restored state"));
        window.maximize().expect("maximize window");
        probe.settle(80);
        assert!(window.is_visible().expect("maximize stays visible"));
        assert!(window.is_maximized().expect("maximized state"));
        let maximized_size = window.outer_size().expect("maximized size");
        let maximized_position = window.outer_position().expect("maximized position");

        probe.retry();
        window.minimize().expect("minimize window");
        probe.settle(450);
        assert!(window.is_visible().expect("minimize stays on taskbar"));
        assert!(window.is_minimized().expect("retry must not restore"));
        assert_eq!(
            focus_windows_webview(&window, "minimized-probe"),
            WindowActivationOutcome::NotActivated
        );
        probe.activate();
        assert!(window.is_maximized().expect("restore maximized window"));
        assert_eq!(window.outer_size().expect("restored size"), maximized_size);
        assert_eq!(
            window.outer_position().expect("restored position"),
            maximized_position
        );

        probe.retry();
        window.hide().expect("hide window");
        probe.settle(450);
        assert!(!window.is_visible().expect("retry must not show"));
        assert_eq!(
            focus_windows_webview(&window, "hidden-probe"),
            WindowActivationOutcome::NotActivated
        );
        probe.activate();

        probe.retry();
        window.close().expect("request close");
        probe.settle(450);
        assert!(!window.is_visible().expect("close hides window"));
        assert!(probe.app.get_webview_window("main").is_some());
        probe.activate();
    }
    eprintln!("WINDOW_LIFECYCLE_PROBE passed maximize, restore, minimize, hide, close and retry cancellation");

    probe.retry();
    window.destroy().expect("destroy WebView");
    probe.settle(450);
    assert!(probe.app.get_webview_window("main").is_none());
    probe.create_window();
    probe.activate();
    probe.window().close().expect("close recreated window");
    probe.settle(450);
    assert!(!probe.window().is_visible().expect("recreated close hides"));
    eprintln!("WINDOW_LIFECYCLE_PROBE passed destroyed/recreated window lifecycle");
}
