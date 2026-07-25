//! Native task notifications for background lifecycle events.

use super::config::RuntimeConfig;
use super::monitor::{events, TaskEvent};
use super::notification_i18n::{
    format_batch_task_message, format_error_message, format_task_message, texts_for_locale,
};
use crate::error::AppError;
use tauri::Manager;

#[cfg(target_os = "linux")]
use std::{
    collections::VecDeque,
    sync::Mutex,
    time::{Duration, Instant},
};

#[cfg(not(target_os = "linux"))]
use tauri_plugin_notification::NotificationExt;

#[cfg(any(target_os = "windows", test))]
use hmac::{Hmac, Mac};
#[cfg(any(target_os = "windows", test))]
use sha2::Sha256;

#[cfg(target_os = "windows")]
use windows::{
    core::HSTRING,
    Data::Xml::Dom::XmlDocument,
    UI::Notifications::{ToastNotification, ToastNotificationManager},
};

#[cfg(target_os = "windows")]
use tauri_plugin_store::StoreExt;

#[cfg(target_os = "linux")]
const LINUX_NOTIFICATION_RETENTION_TTL: Duration = Duration::from_secs(120);
#[cfg(target_os = "linux")]
const LINUX_NOTIFICATION_RETENTION_LIMIT: usize = 32;

#[cfg(any(target_os = "windows", test))]
const WINDOWS_NOTIFICATION_OPEN_FOLDER_ACTION: &str = "open-folder";
#[cfg(any(target_os = "windows", test))]
const WINDOWS_NOTIFICATION_SHOW_TASK_LIST_ACTION: &str = "show-task-list";
#[cfg(target_os = "windows")]
const WINDOWS_START_NOTIFICATION_GROUP: &str = "download-start";
#[cfg(target_os = "windows")]
const WINDOWS_START_NOTIFICATION_LIMIT: usize = 64;
#[cfg(any(target_os = "windows", test))]
pub(crate) const WINDOWS_NOTIFICATION_DIR_MAX_CHARS: usize = 8_192;
#[cfg(target_os = "windows")]
const WINDOWS_DEBUG_NOTIFICATION_APP_ID: &str =
    "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxNotificationIdentity {
    pub app_name: &'static str,
    pub icon: &'static str,
    pub desktop_entry: &'static str,
    pub urgency: notify_rust::Urgency,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxNotificationRetention {
    pub retained: bool,
    pub id: u32,
    pub registry_size: usize,
    pub retention_limit: usize,
    pub ttl_secs: u64,
    pub pruned_expired: usize,
    pub dropped_over_limit: usize,
}

#[cfg(target_os = "linux")]
pub struct LinuxNotificationRegistry {
    retained: Mutex<VecDeque<RetainedLinuxNotification>>,
}

#[cfg(target_os = "linux")]
struct RetainedLinuxNotification {
    created_at: Instant,
    _handle: notify_rust::NotificationHandle,
}

#[cfg(target_os = "linux")]
impl LinuxNotificationRegistry {
    pub fn new() -> Self {
        Self {
            retained: Mutex::new(VecDeque::new()),
        }
    }

    pub fn retain(&self, handle: notify_rust::NotificationHandle) -> LinuxNotificationRetention {
        let id = handle.id();
        let now = Instant::now();
        let mut retained = self
            .retained
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let pruned_expired =
            prune_expired_linux_notifications(&mut retained, now, LINUX_NOTIFICATION_RETENTION_TTL);

        retained.push_back(RetainedLinuxNotification {
            created_at: now,
            _handle: handle,
        });

        let dropped_over_limit =
            trim_linux_notifications_to_limit(&mut retained, LINUX_NOTIFICATION_RETENTION_LIMIT);

        LinuxNotificationRetention {
            retained: true,
            id,
            registry_size: retained.len(),
            retention_limit: LINUX_NOTIFICATION_RETENTION_LIMIT,
            ttl_secs: LINUX_NOTIFICATION_RETENTION_TTL.as_secs(),
            pruned_expired,
            dropped_over_limit,
        }
    }

    pub fn observe_unretained(&self, id: u32) -> LinuxNotificationRetention {
        let now = Instant::now();
        let mut retained = self
            .retained
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let pruned_expired =
            prune_expired_linux_notifications(&mut retained, now, LINUX_NOTIFICATION_RETENTION_TTL);

        LinuxNotificationRetention {
            retained: false,
            id,
            registry_size: retained.len(),
            retention_limit: LINUX_NOTIFICATION_RETENTION_LIMIT,
            ttl_secs: LINUX_NOTIFICATION_RETENTION_TTL.as_secs(),
            pruned_expired,
            dropped_over_limit: 0,
        }
    }
}

#[cfg(target_os = "linux")]
impl Default for LinuxNotificationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "linux")]
fn prune_expired_linux_notifications(
    retained: &mut VecDeque<RetainedLinuxNotification>,
    now: Instant,
    ttl: Duration,
) -> usize {
    let original_len = retained.len();
    retained.retain(|notification| now.duration_since(notification.created_at) < ttl);
    original_len - retained.len()
}

