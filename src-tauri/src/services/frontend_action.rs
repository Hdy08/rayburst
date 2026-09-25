use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

const PENDING_FRONTEND_ACTION_LIMIT: usize = 32;

/// Frontend actions waiting for a recreated WebView to finish booting.
///
/// Lightweight mode destroys the main WebView while keeping native tray and
/// notification callbacks alive. Any action that depends on a Vue listener must
/// survive the gap between native dispatch and listener registration. Actions
/// that preserve window state recreate the WebView hidden.
#[derive(Debug, Default)]
struct PendingFrontendActions {
    queue: Vec<PendingFrontendAction>,
    frontend_ready: bool,
}

pub struct PendingFrontendActionState(Mutex<PendingFrontendActions>);

impl PendingFrontendActionState {
    pub fn new() -> Self {
        Self(Mutex::new(PendingFrontendActions::default()))
    }

    fn set_frontend_ready(&self, ready: bool) {
        match self.0.lock() {
            Ok(mut inner) => {
                inner.frontend_ready = ready;
            }
            Err(poisoned) => {
                poisoned.into_inner().frontend_ready = ready;
            }
        }
    }

    fn frontend_ready(&self) -> bool {
        self.0
            .lock()
            .map(|inner| inner.frontend_ready)
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FrontendActionChannel {
    #[cfg(target_os = "macos")]
    MenuEvent,
    NotificationAction,
    TrayMenuAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingActionEnqueueResult {
    Queued,
    Duplicate,
    ReplacedOldest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowDispatchMode {
    Activate,
    Preserve,
}

impl FrontendActionChannel {
    fn event_name(self) -> &'static str {
        match self {
            #[cfg(target_os = "macos")]
            Self::MenuEvent => "menu-event",
            Self::NotificationAction => "notification-action",
            Self::TrayMenuAction => "tray-menu-action",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FrontendActionKind {
    #[cfg(target_os = "macos")]
    About,
    NewTask,
    #[cfg(target_os = "macos")]
    OpenTorrent,
    #[cfg(target_os = "macos")]
    Preferences,
    #[cfg(target_os = "macos")]
    ReleaseNotes,
    #[cfg(target_os = "macos")]
    ReportIssue,
    ShowDownloads,
    ShowTaskList,
    OpenTaskFile,
    ShowTaskInFolder,
}

impl FrontendActionKind {
    fn as_str(self) -> &'static str {
        match self {
            #[cfg(target_os = "macos")]
            Self::About => "about",
            Self::NewTask => "new-task",
            #[cfg(target_os = "macos")]
            Self::OpenTorrent => "open-torrent",
            #[cfg(target_os = "macos")]
            Self::Preferences => "preferences",
            #[cfg(target_os = "macos")]
            Self::ReleaseNotes => "release-notes",
            #[cfg(target_os = "macos")]
            Self::ReportIssue => "report-issue",
            Self::ShowDownloads => "show-downloads",
            Self::ShowTaskList => "show-task-list",
            Self::OpenTaskFile => "open-file",
            Self::ShowTaskInFolder => "show-in-folder",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingFrontendAction {
    channel: FrontendActionChannel,
    action: FrontendActionKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<String>,
    #[serde(skip)]
    window_mode: WindowDispatchMode,
}

impl PendingFrontendAction {
    fn new(channel: FrontendActionChannel, action: FrontendActionKind) -> Self {
        Self {
            channel,
            action,
            payload: None,
            window_mode: WindowDispatchMode::Activate,
        }
    }

    fn with_payload(
        channel: FrontendActionChannel,
        action: FrontendActionKind,
        payload: String,
    ) -> Self {
        Self {
            channel,
            action,
            payload: Some(payload),
            window_mode: WindowDispatchMode::Activate,
        }
    }

    fn preserving_window_with_payload(
        channel: FrontendActionChannel,
        action: FrontendActionKind,
        payload: String,
    ) -> Self {
        Self {
            channel,
            action,
            payload: Some(payload),
            window_mode: WindowDispatchMode::Preserve,
        }
    }
}

#[cfg(target_os = "macos")]
pub fn menu_action_from_id(id: &str) -> Option<FrontendActionKind> {
    match id {
        "about" => Some(FrontendActionKind::About),
        "new-task" => Some(FrontendActionKind::NewTask),
        "open-torrent" => Some(FrontendActionKind::OpenTorrent),
        "preferences" => Some(FrontendActionKind::Preferences),
        "release-notes" => Some(FrontendActionKind::ReleaseNotes),
        "report-issue" => Some(FrontendActionKind::ReportIssue),
        _ => None,
    }
}

pub fn take_pending_frontend_actions(
    state: &PendingFrontendActionState,
) -> Vec<PendingFrontendAction> {
    match state.0.lock() {
        Ok(mut inner) => {
            inner.frontend_ready = true;
            std::mem::take(&mut inner.queue)
        }
        Err(poisoned) => {
            let mut inner = poisoned.into_inner();
            inner.frontend_ready = true;
            std::mem::take(&mut inner.queue)
        }
    }
}

pub fn peek_pending_frontend_actions_silent(state: &PendingFrontendActionState) -> bool {
    state
        .0
        .lock()
        .map(|inner| {
            !inner.queue.is_empty()
                && inner
                    .queue
                    .iter()
                    .all(|action| action.window_mode == WindowDispatchMode::Preserve)
        })
        .unwrap_or(false)
}

pub fn mark_frontend_actions_unready(app: &AppHandle) {
    if let Some(state) = app.try_state::<PendingFrontendActionState>() {
        state.set_frontend_ready(false);
    }
}

pub fn dispatch_frontend_action(
    app: &AppHandle,
    channel: FrontendActionChannel,
    action: FrontendActionKind,
    source: &'static str,
) {
    dispatch_frontend_action_with_payload(app, channel, action, None, source);
}

pub fn dispatch_frontend_action_with_payload(
    app: &AppHandle,
    channel: FrontendActionChannel,
    action: FrontendActionKind,
    payload: Option<String>,
    source: &'static str,
) {
    dispatch_frontend_action_with_mode(
        app,
        channel,
        action,
        payload,
        source,
        WindowDispatchMode::Activate,
    );
}

pub fn dispatch_frontend_action_with_payload_preserving_window(
    app: &AppHandle,
    channel: FrontendActionChannel,
    action: FrontendActionKind,
    payload: String,
    source: &'static str,
) {
    dispatch_frontend_action_with_mode(
        app,
        channel,
        action,
        Some(payload),
        source,
        WindowDispatchMode::Preserve,
    );
}

fn dispatch_frontend_action_with_mode(
    app: &AppHandle,
    channel: FrontendActionChannel,
    action: FrontendActionKind,
    payload: Option<String>,
    source: &'static str,
    window_mode: WindowDispatchMode,
) {
    let window_was_alive = app.get_webview_window("main").is_some();
    let frontend_ready = is_frontend_ready(app);

    log::info!(
        "frontend_action:dispatch source={source} channel={} action={} window_mode={window_mode:?} window_alive={window_was_alive} frontend_ready={frontend_ready}",
        channel.event_name(),
        action.as_str()
    );

    if window_was_alive && frontend_ready {
        if window_mode == WindowDispatchMode::Activate {
            wake_main_window(app, source, window_mode);
        }
        let event_payload = payload
            .as_deref()
            .map(|value| serde_json::json!({ "action": action.as_str(), "payload": value }))
            .unwrap_or_else(|| serde_json::Value::String(action.as_str().to_string()));
        match app.emit(channel.event_name(), event_payload) {
            Ok(()) => return,
            Err(e) => {
                log::warn!(
                    "frontend_action:emit-failed source={source} channel={} action={} error={e}",
                    channel.event_name(),
                    action.as_str()
                );
            }
        }
    }

    let pending = match (payload, window_mode) {
        (Some(value), WindowDispatchMode::Preserve) => {
            PendingFrontendAction::preserving_window_with_payload(channel, action, value)
        }
        (Some(value), WindowDispatchMode::Activate) => {
            PendingFrontendAction::with_payload(channel, action, value)
        }
        (None, _) => PendingFrontendAction::new(channel, action),
    };
    if queue_pending_frontend_action(app, pending, source) {
        schedule_main_window_wake(app, source, window_mode);
    }
}

fn queue_pending_frontend_action(
    app: &AppHandle,
    action: PendingFrontendAction,
    source: &'static str,
) -> bool {
    match app.try_state::<PendingFrontendActionState>() {
        Some(state) => match state.0.lock() {
            Ok(mut inner) => {
                let result = enqueue_pending_frontend_action(&mut inner, action);
                log_pending_action_enqueue(source, result);
                !matches!(result, PendingActionEnqueueResult::Duplicate)
            }
            Err(poisoned) => {
                let mut inner = poisoned.into_inner();
                let result = enqueue_pending_frontend_action(&mut inner, action);
                log_pending_action_enqueue_after_poison(source, result, inner.queue.len());
                !matches!(result, PendingActionEnqueueResult::Duplicate)
            }
        },
        None => {
            log::error!("frontend_action:queue-unavailable source={source}");
            false
        }
    }
}

fn enqueue_pending_frontend_action(
    inner: &mut PendingFrontendActions,
    action: PendingFrontendAction,
) -> PendingActionEnqueueResult {
    if inner.queue.contains(&action) {
        return PendingActionEnqueueResult::Duplicate;
    }

    if inner.queue.len() >= PENDING_FRONTEND_ACTION_LIMIT {
        inner.queue.remove(0);
        inner.queue.push(action);
        return PendingActionEnqueueResult::ReplacedOldest;
    }

    inner.queue.push(action);
    PendingActionEnqueueResult::Queued
}

fn log_pending_action_enqueue(source: &'static str, result: PendingActionEnqueueResult) {
    match result {
        PendingActionEnqueueResult::Queued => {
            log::info!("frontend_action:queued source={source}");
        }
        PendingActionEnqueueResult::Duplicate => {
            log::debug!("frontend_action:queue-deduplicated source={source}");
        }
        PendingActionEnqueueResult::ReplacedOldest => {
            log::warn!(
                "frontend_action:queue-capacity-reached source={source} limit={PENDING_FRONTEND_ACTION_LIMIT}"
            );
        }
    }
}

fn log_pending_action_enqueue_after_poison(
    source: &'static str,
    result: PendingActionEnqueueResult,
    pending: usize,
) {
    match result {
        PendingActionEnqueueResult::Queued => {
            log::warn!("frontend_action:queued-after-poison source={source} pending={pending}");
        }
        PendingActionEnqueueResult::Duplicate => {
            log::warn!("frontend_action:queue-deduplicated-after-poison source={source}");
        }
        PendingActionEnqueueResult::ReplacedOldest => {
            log::warn!(
                "frontend_action:queue-capacity-reached-after-poison source={source} limit={PENDING_FRONTEND_ACTION_LIMIT}"
            );
        }
    }
}

fn is_frontend_ready(app: &AppHandle) -> bool {
    app.try_state::<PendingFrontendActionState>()
        .map(|state| state.frontend_ready())
        .unwrap_or(false)
}

fn schedule_main_window_wake(
    app: &AppHandle,
    source: &'static str,
    window_mode: WindowDispatchMode,
) {
    let app_for_task = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let app_for_main = app_for_task.clone();
        if let Err(e) = app_for_task.run_on_main_thread(move || {
            wake_main_window(&app_for_main, source, window_mode);
        }) {
            log::error!("frontend_action:wake-schedule-failed source={source} error={e}");
        }
    });
}

fn wake_main_window(app: &AppHandle, source: &'static str, window_mode: WindowDispatchMode) {
    log::debug!("frontend_action:wake-start source={source} window_mode={window_mode:?}");
    let outcome = match window_mode {
        WindowDispatchMode::Activate => crate::tray::activate_main_window(app, source),
        WindowDispatchMode::Preserve => crate::tray::ensure_main_window(app, source),
    };
    if matches!(
        outcome,
        crate::tray::WindowActivationOutcome::Activated
            | crate::tray::WindowActivationOutcome::WindowReady
    ) {
        log::debug!("frontend_action:wake-done source={source} window_mode={window_mode:?}");
    } else {
        log::debug!("frontend_action:wake-incomplete source={source} outcome={outcome:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::{
        enqueue_pending_frontend_action, peek_pending_frontend_actions_silent,
        take_pending_frontend_actions, FrontendActionChannel, FrontendActionKind,
        PendingActionEnqueueResult, PendingFrontendAction, PendingFrontendActionState,
        PendingFrontendActions, PENDING_FRONTEND_ACTION_LIMIT,
    };

    #[cfg(target_os = "macos")]
    use super::menu_action_from_id;

    #[test]
    fn take_pending_frontend_actions_drains_queue_once() {
        let state = PendingFrontendActionState::new();
        {
            let mut inner = state
                .0
                .lock()
                .expect("pending frontend action state poisoned");
            inner.queue.push(PendingFrontendAction::new(
                FrontendActionChannel::TrayMenuAction,
                FrontendActionKind::NewTask,
            ));
        }

        assert_eq!(
            take_pending_frontend_actions(&state),
            vec![PendingFrontendAction::new(
                FrontendActionChannel::TrayMenuAction,
                FrontendActionKind::NewTask,
            )]
        );
        assert!(take_pending_frontend_actions(&state).is_empty());
    }

    #[test]
    fn take_pending_frontend_actions_marks_frontend_ready() {
        let state = PendingFrontendActionState::new();
        assert!(!state.frontend_ready());

        let _ = take_pending_frontend_actions(&state);

        assert!(state.frontend_ready());
    }

    #[test]
    fn frontend_ready_flag_can_be_cleared_after_webview_destruction() {
        let state = PendingFrontendActionState::new();
        let _ = take_pending_frontend_actions(&state);
        assert!(state.frontend_ready());

        state.set_frontend_ready(false);

        assert!(!state.frontend_ready());
    }

    #[test]
    fn pending_frontend_actions_are_deduplicated_and_bounded() {
        let mut pending = PendingFrontendActions::default();
        let show_task_list = PendingFrontendAction::new(
            FrontendActionChannel::NotificationAction,
            FrontendActionKind::ShowTaskList,
        );

        assert_eq!(
            enqueue_pending_frontend_action(&mut pending, show_task_list.clone()),
            PendingActionEnqueueResult::Queued
        );
        assert_eq!(
            enqueue_pending_frontend_action(&mut pending, show_task_list),
            PendingActionEnqueueResult::Duplicate
        );
        assert_eq!(pending.queue.len(), 1);

        pending.queue = vec![
            PendingFrontendAction::new(
                FrontendActionChannel::TrayMenuAction,
                FrontendActionKind::NewTask,
            );
            PENDING_FRONTEND_ACTION_LIMIT
        ];
        let replacement = PendingFrontendAction::new(
            FrontendActionChannel::NotificationAction,
            FrontendActionKind::ShowTaskList,
        );
        assert_eq!(
            enqueue_pending_frontend_action(&mut pending, replacement.clone()),
            PendingActionEnqueueResult::ReplacedOldest
        );
        assert_eq!(pending.queue.len(), PENDING_FRONTEND_ACTION_LIMIT);
        assert_eq!(pending.queue.last(), Some(&replacement));
    }

    #[test]
    fn only_window_preserving_pending_actions_keep_recreated_window_hidden() {
        let state = PendingFrontendActionState::new();
        {
            let mut inner = state
                .0
                .lock()
                .expect("pending frontend action state poisoned");
            inner
                .queue
                .push(PendingFrontendAction::preserving_window_with_payload(
                    FrontendActionChannel::NotificationAction,
                    FrontendActionKind::OpenTaskFile,
                    "gid-1".to_string(),
                ));
        }

        assert!(peek_pending_frontend_actions_silent(&state));

        {
            let mut inner = state
                .0
                .lock()
                .expect("pending frontend action state poisoned");
            inner.queue.push(PendingFrontendAction::new(
                FrontendActionChannel::TrayMenuAction,
                FrontendActionKind::NewTask,
            ));
        }

        assert!(!peek_pending_frontend_actions_silent(&state));
        let _ = take_pending_frontend_actions(&state);
        assert!(!peek_pending_frontend_actions_silent(&state));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn maps_supported_native_menu_ids() {
        assert_eq!(
            menu_action_from_id("new-task"),
            Some(FrontendActionKind::NewTask)
        );
        assert_eq!(
            menu_action_from_id("open-torrent"),
            Some(FrontendActionKind::OpenTorrent)
        );
        assert_eq!(menu_action_from_id("unknown"), None);
    }
}
