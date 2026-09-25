use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

#[cfg(any(target_os = "windows", test))]
const RAYBURST_SCHEME: &str = "rayburst";
#[cfg(any(target_os = "windows", test))]
const NOTIFICATION_OPEN_FOLDER_ACTION: &str = "open-folder";
#[cfg(any(target_os = "windows", test))]
const NOTIFICATION_SHOW_TASK_LIST_ACTION: &str = "show-task-list";
#[cfg(any(target_os = "windows", test))]
const NOTIFICATION_TASK_ACTION_ROUTE: &str = "task-action";
#[cfg(any(target_os = "windows", test))]
const NOTIFICATION_ACTIVATE_ACTION: &str = "activate";
#[cfg(any(target_os = "windows", test))]
const NOTIFICATION_OPEN_FILE_ACTION: &str = "open-file";
#[cfg(any(target_os = "windows", test))]
const NOTIFICATION_SHOW_IN_FOLDER_ACTION: &str = "show-in-folder";
#[cfg(any(target_os = "windows", test))]
const NOTIFICATION_GID_MAX_CHARS: usize = 128;

/// Deep-link URLs waiting for a recreated WebView to finish booting.
///
/// Lightweight mode destroys the main WebView when the user minimizes to tray.
/// External inputs must therefore survive the gap between native wake-up and
/// frontend listener registration.
#[derive(Debug, Default)]
struct PendingDeepLinks {
    queue: Vec<String>,
    frontend_ready: bool,
    silent: bool,
}

pub struct PendingDeepLinkState(Mutex<PendingDeepLinks>);

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingDeepLinksPayload {
    pub urls: Vec<String>,
    pub silent: bool,
}