#[cfg(target_os = "linux")]
fn trim_linux_notifications_to_limit(
    retained: &mut VecDeque<RetainedLinuxNotification>,
    limit: usize,
) -> usize {
    let original_len = retained.len();
    while retained.len() > limit {
        retained.pop_front();
    }
    original_len - retained.len()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskNotificationKind {
    Start,
    Complete,
    SharingComplete,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskNotificationOpenTarget {
    pub dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskNotificationContent {
    pub kind: TaskNotificationKind,
    pub title: String,
    pub body: String,
    pub locale: &'static str,
    pub click_open_target: Option<TaskNotificationOpenTarget>,
    pub click_show_task_list: bool,
}

/// A submitted task identity supplied by the frontend with a start notification.
/// GIDs are stable for regular downloads and let Windows remove the exact start
/// toast even when aria2 later resolves a different display name.
#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskStartNotificationTask {
    pub name: String,
    #[serde(default)]
    pub gid: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotificationDispatchResult {
    #[cfg(not(target_os = "linux"))]
    Submitted,
    #[cfg(target_os = "linux")]
    Delivered {
        id: u32,
        identity: LinuxNotificationIdentity,
        retention: LinuxNotificationRetention,
    },
}

#[cfg(target_os = "linux")]
pub fn linux_notification_identity() -> LinuxNotificationIdentity {
    LinuxNotificationIdentity {
        app_name: "motrixnext",
        icon: "motrix-next",
        desktop_entry: "MotrixNext",
        urgency: notify_rust::Urgency::Normal,
    }
}

fn kind_for_event(event_name: &str) -> Option<TaskNotificationKind> {
    match event_name {
        events::TASK_COMPLETE => Some(TaskNotificationKind::Complete),
        events::SHARING_COMPLETE => Some(TaskNotificationKind::SharingComplete),
        events::TASK_ERROR => Some(TaskNotificationKind::Error),
        _ => None,
    }
}

fn notification_enabled(kind: TaskNotificationKind, config: &RuntimeConfig) -> bool {
    if !config.task_notification {
        return false;
    }

    match kind {
        TaskNotificationKind::Start => config.notify_on_start,
        TaskNotificationKind::Complete | TaskNotificationKind::SharingComplete => {
            config.notify_on_complete
        }
        TaskNotificationKind::Error => true,
    }
}

fn click_open_target_for_event(
    kind: TaskNotificationKind,
    event: &TaskEvent,
    config: &RuntimeConfig,
) -> Option<TaskNotificationOpenTarget> {
    if !config.open_folder_on_notification_click {
        return None;
    }

    match kind {
        TaskNotificationKind::Complete | TaskNotificationKind::SharingComplete => {
            notification_open_target_for_event(event)
        }
        TaskNotificationKind::Start | TaskNotificationKind::Error => None,
    }
}

fn notification_open_target_for_event(event: &TaskEvent) -> Option<TaskNotificationOpenTarget> {
    notification_open_dir_for_event(event).map(|dir| TaskNotificationOpenTarget { dir })
}

fn notification_open_dir_for_event(event: &TaskEvent) -> Option<String> {
    let dir = event.dir.trim();
    if !dir.is_empty() && looks_like_absolute_path(dir) {
        return Some(dir.to_string());
    }

    let parent_dir = |path: &str| {
        let path = path.trim();
        looks_like_absolute_path(path)
            .then(|| std::path::Path::new(path).parent())
            .flatten()
            .map(|parent| parent.to_string_lossy().to_string())
            .filter(|parent| !parent.is_empty())
    };

    event
        .files
        .iter()
        .filter(|file| file.selected.eq_ignore_ascii_case("true"))
        .find_map(|file| parent_dir(&file.path))
        .or_else(|| event.files.iter().find_map(|file| parent_dir(&file.path)))
}

fn looks_like_absolute_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/')
        || path.starts_with('\\')
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'\\' || bytes[2] == b'/'))
}

#[cfg(target_os = "linux")]
fn is_default_notification_action(action: &notify_rust::ActionResponse<'_>) -> bool {
    matches!(action, notify_rust::ActionResponse::Custom("default"))
}

#[cfg(target_os = "linux")]
fn spawn_click_open_target_handler(
    app: tauri::AppHandle,
    target: TaskNotificationOpenTarget,
    handle: notify_rust::NotificationHandle,
) {
    tauri::async_runtime::spawn(async move {
        handle
            .wait_for_action_async(|action| {
                if is_default_notification_action(action) {
                    open_notification_target(&app, &target);
                } else {
                    log::debug!("notification:click-open-target ignored action={action:?}");
                }
            })
            .await;
    });
}

#[cfg(target_os = "linux")]
fn spawn_click_show_task_list_handler(
    app: tauri::AppHandle,
    handle: notify_rust::NotificationHandle,
) {
    tauri::async_runtime::spawn(async move {
        handle
            .wait_for_action_async(|action| {
                if is_default_notification_action(action) {
                    show_notification_task_list(&app);
                } else {
                    log::debug!("notification:click-show-task-list ignored action={action:?}");
                }
            })
            .await;
    });
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) fn open_notification_target(
    app: &tauri::AppHandle,
    target: &TaskNotificationOpenTarget,
) {
    let path = std::path::Path::new(&target.dir);
    if !path.is_absolute() || !path.is_dir() {
        log::warn!(
            "notification:click-open-target rejected non-directory path={:?}",
            target.dir
        );
        return;
    }

    match crate::commands::fs::open_path_normalized(app.clone(), target.dir.clone()) {
        Ok(()) => log::info!("notification:click-open-target opened dir={:?}", target.dir),
        Err(error) => log::warn!(
            "notification:click-open-target failed dir={:?} error={error}",
            target.dir
        ),
    }
}

#[cfg(target_os = "linux")]
fn show_notification_task_list(app: &tauri::AppHandle) {
    crate::services::frontend_action::dispatch_frontend_action(
        app,
        crate::services::frontend_action::FrontendActionChannel::NotificationAction,
        crate::services::frontend_action::FrontendActionKind::ShowTaskList,
        "notification-click-show-task-list",
    );
}

