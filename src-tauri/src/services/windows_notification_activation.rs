use std::{
    cell::RefCell,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

use tauri::{AppHandle, Manager};

use crate::windows_toast::{Identity, Registration, LAUNCH_ARG};

thread_local! {
    static REGISTRATION: RefCell<Option<Registration>> = const { RefCell::new(None) };
}

pub struct WindowsNotificationActivationState {
    pub registered: AtomicBool,
    startup_silent: AtomicBool,
    main_window_requested: AtomicBool,
    queued: AtomicUsize,
}

pub fn is_notification_launch(args: &[String]) -> bool {
    args.iter().any(|arg| arg == LAUNCH_ARG)
}

pub fn setup(app: &AppHandle) {
    let args = std::env::args().collect::<Vec<_>>();
    app.manage(WindowsNotificationActivationState {
        registered: AtomicBool::new(false),
        startup_silent: AtomicBool::new(is_notification_launch(&args)),
        main_window_requested: AtomicBool::new(false),
        queued: AtomicUsize::new(0),
    });
    let handle = app.clone();
    let identity = Identity::for_app();
    match Registration::new(
        &identity,
        Arc::new(move |arguments| {
            let state = handle.state::<WindowsNotificationActivationState>();
            if state
                .queued
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                    (count < 32).then_some(count + 1)
                })
                .is_err()
            {
                log::warn!("notification:com-queue-full");
                return;
            }
            crate::windows_focus::retain_notification_foreground_permission();
            let app_for_worker = handle.clone();
            // Queue UI work rather than showing or recreating windows inside
            // the notification COM callback.
            tauri::async_runtime::spawn(async move {
                let app_for_main = app_for_worker.clone();
                if let Err(error) = app_for_worker.run_on_main_thread(move || {
                    app_for_main
                        .state::<WindowsNotificationActivationState>()
                        .queued
                        .fetch_sub(1, Ordering::AcqRel);
                    if !super::deep_link::handle_native_action_args(
                        &app_for_main,
                        &[arguments],
                        "toast-com-activation",
                    ) {
                        log::warn!("notification:com-action-rejected");
                    }
                }) {
                    app_for_worker
                        .state::<WindowsNotificationActivationState>()
                        .queued
                        .fetch_sub(1, Ordering::AcqRel);
                    log::warn!("notification:com-dispatch-failed error={error}");
                }
            });
        }),
    ) {
        Ok(registration) => {
            REGISTRATION.with(|slot| *slot.borrow_mut() = Some(registration));
            app.state::<WindowsNotificationActivationState>()
                .registered
                .store(true, Ordering::Release);
            log::info!("notification:com-registered app_id={}", identity.app_id);
        }
        Err(error) => log::warn!("notification:com-registration-failed error={error}"),
    }
}

pub fn is_registered(app: &AppHandle) -> bool {
    app.try_state::<WindowsNotificationActivationState>()
        .is_some_and(|state| state.registered.load(Ordering::Acquire))
}

pub fn startup_silent(app: &AppHandle) -> bool {
    app.try_state::<WindowsNotificationActivationState>()
        .is_some_and(|state| state.startup_silent.load(Ordering::Acquire))
}

pub fn acknowledge_main_window_activation(app: &AppHandle) {
    if let Some(state) = app.try_state::<WindowsNotificationActivationState>() {
        state.startup_silent.store(false, Ordering::Release);
        state.main_window_requested.store(true, Ordering::Release);
    }
}

pub fn main_window_requested(app: &AppHandle) -> bool {
    app.try_state::<WindowsNotificationActivationState>()
        .is_some_and(|state| state.main_window_requested.load(Ordering::Acquire))
}

pub fn clear_main_window_request(app: &AppHandle) {
    if let Some(state) = app.try_state::<WindowsNotificationActivationState>() {
        state.main_window_requested.store(false, Ordering::Release);
    }
}

pub fn shutdown() {
    REGISTRATION.with(|slot| slot.borrow_mut().take());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_exact_com_launch_argument_keeps_startup_hidden() {
        assert!(is_notification_launch(&[
            "motrix-next.exe".into(),
            LAUNCH_ARG.into()
        ]));
        assert!(!is_notification_launch(&[
            "--notification-activation=anything".into()
        ]));
        assert!(!is_notification_launch(&["rayburst://activate".into()]));
    }
}