impl PendingDeepLinkState {
    pub fn new() -> Self {
        Self(Mutex::new(PendingDeepLinks::default()))
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

/// Return only argv entries that represent external download inputs.
///
/// Used by the desktop single-instance callback. CLI flags are ignored; URL
/// schemes and supported local metadata/torrent files are forwarded.
pub fn filter_external_input_args(args: &[String]) -> Vec<String> {
    args.iter()
        .filter(|arg| {
            if arg.starts_with('-') {
                return false;
            }
            let lower = arg.to_lowercase();
            lower.contains("://")
                || lower.starts_with("magnet:")
                || lower.starts_with("ed2k://")
                || lower.ends_with(".torrent")
        })
        .cloned()
        .collect()
}

/// Handles native app actions that should not be forwarded to the frontend.
///
/// Windows notification-center clicks arrive as a protocol activation when the
/// original in-process toast callback is no longer available. Notification
/// bodies activate the main window; task file operations use dedicated action
/// URLs and are handled separately below.
#[cfg(target_os = "windows")]
pub fn handle_native_action_args(app: &AppHandle, args: &[String], source: &'static str) -> bool {
    let mut handled = false;
    for arg in args {
        if is_notification_open_folder_url(arg) {
            handled = true;
            let Some(secret) = crate::services::notification::notification_action_secret(app)
            else {
                log::warn!(
                    "deep_link:native-action-rejected source={source} reason=missing-secret"
                );
                continue;
            };
            if notification_open_target_from_url(arg, &secret).is_none() {
                log::warn!(
                    "deep_link:native-action-rejected source={source} reason=invalid-signature"
                );
                continue;
            }
            crate::tray::activate_main_window(app, source);
            log::info!("deep_link:native-action-activate source={source} legacy=open-folder");
        } else if is_notification_show_task_list_candidate(arg) {
            handled = true;
            if is_notification_show_task_list_url(arg) {
                crate::services::frontend_action::dispatch_frontend_action(
                    app,
                    crate::services::frontend_action::FrontendActionChannel::NotificationAction,
                    crate::services::frontend_action::FrontendActionKind::ShowTaskList,
                    source,
                );
            } else {
                log::warn!("deep_link:native-action-rejected source={source} reason=invalid-show-task-list");
            }
        } else if is_notification_activate_candidate(arg) {
            handled = true;
            if is_notification_activate_url(arg) {
                crate::tray::activate_main_window(app, source);
            } else {
                log::warn!(
                    "deep_link:native-action-rejected source={source} reason=invalid-activate"
                );
            }
        } else if is_notification_task_action_url(arg) {
            handled = true;
            if let Some((action, gid)) = notification_task_action_from_url(arg) {
                crate::tray::cancel_pending_main_window_activation();
                crate::services::frontend_action::dispatch_frontend_action_with_payload_preserving_window(
                    app,
                    crate::services::frontend_action::FrontendActionChannel::NotificationAction,
                    action,
                    gid,
                    source,
                );
            } else {
                log::warn!(
                    "deep_link:native-action-rejected source={source} reason=invalid-task-action"
                );
            }
        }
    }
    handled
}

#[cfg(any(target_os = "windows", test))]
fn notification_open_target_from_url(
    value: &str,
    secret: &str,
) -> Option<crate::services::notification::TaskNotificationOpenTarget> {
    let parsed = url::Url::parse(value).ok()?;
    if parsed.scheme() != RAYBURST_SCHEME {
        return None;
    }

    let action = parsed
        .host_str()
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| parsed.path().trim_start_matches('/'));
    if action != NOTIFICATION_OPEN_FOLDER_ACTION {
        return None;
    }

    let mut dir = None;
    let mut signature = None;
    for (key, value) in parsed.query_pairs() {
        match key.as_ref() {
            "dir" if dir.is_none() => dir = Some(value.into_owned()),
            "sig" if signature.is_none() => signature = Some(value.into_owned()),
            _ => return None,
        }
    }

    let dir = dir?.trim().to_string();
    let signature = signature?;
    if dir.is_empty()
        || dir.chars().count() > crate::services::notification::WINDOWS_NOTIFICATION_DIR_MAX_CHARS
        || !crate::services::notification::verify_notification_open_dir_signature(
            secret, &dir, &signature,
        )
    {
        return None;
    }

    Some(crate::services::notification::TaskNotificationOpenTarget { dir })
}

#[cfg(target_os = "windows")]
fn is_notification_open_folder_url(value: &str) -> bool {
    matches!(
        motrix_action_from_url(value).as_deref(),
        Some(NOTIFICATION_OPEN_FOLDER_ACTION)
    )
}

#[cfg(any(target_os = "windows", test))]
fn is_notification_show_task_list_url(value: &str) -> bool {
    matches!(
        motrix_action_from_url(value).as_deref(),
        Some(NOTIFICATION_SHOW_TASK_LIST_ACTION)
    ) && url_has_no_query_or_fragment(value)
}

#[cfg(target_os = "windows")]
fn is_notification_show_task_list_candidate(value: &str) -> bool {
    motrix_action_from_url(value).as_deref() == Some(NOTIFICATION_SHOW_TASK_LIST_ACTION)
}

#[cfg(any(target_os = "windows", test))]
fn is_notification_activate_url(value: &str) -> bool {
    motrix_action_from_url(value).as_deref() == Some(NOTIFICATION_ACTIVATE_ACTION)
        && url_has_no_query_or_fragment(value)
}

#[cfg(target_os = "windows")]
fn is_notification_activate_candidate(value: &str) -> bool {
    motrix_action_from_url(value).as_deref() == Some(NOTIFICATION_ACTIVATE_ACTION)
}

#[cfg(any(target_os = "windows", test))]
fn notification_task_action_from_url(
    value: &str,
) -> Option<(crate::services::frontend_action::FrontendActionKind, String)> {
    let parsed = url::Url::parse(value).ok()?;
    if parsed.scheme() != RAYBURST_SCHEME
        || parsed.fragment().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
    {
        return None;
    }

    let path_segments = if parsed.path().is_empty() {
        Vec::new()
    } else {
        parsed.path_segments()?.collect::<Vec<_>>()
    };
    let (path_action, path_gid) = match parsed.host_str() {
        Some(host) if host == NOTIFICATION_TASK_ACTION_ROUTE => match path_segments.as_slice() {
            [] | [""] => (None, None),
            [action, gid] if !action.is_empty() && !gid.is_empty() => {
                (Some((*action).to_string()), Some((*gid).to_string()))
            }
            _ => return None,
        },
        None => match path_segments.as_slice() {
            [route] | [route, ""] if *route == NOTIFICATION_TASK_ACTION_ROUTE => (None, None),
            [route, action, gid]
                if *route == NOTIFICATION_TASK_ACTION_ROUTE
                    && !action.is_empty()
                    && !gid.is_empty() =>
            {
                (Some((*action).to_string()), Some((*gid).to_string()))
            }
            _ => return None,
        },
        _ => return None,
    };

    let (action, gid) = match (path_action, path_gid) {
        (Some(action), Some(gid)) if parsed.query().is_none() => (action, gid),
        (None, None) => {
            let mut action = None;
            let mut gid = None;
            for (key, value) in parsed.query_pairs() {
                match key.as_ref() {
                    "action" if action.is_none() => action = Some(value.into_owned()),
                    "gid" if gid.is_none() => gid = Some(value.into_owned()),
                    _ => return None,
                }
            }
            (action?, gid?)
        }
        _ => return None,
    };

    let gid = gid.trim().to_string();
    if gid.is_empty()
        || gid.chars().count() > NOTIFICATION_GID_MAX_CHARS
        || !gid
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return None;
    }