pub fn build_task_notification(
    event_name: &str,
    event: &TaskEvent,
    config: &RuntimeConfig,
) -> Option<TaskNotificationContent> {
    let kind = kind_for_event(event_name)?;
    if !notification_enabled(kind, config) {
        return None;
    }

    let requested_locale = if config.locale == "auto" {
        sys_locale::get_locale().unwrap_or_else(|| "en-US".to_string())
    } else {
        config.locale.clone()
    };
    let locale = super::notification_i18n::resolve_supported_locale(&requested_locale);
    let texts = texts_for_locale(locale);
    let task_name = event.name.as_str();

    let (title, body) = match kind {
        TaskNotificationKind::Start => return None,
        TaskNotificationKind::Complete => (
            texts.download_complete_title.to_string(),
            format_task_message(texts.download_complete_body, task_name),
        ),
        TaskNotificationKind::SharingComplete => {
            if event.sharing_kind == Some("ed2k") {
                (
                    texts.ed2k_complete_title.to_string(),
                    format_task_message(texts.ed2k_complete_body, task_name),
                )
            } else {
                (
                    texts.bt_complete_title.to_string(),
                    format_task_message(texts.bt_complete_body, task_name),
                )
            }
        }
        TaskNotificationKind::Error => {
            let reason = event
                .error_message
                .as_deref()
                .filter(|message| !message.trim().is_empty())
                .unwrap_or(texts.error_unknown);
            (
                texts.download_failed_title.to_string(),
                format_error_message(texts.download_failed_body, task_name, reason),
            )
        }
    };

    Some(TaskNotificationContent {
        kind,
        title,
        body,
        locale,
        click_open_target: click_open_target_for_event(kind, event, config),
        click_show_task_list: false,
    })
}

pub fn build_task_start_notification(
    task_names: &[String],
    config: &RuntimeConfig,
) -> Option<TaskNotificationContent> {
    if !notification_enabled(TaskNotificationKind::Start, config) {
        return None;
    }

    let task_names = task_names
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    let first_name = task_names.first()?;
    let requested_locale = if config.locale == "auto" {
        sys_locale::get_locale().unwrap_or_else(|| "en-US".to_string())
    } else {
        config.locale.clone()
    };
    let locale = super::notification_i18n::resolve_supported_locale(&requested_locale);
    let texts = texts_for_locale(locale);
    let body = if task_names.len() == 1 {
        format_task_message(texts.download_start_body, first_name)
    } else {
        format_batch_task_message(
            texts.download_batch_start_body,
            first_name,
            task_names.len() - 1,
        )
    };

    Some(TaskNotificationContent {
        kind: TaskNotificationKind::Start,
        title: texts.download_start_title.to_string(),
        body,
        locale,
        click_open_target: None,
        click_show_task_list: config.open_task_list_on_start_notification_click,
    })
}

fn normalized_start_notification_tasks(
    task_names: &[String],
    tasks: &[TaskStartNotificationTask],
) -> Vec<TaskStartNotificationTask> {
    let source = if tasks.is_empty() {
        task_names
            .iter()
            .map(|name| TaskStartNotificationTask {
                name: name.clone(),
                gid: None,
            })
            .collect::<Vec<_>>()
    } else {
        tasks.to_vec()
    };

    source
        .into_iter()
        .filter_map(|task| {
            let name = task.name.trim();
            (!name.is_empty()).then(|| TaskStartNotificationTask {
                name: name.to_string(),
                gid: task.gid.and_then(|gid| {
                    let gid = gid.trim();
                    (!gid.is_empty()).then(|| gid.to_string())
                }),
            })
        })
        .collect()
}

pub fn send_task_start_notification(
    app: &tauri::AppHandle,
    task_names: &[String],
    tasks: &[TaskStartNotificationTask],
    config: &RuntimeConfig,
) -> Result<bool, AppError> {
    let tasks = normalized_start_notification_tasks(task_names, tasks);

    #[cfg(target_os = "windows")]
    {
        send_windows_task_start_notifications(app, &tasks, config)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let task_names = tasks
            .iter()
            .map(|task| task.name.clone())
            .collect::<Vec<_>>();
        let Some(content) = build_task_start_notification(&task_names, config) else {
            log::debug!("notification:skip reason=preference-disabled type=Start");
            return Ok(false);
        };

        send_native_notification(app, &content)?;
        log::info!(
            "notification:submitted type={:?} locale={} webview_alive={}",
            content.kind,
            content.locale,
            app.get_webview_window("main").is_some()
        );
        Ok(true)
    }
}