    let action = match action.as_str() {
        NOTIFICATION_OPEN_FILE_ACTION => {
            crate::services::frontend_action::FrontendActionKind::OpenTaskFile
        }
        NOTIFICATION_SHOW_IN_FOLDER_ACTION => {
            crate::services::frontend_action::FrontendActionKind::ShowTaskInFolder
        }
        _ => return None,
    };
    Some((action, gid))
}

#[cfg(target_os = "windows")]
fn is_notification_task_action_url(value: &str) -> bool {
    url::Url::parse(value)
        .map(|parsed| {
            if parsed.scheme() != RAYBURST_SCHEME {
                return false;
            }
            if parsed.host_str() == Some(NOTIFICATION_TASK_ACTION_ROUTE) {
                return true;
            }
            parsed.host_str().is_none()
                && parsed
                    .path_segments()
                    .and_then(|mut segments| segments.next())
                    == Some(NOTIFICATION_TASK_ACTION_ROUTE)
        })
        .unwrap_or(false)
}

#[cfg(any(target_os = "windows", test))]
fn url_has_no_query_or_fragment(value: &str) -> bool {
    url::Url::parse(value)
        .map(|parsed| parsed.query().is_none() && parsed.fragment().is_none())
        .unwrap_or(false)
}

#[cfg(any(target_os = "windows", test))]
fn motrix_action_from_url(value: &str) -> Option<String> {
    let parsed = url::Url::parse(value).ok()?;
    if parsed.scheme() != RAYBURST_SCHEME {
        return None;
    }

    Some(
        parsed
            .host_str()
            .filter(|host| !host.is_empty())
            .unwrap_or_else(|| parsed.path().trim_start_matches('/'))
            .to_string(),
    )
}

/// Returns true when argv belongs to the OS autostart path.
pub fn is_autostart_arg_launch(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--autostart" || arg.starts_with("--autostart="))
}

/// Drain pending external inputs for the frontend boot path.
pub fn take_pending_deep_links(state: &PendingDeepLinkState) -> PendingDeepLinksPayload {
    match state.0.lock() {
        Ok(mut inner) => {
            inner.frontend_ready = true;
            take_pending_payload(&mut inner)
        }
        Err(poisoned) => {
            let mut inner = poisoned.into_inner();
            inner.frontend_ready = true;
            take_pending_payload(&mut inner)
        }
    }
}

pub fn peek_pending_deep_links_silent(state: &PendingDeepLinkState) -> bool {
    state.0.lock().map(|inner| inner.silent).unwrap_or(false)
}

fn take_pending_payload(inner: &mut PendingDeepLinks) -> PendingDeepLinksPayload {
    PendingDeepLinksPayload {
        urls: std::mem::take(&mut inner.queue),
        silent: std::mem::take(&mut inner.silent),
    }
}

/// Mark the frontend event listeners as unavailable.
///
/// Lightweight mode destroys the WebView while keeping the process alive. The
/// next recreated WebView must register listeners and drain pending inputs
/// before native code can safely emit directly to it again.
pub fn mark_frontend_unready(app: &AppHandle) {
    if let Some(state) = app.try_state::<PendingDeepLinkState>() {
        state.set_frontend_ready(false);
    }
}

/// Route external inputs to the active frontend, or queue them before waking a
/// destroyed lightweight-mode window.
pub fn route_external_inputs(app: &AppHandle, urls: Vec<String>, source: &'static str) {
    route_external_inputs_with_intent(app, urls, source, false);
}

fn route_external_inputs_with_intent(
    app: &AppHandle,
    urls: Vec<String>,
    source: &'static str,
    silent: bool,
) {
    if urls.is_empty() {
        log::debug!("deep_link:route source={source} count=0 skipped=true");
        return;
    }

    let window_was_alive = app.get_webview_window("main").is_some();
    log::info!(
        "deep_link:route source={source} count={} window_alive={window_was_alive}",
        urls.len()
    );

    let frontend_ready = is_frontend_ready(app);
    if window_was_alive && frontend_ready {
        crate::tray::request_main_window(app, source, !silent);
        let payload = PendingDeepLinksPayload {
            urls: urls.clone(),
            silent,
        };
        match app.emit("deep-link-open", payload) {
            Ok(()) => return,
            Err(e) => {
                log::warn!("deep_link:emit-failed source={source} error={e}");
            }
        }
    }

    queue_pending_deep_links(app, &urls, source, silent);
    crate::tray::request_main_window(app, source, !silent);
}

fn queue_pending_deep_links(app: &AppHandle, urls: &[String], source: &'static str, silent: bool) {
    match app.try_state::<PendingDeepLinkState>() {
        Some(state) => match state.0.lock() {
            Ok(mut inner) => {
                let added = append_unique_pending(&mut inner.queue, urls);
                inner.silent = inner.silent || silent;
                log::info!(
                    "deep_link:queued source={source} count={} added={} pending={} silent={}",
                    urls.len(),
                    added,
                    inner.queue.len(),
                    inner.silent
                );
            }
            Err(poisoned) => {
                let mut inner = poisoned.into_inner();
                let added = append_unique_pending(&mut inner.queue, urls);
                inner.silent = inner.silent || silent;
                log::warn!(
                    "deep_link:queued-after-poison source={source} count={} added={} pending={} silent={}",
                    urls.len(),
                    added,
                    inner.queue.len(),
                    inner.silent
                );
            }
        },
        None => {
            log::error!(
                "deep_link:queue-unavailable source={source} count={}",
                urls.len()
            );
        }
    }
}

fn append_unique_pending(queue: &mut Vec<String>, urls: &[String]) -> usize {
    let mut added = 0;
    for url in urls {
        if queue.iter().any(|pending| pending == url) {
            continue;
        }
        queue.push(url.clone());
        added += 1;
    }
    added
}