#[cfg(target_os = "windows")]
fn send_windows_task_start_notifications(
    app: &tauri::AppHandle,
    tasks: &[TaskStartNotificationTask],
    config: &RuntimeConfig,
) -> Result<bool, AppError> {
    if !notification_enabled(TaskNotificationKind::Start, config) {
        log::debug!("notification:skip reason=preference-disabled type=Start");
        return Ok(false);
    }

    let attempted = tasks.len().min(WINDOWS_START_NOTIFICATION_LIMIT);
    let mut submitted = 0usize;
    let mut first_error = None;
    for task in tasks.iter().take(attempted) {
        let Some(content) = build_task_start_notification(std::slice::from_ref(&task.name), config)
        else {
            continue;
        };
        let notification_key = task.gid.as_deref().unwrap_or(&task.name);
        match show_windows_start_notification(app, &content, notification_key) {
            Ok(()) => submitted += 1,
            Err(error) => {
                log::warn!(
                    "notification:windows-start-submit-failed task_name={:?} error={error}",
                    task.name
                );
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }

    if submitted == 0 {
        if let Some(error) = first_error {
            return Err(AppError::Io(error));
        }
        log::debug!("notification:skip reason=empty-task-names type=Start");
        return Ok(false);
    }

    if attempted > submitted {
        log::warn!(
            "notification:windows-start-partial submitted={} failed={}",
            submitted,
            attempted - submitted
        );
    }
    if tasks.len() > attempted {
        log::warn!(
            "notification:windows-start-truncated submitted={} omitted={} limit={}",
            submitted,
            tasks.len() - attempted,
            WINDOWS_START_NOTIFICATION_LIMIT
        );
    }

    log::info!(
        "notification:submitted type=Start count={} webview_alive={}",
        submitted,
        app.get_webview_window("main").is_some()
    );
    Ok(true)
}

pub fn send_app_notification(
    app: &tauri::AppHandle,
    title: &str,
    body: &str,
) -> Result<(), AppError> {
    let title = title.trim();
    let body = body.trim();
    if title.is_empty() || body.is_empty() {
        return Ok(());
    }

    let content = TaskNotificationContent {
        kind: TaskNotificationKind::Start,
        title: title.to_string(),
        body: body.to_string(),
        locale: "frontend",
        click_open_target: None,
        click_show_task_list: false,
    };
    send_native_notification(app, &content)
}

pub fn send_task_notification(
    app: &tauri::AppHandle,
    event_name: &str,
    event: &TaskEvent,
    config: &RuntimeConfig,
) {
    let Some(kind) = kind_for_event(event_name) else {
        return;
    };

    #[cfg(target_os = "windows")]
    remove_windows_start_notification_for_completed_task(app, kind, event);

    let Some(content) = build_task_notification(event_name, event, config) else {
        log::debug!(
            "notification:skip reason=preference-disabled type={kind:?} gid={}",
            event.gid
        );
        return;
    };

    log::debug!(
        "notification:send-start type={:?} gid={} locale={} title={:?}",
        content.kind,
        event.gid,
        content.locale,
        content.title
    );

    match send_platform_notification(app, &content) {
        Ok(dispatch) => {
            let webview_alive = app.get_webview_window("main").is_some();
            log_notification_success(&content, event, dispatch, webview_alive);
        }
        Err(e) => {
            log::warn!(
                "notification:failed type={:?} gid={} locale={} error={e}",
                content.kind,
                event.gid,
                content.locale
            );
        }
    }
}

pub fn send_native_notification(
    app: &tauri::AppHandle,
    content: &TaskNotificationContent,
) -> Result<(), AppError> {
    send_platform_notification(app, content)
        .map(|_| ())
        .map_err(AppError::Io)
}

#[cfg(target_os = "linux")]
fn log_notification_success(
    content: &TaskNotificationContent,
    event: &TaskEvent,
    dispatch: NotificationDispatchResult,
    webview_alive: bool,
) {
    match dispatch {
        NotificationDispatchResult::Delivered {
            id,
            identity,
            retention,
        } => {
            log::info!(
                "notification:delivered platform=linux id={} type={:?} gid={} locale={} webview_alive={} app_name={} icon={} desktop_entry={} urgency=normal retained={} registry_size={} retention_limit={} ttl_secs={} pruned_expired={} dropped_over_limit={}",
                id,
                content.kind,
                event.gid,
                content.locale,
                webview_alive,
                identity.app_name,
                identity.icon,
                identity.desktop_entry,
                retention.retained,
                retention.registry_size,
                retention.retention_limit,
                retention.ttl_secs,
                retention.pruned_expired,
                retention.dropped_over_limit
            );
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn log_notification_success(
    content: &TaskNotificationContent,
    event: &TaskEvent,
    dispatch: NotificationDispatchResult,
    webview_alive: bool,
) {
    match dispatch {
        NotificationDispatchResult::Submitted => {
            log::info!(
                "notification:submitted type={:?} gid={} locale={} webview_alive={}",
                content.kind,
                event.gid,
                content.locale,
                webview_alive
            );
        }
    }
}

#[cfg(target_os = "linux")]
fn send_platform_notification(
    app: &tauri::AppHandle,
    content: &TaskNotificationContent,
) -> Result<NotificationDispatchResult, String> {
    let identity = linux_notification_identity();
    let mut notification = notify_rust::Notification::new();
    notification
        .appname(identity.app_name)
        .icon(identity.icon)
        .hint(notify_rust::Hint::DesktopEntry(
            identity.desktop_entry.to_string(),
        ))
        .urgency(identity.urgency)
        .summary(&content.title)
        .body(&content.body);

    if content.click_open_target.is_some() || content.click_show_task_list {
        notification.action("default", "Open");
    }

    let handle = notification.show().map_err(|error| error.to_string())?;
    let registry = app.state::<LinuxNotificationRegistry>();
    let retention = if let Some(target) = content.click_open_target.clone() {
        let id = handle.id();
        let retention = registry.observe_unretained(id);
        spawn_click_open_target_handler(app.clone(), target, handle);
        retention
    } else if content.click_show_task_list {
        let id = handle.id();
        let retention = registry.observe_unretained(id);
        spawn_click_show_task_list_handler(app.clone(), handle);
        retention
    } else {
        registry.retain(handle)
    };

    Ok(NotificationDispatchResult::Delivered {
        id: retention.id,
        identity,
        retention,
    })
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
fn send_platform_notification(
    app: &tauri::AppHandle,
    content: &TaskNotificationContent,
) -> Result<NotificationDispatchResult, String> {
    let dispatch = show_default_platform_notification(app, content)?;

    if content.click_open_target.is_some() {
        log::debug!(
            "notification:click-open-target unsupported platform={} type={:?}",
            std::env::consts::OS,
            content.kind
        );
    }
    if content.click_show_task_list {
        log::debug!(
            "notification:click-show-task-list unsupported platform={} type={:?}",
            std::env::consts::OS,
            content.kind
        );
    }

    Ok(dispatch)
}

#[cfg(target_os = "windows")]
fn send_platform_notification(
    app: &tauri::AppHandle,
    content: &TaskNotificationContent,
) -> Result<NotificationDispatchResult, String> {
    if content.click_open_target.is_none() && !content.click_show_task_list {
        return show_default_platform_notification(app, content);
    }

    show_windows_notification(app, content, None, None)?;

    Ok(NotificationDispatchResult::Submitted)
}

#[cfg(target_os = "windows")]
fn show_windows_notification(
    app: &tauri::AppHandle,
    content: &TaskNotificationContent,
    tag: Option<&str>,
    group: Option<&str>,
) -> Result<(), String> {
    let app_id = windows_notification_app_id(app);
    let toast = build_windows_toast_notification(app, content)?;

    if let Some(tag) = tag {
        toast
            .SetTag(&HSTRING::from(tag))
            .map_err(|error| format!("{error:?}"))?;
    }
    if let Some(group) = group {
        toast
            .SetGroup(&HSTRING::from(group))
            .map_err(|error| format!("{error:?}"))?;
    }

    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(&app_id))
        .map_err(|error| format!("{error:?}"))?;
    notifier
        .Show(&toast)
        .map_err(|error| format!("{error:?}"))?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn show_windows_start_notification(
    app: &tauri::AppHandle,
    content: &TaskNotificationContent,
    task_key: &str,
) -> Result<(), String> {
    let tag = windows_start_notification_tag(task_key);
    show_windows_notification(
        app,
        content,
        Some(&tag),
        Some(WINDOWS_START_NOTIFICATION_GROUP),
    )?;

    log::debug!(
        "notification:windows-start-submitted tag={} task_key={task_key:?} clickable={}",
        tag,
        content.click_show_task_list
    );
    Ok(())
}

#[cfg(target_os = "windows")]
fn remove_windows_start_notification_for_completed_task(
    app: &tauri::AppHandle,
    kind: TaskNotificationKind,
    event: &TaskEvent,
) {
    if !matches!(
        kind,
        TaskNotificationKind::Complete | TaskNotificationKind::SharingComplete
    ) {
        return;
    }

    let app_id = windows_notification_app_id(app);
    for task_key in windows_start_notification_keys_for_completed_task(event) {
        let tag = windows_start_notification_tag(&task_key);
        match ToastNotificationManager::History().and_then(|history| {
            history.RemoveGroupedTagWithId(
                &HSTRING::from(&tag),
                &HSTRING::from(WINDOWS_START_NOTIFICATION_GROUP),
                &HSTRING::from(&app_id),
            )
        }) {
            Ok(()) => log::debug!(
                "notification:windows-start-removed tag={} task_key={task_key:?}",
                tag
            ),
            Err(error) => log::warn!(
                "notification:windows-start-remove-failed tag={} task_key={task_key:?} error={error:?}",
                tag
            ),
        }
    }
}

#[cfg(any(target_os = "windows", test))]
fn windows_start_notification_keys_for_completed_task(event: &TaskEvent) -> Vec<String> {
    let mut keys = Vec::with_capacity(3);
    for value in [
        Some(event.gid.as_str()),
        event.following.as_deref(),
        Some(event.name.as_str()),
    ] {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            continue;
        };
        if !keys.iter().any(|key| key == value) {
            keys.push(value.to_string());
        }
    }
    keys
}

#[cfg(target_os = "windows")]
fn build_windows_toast_notification(
    app: &tauri::AppHandle,
    content: &TaskNotificationContent,
) -> Result<ToastNotification, String> {
    let action_secret = notification_action_secret(app);
    if content.click_open_target.is_some() && action_secret.is_none() {
        log::warn!("notification:windows-click-disabled reason=missing-action-secret");
    }
    let toast_xml = XmlDocument::new().map_err(|error| format!("{error:?}"))?;
    toast_xml
        .LoadXml(&HSTRING::from(build_windows_toast_xml(
            content,
            action_secret.as_deref(),
        )))
        .map_err(|error| format!("{error:?}"))?;
    ToastNotification::CreateToastNotification(&toast_xml).map_err(|error| format!("{error:?}"))
}

#[cfg(any(target_os = "windows", test))]
fn build_windows_toast_xml(
    content: &TaskNotificationContent,
    action_secret: Option<&str>,
) -> String {
    let activation = windows_notification_activation_url(content, action_secret)
        .map(|url| {
            format!(
                r#" activationType="protocol" launch="{}""#,
                escape_windows_toast_xml(&url)
            )
        })
        .unwrap_or_default();

    format!(
        r#"<toast duration="short"{}><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual></toast>"#,
        activation,
        escape_windows_toast_xml(&content.title),
        escape_windows_toast_xml(&content.body)
    )
}

#[cfg(any(target_os = "windows", test))]
fn windows_notification_activation_url(
    content: &TaskNotificationContent,
    action_secret: Option<&str>,
) -> Option<String> {
    if let Some(target) = content.click_open_target.as_ref() {
        windows_open_folder_activation_url(target, action_secret?)
    } else {
        content
            .click_show_task_list
            .then(windows_show_task_list_activation_url)
    }
}

#[cfg(any(target_os = "windows", test))]
fn windows_open_folder_activation_url(
    target: &TaskNotificationOpenTarget,
    action_secret: &str,
) -> Option<String> {
    let dir = target.dir.trim();
    if dir.is_empty() || dir.chars().count() > WINDOWS_NOTIFICATION_DIR_MAX_CHARS {
        return None;
    }

    let signature = sign_notification_open_dir(action_secret, dir)?;
    let mut url = url::Url::parse("motrixnext://open-folder").ok()?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("dir", dir);
        query.append_pair("sig", &signature);
    }
    Some(url.to_string())
}

#[cfg(any(target_os = "windows", test))]
fn windows_show_task_list_activation_url() -> String {
    format!("motrixnext://{WINDOWS_NOTIFICATION_SHOW_TASK_LIST_ACTION}")
}

#[cfg(any(target_os = "windows", test))]
type NotificationActionMac = Hmac<Sha256>;

#[cfg(any(target_os = "windows", test))]
pub(crate) fn sign_notification_open_dir(secret: &str, dir: &str) -> Option<String> {
    let mut mac = NotificationActionMac::new_from_slice(secret.as_bytes()).ok()?;
    mac.update(WINDOWS_NOTIFICATION_OPEN_FOLDER_ACTION.as_bytes());
    mac.update(&[0]);
    mac.update(dir.as_bytes());
    Some(encode_hex(&mac.finalize().into_bytes()))
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn verify_notification_open_dir_signature(
    secret: &str,
    dir: &str,
    signature: &str,
) -> bool {
    let Some(signature) = decode_sha256_hex(signature) else {
        return false;
    };
    let Ok(mut mac) = NotificationActionMac::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(WINDOWS_NOTIFICATION_OPEN_FOLDER_ACTION.as_bytes());
    mac.update(&[0]);
    mac.update(dir.as_bytes());
    mac.verify_slice(&signature).is_ok()
}

#[cfg(any(target_os = "windows", test))]
fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(*byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(*byte & 0x0f)]));
    }
    encoded
}

#[cfg(any(target_os = "windows", test))]
fn decode_sha256_hex(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }

    let mut decoded = [0u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        decoded[index] = (decode_hex_nibble(pair[0])? << 4) | decode_hex_nibble(pair[1])?;
    }
    Some(decoded)
}

#[cfg(any(target_os = "windows", test))]
fn decode_hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(any(target_os = "windows", test))]
fn windows_start_notification_tag(task_name: &str) -> String {
    format!("start-{:016x}", fnv1a64(task_name.trim()))
}

#[cfg(any(target_os = "windows", test))]
fn fnv1a64(value: &str) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;

    value.as_bytes().iter().fold(OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    })
}

#[cfg(any(target_os = "windows", test))]
fn escape_windows_toast_xml(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(not(target_os = "linux"))]
fn show_default_platform_notification(
    app: &tauri::AppHandle,
    content: &TaskNotificationContent,
) -> Result<NotificationDispatchResult, String> {
    app.notification()
        .builder()
        .title(content.title.clone())
        .body(content.body.clone())
        .show()
        .map_err(|error| error.to_string())?;

    Ok(NotificationDispatchResult::Submitted)
}

#[cfg(target_os = "windows")]
fn windows_notification_app_id(app: &tauri::AppHandle) -> String {
    let identifier = app.config().identifier.clone();
    let Ok(exe) = std::env::current_exe() else {
        return identifier;
    };
    let Some(exe_dir) = exe.parent() else {
        return identifier;
    };

    let separator = std::path::MAIN_SEPARATOR;
    let exe_dir = exe_dir.display().to_string();
    let debug_suffix = format!("{separator}target{separator}debug");
    let release_suffix = format!("{separator}target{separator}release");
    if exe_dir.ends_with(&debug_suffix) || exe_dir.ends_with(&release_suffix) {
        WINDOWS_DEBUG_NOTIFICATION_APP_ID.to_string()
    } else {
        identifier
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn notification_action_secret(app: &tauri::AppHandle) -> Option<String> {
    let preferences = app.store("config.json").ok()?.get("preferences")?;
    ["extensionApiSecret", "rpcSecret"]
        .into_iter()
        .find_map(|key| {
            preferences
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|secret| !secret.is_empty())
                .map(str::to_string)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> RuntimeConfig {
        RuntimeConfig {
            locale: "en-US".to_string(),
            task_notification: true,
            notify_on_complete: true,
            notify_on_start: true,
            ..RuntimeConfig::default()
        }
    }

    fn event() -> TaskEvent {
        TaskEvent {
            gid: "g1".to_string(),
            name: "file.zip".to_string(),
            status: "complete".to_string(),
            error_code: None,
            error_message: None,
            dir: "/tmp".to_string(),
            total_length: "1".to_string(),
            completed_length: "1".to_string(),
            info_hash: None,
            magnet_link: None,
            ed2k_link: None,
            is_bt: false,
            is_ed2k: false,
            sharing_kind: None,
            #[cfg(any(target_os = "windows", test))]
            following: None,
            files: vec![crate::services::monitor::TaskEventFile {
                path: "/tmp/file.zip".to_string(),
                length: "1".to_string(),
                selected: "true".to_string(),
                uris: Vec::new(),
            }],
            announce_list: Vec::new(),
        }
    }

    #[test]
    fn builds_localised_complete_notification() {
        let content = build_task_notification(events::TASK_COMPLETE, &event(), &cfg()).unwrap();
        assert_eq!(content.kind, TaskNotificationKind::Complete);
        assert_eq!(content.title, "Download Complete");
        assert_eq!(content.body, "Saved: file.zip");
        assert_eq!(content.locale, "en-US");
        assert_eq!(content.click_open_target, None);
    }

    #[test]
    fn complete_notification_includes_click_open_target_when_enabled() {
        let mut config = cfg();
        config.open_folder_on_notification_click = true;

        let content = build_task_notification(events::TASK_COMPLETE, &event(), &config).unwrap();

        assert_eq!(content.kind, TaskNotificationKind::Complete);
        assert_eq!(
            content.click_open_target,
            Some(TaskNotificationOpenTarget {
                dir: "/tmp".to_string(),
            })
        );
    }

    #[test]
    fn builds_localised_bt_complete_notification() {
        let mut ev = event();
        ev.is_bt = true;
        ev.sharing_kind = Some("bt");
        let content = build_task_notification(events::SHARING_COMPLETE, &ev, &cfg()).unwrap();
        assert_eq!(content.kind, TaskNotificationKind::SharingComplete);
        assert_eq!(content.title, "BT Download Complete");
        assert_eq!(content.body, "Seeding: file.zip");
    }

    #[test]
    fn sharing_complete_notification_includes_click_open_target_when_enabled() {
        let mut ev = event();
        ev.is_bt = true;
        ev.sharing_kind = Some("bt");
        let mut config = cfg();
        config.open_folder_on_notification_click = true;

        let content = build_task_notification(events::SHARING_COMPLETE, &ev, &config).unwrap();

        assert_eq!(content.kind, TaskNotificationKind::SharingComplete);
        assert_eq!(
            content.click_open_target,
            Some(TaskNotificationOpenTarget {
                dir: "/tmp".to_string(),
            })
        );
    }

    #[test]
    fn builds_localised_ed2k_sharing_notification() {
        let mut ev = event();
        ev.is_ed2k = true;
        ev.sharing_kind = Some("ed2k");
        let content = build_task_notification(events::SHARING_COMPLETE, &ev, &cfg()).unwrap();
        assert_eq!(content.kind, TaskNotificationKind::SharingComplete);
        assert_eq!(content.title, "ED2K Download Complete");
        assert_eq!(content.body, "Sharing: file.zip");
    }

    #[test]
    fn builds_zh_cn_ed2k_sharing_notification() {
        let mut ev = event();
        ev.is_ed2k = true;
        ev.sharing_kind = Some("ed2k");
        let mut config = cfg();
        config.locale = "zh-CN".to_string();

        let content = build_task_notification(events::SHARING_COMPLETE, &ev, &config).unwrap();

        assert_eq!(content.kind, TaskNotificationKind::SharingComplete);
        assert_eq!(content.title, "ED2K 下载完成");
        assert_eq!(content.body, "共享中：file.zip");
        assert_eq!(content.locale, "zh-CN");
    }

    #[test]
    fn builds_localised_error_notification_with_reason() {
        let mut ev = event();
        ev.error_message = Some("Network error".to_string());
        let content = build_task_notification(events::TASK_ERROR, &ev, &cfg()).unwrap();
        assert_eq!(content.kind, TaskNotificationKind::Error);
        assert_eq!(content.title, "Download Failed");
        assert_eq!(content.body, "file.zip: Network error");
        assert_eq!(content.click_open_target, None);
    }

    #[test]
    fn error_notification_ignores_click_open_target_setting() {
        let mut ev = event();
        ev.error_message = Some("Network error".to_string());
        let mut config = cfg();
        config.open_folder_on_notification_click = true;

        let content = build_task_notification(events::TASK_ERROR, &ev, &config).unwrap();

        assert_eq!(content.kind, TaskNotificationKind::Error);
        assert_eq!(content.click_open_target, None);
    }

    #[test]
    fn complete_notification_uses_file_parent_when_dir_is_blank() {
        let mut ev = event();
        ev.dir = "  ".to_string();
        let mut config = cfg();
        config.open_folder_on_notification_click = true;

        let content = build_task_notification(events::TASK_COMPLETE, &ev, &config).unwrap();

        assert_eq!(content.kind, TaskNotificationKind::Complete);
        assert_eq!(
            content.click_open_target,
            Some(TaskNotificationOpenTarget {
                dir: "/tmp".to_string(),
            })
        );
    }

    #[test]
    fn complete_notification_rejects_relative_open_targets() {
        let mut ev = event();
        ev.dir = "relative/downloads".to_string();
        ev.files[0].path = "relative/downloads/file.zip".to_string();
        let mut config = cfg();
        config.open_folder_on_notification_click = true;

        let content = build_task_notification(events::TASK_COMPLETE, &ev, &config).unwrap();

        assert_eq!(content.click_open_target, None);
    }

    #[test]
    fn skips_completion_when_complete_notifications_are_disabled() {
        let mut config = cfg();
        config.notify_on_complete = false;
        assert!(build_task_notification(events::TASK_COMPLETE, &event(), &config).is_none());
        assert!(build_task_notification(events::TASK_ERROR, &event(), &config).is_some());
    }

    #[test]
    fn skips_all_when_task_notifications_are_disabled() {
        let mut config = cfg();
        config.task_notification = false;
        assert!(build_task_notification(events::TASK_COMPLETE, &event(), &config).is_none());
        assert!(build_task_notification(events::TASK_ERROR, &event(), &config).is_none());
        assert!(build_task_start_notification(&["file.zip".to_string()], &config).is_none());
    }

    #[test]
    fn builds_localised_start_notification() {
        let content = build_task_start_notification(&["file.zip".to_string()], &cfg()).unwrap();
        assert_eq!(content.kind, TaskNotificationKind::Start);
        assert_eq!(content.title, "Download Started");
        assert_eq!(content.body, "Downloading: file.zip");
        assert_eq!(content.locale, "en-US");
        assert_eq!(content.click_open_target, None);
        assert!(!content.click_show_task_list);
    }

    #[test]
    fn start_notification_includes_click_show_task_list_when_enabled() {
        let mut config = cfg();
        config.open_task_list_on_start_notification_click = true;

        let content = build_task_start_notification(&["file.zip".to_string()], &config).unwrap();

        assert_eq!(content.kind, TaskNotificationKind::Start);
        assert!(content.click_show_task_list);
    }

    #[test]
    fn builds_localised_batch_start_notification() {
        let content = build_task_start_notification(
            &[
                "file.zip".to_string(),
                "b.iso".to_string(),
                "c.torrent".to_string(),
            ],
            &cfg(),
        )
        .unwrap();
        assert_eq!(content.kind, TaskNotificationKind::Start);
        assert_eq!(content.title, "Download Started");
        assert_eq!(content.body, "Downloading: file.zip and 2 other task(s)");
    }

    #[test]
    fn batch_start_notification_ignores_blank_task_names_in_count() {
        let content = build_task_start_notification(
            &[
                "file.zip".to_string(),
                "  ".to_string(),
                "b.iso".to_string(),
            ],
            &cfg(),
        )
        .unwrap();

        assert_eq!(content.body, "Downloading: file.zip and 1 other task(s)");
    }

    #[test]
    fn skips_start_when_start_notifications_are_disabled() {
        let mut config = cfg();
        config.notify_on_start = false;
        assert!(build_task_start_notification(&["file.zip".to_string()], &config).is_none());
    }

    #[test]
    fn skips_start_when_task_names_are_empty() {
        assert!(build_task_start_notification(&[], &cfg()).is_none());
        assert!(build_task_start_notification(&["  ".to_string()], &cfg()).is_none());
    }

    #[test]
    fn start_notification_tasks_preserve_gids_and_trim_legacy_names() {
        let tasks = normalized_start_notification_tasks(
            &[" legacy.zip ".to_string()],
            &[
                TaskStartNotificationTask {
                    name: "  renamed.zip ".to_string(),
                    gid: Some(" gid-123 ".to_string()),
                },
                TaskStartNotificationTask {
                    name: " ".to_string(),
                    gid: Some("ignored".to_string()),
                },
            ],
        );

        assert_eq!(
            tasks,
            vec![TaskStartNotificationTask {
                name: "renamed.zip".to_string(),
                gid: Some("gid-123".to_string()),
            }]
        );
        assert_eq!(
            normalized_start_notification_tasks(&[" legacy.zip ".to_string()], &[]),
            vec![TaskStartNotificationTask {
                name: "legacy.zip".to_string(),
                gid: None,
            }]
        );
    }

    #[test]
    fn notification_open_dir_signatures_reject_tampering() {
        let signature = sign_notification_open_dir("test-secret", "C:\\Downloads").unwrap();

        assert!(verify_notification_open_dir_signature(
            "test-secret",
            "C:\\Downloads",
            &signature
        ));
        assert!(!verify_notification_open_dir_signature(
            "test-secret",
            "C:\\Windows",
            &signature
        ));
        assert!(!verify_notification_open_dir_signature(
            "other-secret",
            "C:\\Downloads",
            &signature
        ));
        assert!(!verify_notification_open_dir_signature(
            "test-secret",
            "C:\\Downloads",
            "not-hex"
        ));
    }

    #[test]
    fn windows_toast_xml_contains_protocol_activation_and_escapes_text_content() {
        let content = TaskNotificationContent {
            kind: TaskNotificationKind::Complete,
            title: "A&B <done>".to_string(),
            body: "Saved: \"it's here\"".to_string(),
            locale: "en-US",
            click_open_target: Some(TaskNotificationOpenTarget {
                dir: "C:\\Downloads".to_string(),
            }),
            click_show_task_list: false,
        };

        let xml = build_windows_toast_xml(&content, Some("test-secret"));

        assert!(xml.contains(r#"activationType="protocol""#));
        assert!(xml.contains(r#"launch="motrixnext://open-folder?dir=C%3A%5CDownloads&amp;sig="#));
        assert!(xml.contains("A&amp;B &lt;done&gt;"));
        assert!(xml.contains("Saved: &quot;it&apos;s here&quot;"));
    }

    #[test]
    fn windows_folder_activation_is_disabled_without_a_secret() {
        let content = TaskNotificationContent {
            kind: TaskNotificationKind::Complete,
            title: "Download Complete".to_string(),
            body: "Saved: file.zip".to_string(),
            locale: "en-US",
            click_open_target: Some(TaskNotificationOpenTarget {
                dir: "C:\\Downloads".to_string(),
            }),
            click_show_task_list: false,
        };

        let xml = build_windows_toast_xml(&content, None);

        assert!(!xml.contains(r#"activationType="protocol""#));
    }

    #[test]
    fn windows_toast_xml_contains_show_task_list_activation() {
        let content = TaskNotificationContent {
            kind: TaskNotificationKind::Start,
            title: "Download Started".to_string(),
            body: "Downloading: file.zip".to_string(),
            locale: "en-US",
            click_open_target: None,
            click_show_task_list: true,
        };

        let xml = build_windows_toast_xml(&content, None);

        assert!(xml.contains(r#"activationType="protocol""#));
        assert!(xml.contains(r#"launch="motrixnext://show-task-list""#));
    }

    #[test]
    fn windows_start_notification_tag_is_stable_and_trimmed() {
        assert_eq!(
            windows_start_notification_tag(" file.zip "),
            windows_start_notification_tag("file.zip")
        );
        assert!(windows_start_notification_tag("file.zip").starts_with("start-"));
        assert_ne!(
            windows_start_notification_tag("file.zip"),
            windows_start_notification_tag("other.zip")
        );
    }

    #[test]
    fn windows_completion_notification_keys_include_following_metadata_gid() {
        let mut completion = event();
        completion.gid = "child-gid".to_string();
        completion.following = Some("metadata-gid".to_string());

        assert_eq!(
            windows_start_notification_keys_for_completed_task(&completion),
            vec![
                "child-gid".to_string(),
                "metadata-gid".to_string(),
                "file.zip".to_string(),
            ]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_notification_identity_matches_gnome_desktop_entry() {
        let identity = linux_notification_identity();
        assert_eq!(identity.app_name, "motrixnext");
        assert_eq!(identity.icon, "motrix-next");
        assert_eq!(identity.desktop_entry, "MotrixNext");
        assert_eq!(identity.urgency, notify_rust::Urgency::Normal);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_default_notification_action_is_recognized() {
        assert!(is_default_notification_action(
            &notify_rust::ActionResponse::Custom("default")
        ));
        assert!(!is_default_notification_action(
            &notify_rust::ActionResponse::Custom("other")
        ));
        assert!(!is_default_notification_action(
            &notify_rust::ActionResponse::Closed(notify_rust::CloseReason::Dismissed)
        ));
    }
}