fn is_frontend_ready(app: &AppHandle) -> bool {
    app.try_state::<PendingDeepLinkState>()
        .map(|state| state.frontend_ready())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{
        append_unique_pending, filter_external_input_args, is_autostart_arg_launch,
        is_notification_activate_url, is_notification_show_task_list_url,
        notification_open_target_from_url, notification_task_action_from_url,
        take_pending_deep_links, PendingDeepLinkState,
    };
    use crate::services::notification::{sign_notification_open_dir, TaskNotificationOpenTarget};

    fn signed_open_folder_url(dir: &str, secret: &str) -> String {
        let signature = sign_notification_open_dir(secret, dir).expect("sign notification action");
        let mut url = url::Url::parse("rayburst://open-folder").expect("parse static action URL");
        url.query_pairs_mut()
            .append_pair("dir", dir)
            .append_pair("sig", &signature);
        url.to_string()
    }

    #[test]
    fn filters_supported_external_inputs_from_argv() {
        let args = vec![
            "/Applications/Rayburst.app".to_string(),
            "--flag".to_string(),
            "file:///Users/example/ubuntu.torrent".to_string(),
            "magnet:?xt=urn:btih:abc".to_string(),
            "ed2k://|file|ubuntu.iso|123|0123456789abcdef0123456789abcdef|/".to_string(),
            "notes.txt".to_string(),
        ];

        let filtered = filter_external_input_args(&args);

        assert_eq!(
            filtered,
            vec![
                "file:///Users/example/ubuntu.torrent".to_string(),
                "magnet:?xt=urn:btih:abc".to_string(),
                "ed2k://|file|ubuntu.iso|123|0123456789abcdef0123456789abcdef|/".to_string()
            ]
        );
    }

    #[test]
    fn detects_autostart_args_for_empty_single_instance_launches() {
        assert!(is_autostart_arg_launch(&[
            "Rayburst.exe".to_string(),
            "--autostart".to_string(),
        ]));
        assert!(is_autostart_arg_launch(&[
            "Rayburst.exe".to_string(),
            "--autostart=true".to_string(),
        ]));
        assert!(!is_autostart_arg_launch(&[
            "Rayburst.exe".to_string(),
            "--flag".to_string(),
        ]));
    }

    #[test]
    fn parses_notification_open_folder_action_url() {
        let secret = "test-secret";
        let url = signed_open_folder_url("C:\\Downloads", secret);

        assert_eq!(
            notification_open_target_from_url(&url, secret),
            Some(TaskNotificationOpenTarget {
                dir: "C:\\Downloads".to_string(),
            })
        );
        assert!(notification_open_target_from_url(&url, "wrong-secret").is_none());
        assert!(notification_open_target_from_url(
            "rayburst://open-folder?dir=C%3A%5CDownloads",
            secret
        )
        .is_none());
        assert!(notification_open_target_from_url(
            "rayburst://open-folder?dir=C%3A%5CDownloads&dir=D%3A%5CMedia&sig=00",
            secret
        )
        .is_none());
        assert!(notification_open_target_from_url(
            "rayburst://new?url=https%3A%2F%2Fexample.com",
            secret
        )
        .is_none());
    }

    #[test]
    fn detects_notification_show_task_list_action_url() {
        assert!(is_notification_show_task_list_url(
            "rayburst://show-task-list"
        ));
        assert!(is_notification_show_task_list_url(
            "rayburst:/show-task-list"
        ));
        assert!(!is_notification_show_task_list_url(
            "rayburst://open-folder?dir=C%3A%5CDownloads"
        ));
        assert!(!is_notification_show_task_list_url(
            "https://example.com/show-task-list"
        ));
    }

    #[test]
    fn parses_notification_task_actions_with_strict_gid_validation() {
        assert_eq!(
            notification_task_action_from_url(
                "rayburst://task-action?action=open-file&gid=0123456789abcdef"
            )
            .map(|(_, gid)| gid),
            Some("0123456789abcdef".to_string())
        );
        assert!(notification_task_action_from_url(
            "rayburst://task-action?action=show-in-folder&gid=g-1"
        )
        .is_some());
        assert!(notification_task_action_from_url(
            "rayburst://task-action?action=open-file&gid=bad%20gid"
        )
        .is_none());
        assert!(notification_task_action_from_url(
            "rayburst://task-action?action=open-file&gid=g1&gid=g2"
        )
        .is_none());
        assert!(
            notification_task_action_from_url("rayburst://task-action?action=unknown&gid=g1")
                .is_none()
        );
    }

    #[test]
    fn parses_notification_task_actions_from_path_urls_and_windows_normalization() {
        assert!(notification_task_action_from_url(
            "rayburst://task-action/open-file/0123456789abcdef"
        )
        .is_some());
        assert!(
            notification_task_action_from_url("rayburst:/task-action/show-in-folder/g-1").is_some()
        );
        assert!(notification_task_action_from_url(
            "rayburst://task-action/?action=open-file&gid=g1"
        )
        .is_some());
        assert!(notification_task_action_from_url(
            "rayburst:/task-action/?action=show-in-folder&gid=g1"
        )
        .is_some());
        assert!(
            notification_task_action_from_url("rayburst://task-action/open-file/g1?extra=1")
                .is_none()
        );
        assert!(
            notification_task_action_from_url("rayburst://task-action/open-file/g1/").is_none()
        );
        assert!(
            notification_task_action_from_url("rayburst://task-action/open-file/g1#fragment")
                .is_none()
        );
    }

    #[test]
    fn detects_activate_action_without_query_parameters() {
        assert!(is_notification_activate_url("rayburst://activate"));
        assert!(is_notification_activate_url("rayburst:/activate"));
        assert!(!is_notification_activate_url("rayburst://activate?x=1"));
    }

    #[test]
    fn pending_queue_deduplicates_urls_while_preserving_order() {
        let mut queue = vec!["file:///Users/example/ubuntu.torrent".to_string()];
        let added = append_unique_pending(
            &mut queue,
            &[
                "file:///Users/example/ubuntu.torrent".to_string(),
                "magnet:?xt=urn:btih:abc".to_string(),
                "magnet:?xt=urn:btih:abc".to_string(),
                "/Users/example/Fedora.torrent".to_string(),
            ],
        );

        assert_eq!(added, 2);
        assert_eq!(
            queue,
            vec![
                "file:///Users/example/ubuntu.torrent".to_string(),
                "magnet:?xt=urn:btih:abc".to_string(),
                "/Users/example/Fedora.torrent".to_string(),
            ]
        );
    }

    #[test]
    fn take_pending_deep_links_drains_queue_once() {
        let state = PendingDeepLinkState::new();
        {
            let mut inner = state.0.lock().expect("pending deep-link state poisoned");
            append_unique_pending(
                &mut inner.queue,
                &[
                    "file:///Users/example/ubuntu.torrent".to_string(),
                    "magnet:?xt=urn:btih:abc".to_string(),
                ],
            );
        }

        let first = take_pending_deep_links(&state);
        assert_eq!(
            first.urls,
            vec![
                "file:///Users/example/ubuntu.torrent".to_string(),
                "magnet:?xt=urn:btih:abc".to_string(),
            ]
        );
        assert!(!first.silent);
        let second = take_pending_deep_links(&state);
        assert!(second.urls.is_empty());
        assert!(!second.silent);
    }

    #[test]
    fn take_pending_deep_links_marks_frontend_ready() {
        let state = PendingDeepLinkState::new();
        assert!(!state.frontend_ready());

        let _ = take_pending_deep_links(&state);

        assert!(state.frontend_ready());
    }

    #[test]
    fn frontend_ready_flag_can_be_cleared_after_webview_destruction() {
        let state = PendingDeepLinkState::new();
        let _ = take_pending_deep_links(&state);
        assert!(state.frontend_ready());

        state.set_frontend_ready(false);

        assert!(!state.frontend_ready());
    }

    #[test]
    fn take_pending_deep_links_preserves_silent_intent() {
        let state = PendingDeepLinkState::new();
        {
            let mut inner = state.0.lock().expect("pending deep-link state poisoned");
            append_unique_pending(
                &mut inner.queue,
                &["https://example.com/file.zip".to_string()],
            );
            inner.silent = true;
        }

        let payload = take_pending_deep_links(&state);

        assert_eq!(
            payload.urls,
            vec!["https://example.com/file.zip".to_string()]
        );
        assert!(payload.silent);
        assert!(!take_pending_deep_links(&state).silent);
    }
}
