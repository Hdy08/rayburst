use tauri::async_runtime::Receiver;
use tauri::Manager;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

use super::cleanup::cleanup_port;
use super::config::{generate_runtime_config, runtime_config_args, ManagedEngineConfig};
use super::state::{path_to_safe_string, strip_ansi, EngineState};
use super::{valid_aria2_log_level, DEFAULT_ARIA2_LOG_LEVEL};
use crate::services::port_guard;
use tauri_plugin_store::StoreExt;

const ENGINE_SIDECAR_NAME: &str = "aria2-next";
const DEFAULT_RPC_PORT_STR: &str = "29100";
const ENGINE_PORT_RELEASE_TIMEOUT_MS: u64 = 2600;
const ENGINE_PORT_RELEASE_POLL_MS: u64 = 100;
const WINDOWS_DBG_TERMINATE_PROCESS: u32 = 0x4001_0004;
const WINDOWS_STATUS_DLL_INIT_FAILED_LOGOFF: u32 = 0xC000_026B;
const PROXY_ENV_VARS: &[&str] = &[
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
];

fn sanitized_engine_proxy_env() -> Vec<(&'static str, &'static str)> {
    PROXY_ENV_VARS.iter().map(|key| (*key, "")).collect()
}

fn read_aria2_log_level(app: &tauri::AppHandle) -> String {
    let Some(store) = app.store("config.json").ok() else {
        return DEFAULT_ARIA2_LOG_LEVEL.to_string();
    };
    let Some(level) = store
        .get("preferences")
        .and_then(|p| p.get("aria2LogLevel")?.as_str().map(ToString::to_string))
    else {
        return DEFAULT_ARIA2_LOG_LEVEL.to_string();
    };
    if valid_aria2_log_level(&level) {
        level
    } else {
        DEFAULT_ARIA2_LOG_LEVEL.to_string()
    }
}

fn engine_log_config(app: &tauri::AppHandle) -> Result<(String, String), String> {
    let log_path = app
        .path()
        .app_log_dir()
        .map_err(|e| format!("Failed to get app log dir: {e}"))?
        .join(crate::log_policy::ARIA2_LOG_FILE);
    let log_path = path_to_safe_string(&log_path);
    let log_level = read_aria2_log_level(app);
    Ok((log_path, log_level))
}

fn ensure_download_session(path: &std::path::Path) -> Result<(), String> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(format!(
            "Failed to create download session '{}': {error}",
            path.display()
        )),
    }
}

#[cfg(not(windows))]
fn kill_process_by_pid(pid: u32) -> Result<(), String> {
    let status = std::process::Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .map_err(|e| format!("Failed to execute kill for PID {pid}: {e}"))?;
    if status.success() {
        return Ok(());
    }
    Err(format!("kill failed for PID {pid}: {status}"))
}

fn is_windows_session_shutdown_exit(exit_code: i32) -> bool {
    matches!(
        exit_code as u32,
        WINDOWS_DBG_TERMINATE_PROCESS | WINDOWS_STATUS_DLL_INIT_FAILED_LOGOFF
    )
}

fn is_system_shutdown_exit(exit_code: i32) -> bool {
    cfg!(windows) && is_windows_session_shutdown_exit(exit_code)
}

fn process_spawn_allowed(app: &tauri::AppHandle) -> bool {
    app.try_state::<crate::engine::supervisor::EngineSupervisor>()
        .is_some_and(|supervisor| supervisor.allows_process_spawn())
}

pub(crate) fn wait_for_engine_ports_release(app: &tauri::AppHandle) {
    match port_guard::wait_for_engine_ports_available(
        app,
        std::time::Duration::from_millis(ENGINE_PORT_RELEASE_TIMEOUT_MS),
        std::time::Duration::from_millis(ENGINE_PORT_RELEASE_POLL_MS),
    ) {
        Ok(true) => {}
        Ok(false) => {
            log::warn!(
                "restart: engine ports still occupied after {}ms, running conflict recovery",
                ENGINE_PORT_RELEASE_TIMEOUT_MS
            );
        }
        Err(e) => {
            log::warn!("restart: failed to wait for engine port release: {e}");
        }
    }
}

fn prepare_engine_args(
    app: &tauri::AppHandle,
    config: &serde_json::Value,
) -> Result<Vec<String>, String> {
    if let Some(directory) = config.get("dir").and_then(serde_json::Value::as_str) {
        std::fs::create_dir_all(directory).map_err(|error| {
            format!("Failed to create download directory '{directory}': {error}")
        })?;
    }

    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to get app data dir: {error}"))?;
    std::fs::create_dir_all(&data_dir)
        .map_err(|error| format!("Failed to create app data directory: {error}"))?;
    let session_path = data_dir.join("download.session");
    ensure_download_session(&session_path)?;
    let session_path_string = path_to_safe_string(&session_path);
    let state_dir = data_dir.join("engine").join("state");
    std::fs::create_dir_all(&state_dir)
        .map_err(|error| format!("Failed to create engine state directory: {error}"))?;
    let state_dir_string = path_to_safe_string(&state_dir);

    let (log_file_path, log_level) = engine_log_config(app)?;
    let ed2k_bootstrap = crate::commands::ed2k::ensure_ed2k_bootstrap_cache(app)
        .map_err(|error| format!("Failed to prepare ED2K bootstrap cache: {error}"))?;
    let bt_peer_blocklist = crate::commands::bt_blocklist::startup_blocklist_path(app)
        .map_err(|error| format!("Failed to prepare BT peer blocklist: {error}"))?;
    let runtime_config = generate_runtime_config(
        app,
        config,
        ManagedEngineConfig {
            session_path: &session_path_string,
            state_dir: &state_dir_string,
            log_file_path: &log_file_path,
            log_level: &log_level,
            ed2k_server_list: &ed2k_bootstrap.0,
            ed2k_node_list: &ed2k_bootstrap.1,
            bt_peer_blocklist: bt_peer_blocklist.as_deref(),
        },
    )?;
    Ok(runtime_config_args(&runtime_config))
}

fn spawn_engine(
    app: &tauri::AppHandle,
    args: &[String],
) -> Result<(Receiver<CommandEvent>, CommandChild), String> {
    log::info!(target: "engine", event = "engine_spawn_started", argument_count = args.len(); "engine_spawn_started");
    app.shell()
        .sidecar(ENGINE_SIDECAR_NAME)
        .map_err(|error| format!("Failed to create sidecar: {error}"))?
        .envs(sanitized_engine_proxy_env())
        .args(args)
        .spawn()
        .map_err(|error| format!("Failed to spawn Aria2 Next: {error}"))
}

fn monitor_engine(
    app: tauri::AppHandle,
    mut receiver: Receiver<CommandEvent>,
    process_id: u32,
    generation: u32,
) {
    tauri::async_runtime::spawn(async move {
        while let Some(event) = receiver.recv().await {
            match event {
                CommandEvent::Stdout(line) => {
                    let text = strip_ansi(&String::from_utf8_lossy(&line));
                    #[cfg(debug_assertions)]
                    if !text.trim().is_empty() {
                        anstream::println!(
                            "{}",
                            crate::log_policy::format_engine_terminal_record(&text)
                        );
                    }
                    if let Some(kind) = port_guard::aria2_runtime_bind_error_kind(&text) {
                        crate::engine::supervisor::report_port_conflict(
                            app.clone(),
                            generation,
                            kind,
                        );
                    }
                }
                CommandEvent::Stderr(line) => {
                    let text = String::from_utf8_lossy(&line);
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        log::warn!("engine stderr: {trimmed}");
                        if let Some(state) = app.try_state::<EngineState>() {
                            state.push_stderr(trimmed.to_string());
                        }
                    }
                }
                CommandEvent::Terminated(payload) => {
                    let exit_code = payload.code.unwrap_or(-1);
                    let is_stale = app
                        .try_state::<EngineState>()
                        .is_none_or(|state| !state.is_current_generation(generation));
                    if is_stale {
                        log::debug!(
                            target: "engine",
                            event = "stale_engine_terminated",
                            pid = process_id,
                            generation,
                            exit_code;
                            "stale_engine_terminated"
                        );
                        break;
                    }

                    let supervisor_expected = app
                        .try_state::<crate::engine::supervisor::EngineSupervisor>()
                        .is_none_or(|supervisor| supervisor.is_process_exit_expected());
                    let shutdown_exit = is_system_shutdown_exit(exit_code);
                    let expected = supervisor_expected || shutdown_exit;
                    if expected {
                        log::info!(
                            target: "engine",
                            event = "engine_stopped",
                            pid = process_id,
                            generation,
                            exit_code,
                            exit_code_unsigned = exit_code as u32,
                            reason = if shutdown_exit { "windows-session-end" } else { "expected" },
                            signal:? = payload.signal;
                            "engine_stopped"
                        );
                    } else {
                        log::warn!(
                            target: "engine",
                            event = "engine_crashed",
                            pid = process_id,
                            generation,
                            exit_code,
                            signal:? = payload.signal;
                            "engine_crashed"
                        );
                    }

                    if let Some(state) = app.try_state::<EngineState>() {
                        if let Ok(mut child) = state.child.lock() {
                            if child
                                .as_ref()
                                .map(tauri_plugin_shell::process::CommandChild::pid)
                                == Some(process_id)
                            {
                                *child = None;
                            }
                        }
                    }
                    if !expected {
                        crate::engine::supervisor::report_process_exit(
                            app.clone(),
                            generation,
                            exit_code,
                            payload.signal,
                        );
                    }
                    break;
                }
                _ => {}
            }
        }
    });
}

/// Spawns Aria2 Next from the current persisted system configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartEngineOutcome {
    Started,
    Cancelled,
}

pub fn start_engine(app: &tauri::AppHandle) -> Result<StartEngineOutcome, crate::error::AppError> {
    use crate::error::AppError;
    if !process_spawn_allowed(app) {
        return Ok(StartEngineOutcome::Cancelled);
    }

    let state = app.state::<EngineState>();
    let mut child_lock = state
        .child
        .lock()
        .map_err(|e| AppError::Engine(e.to_string()))?;

    if child_lock.is_some() {
        return Ok(StartEngineOutcome::Started);
    }

    let mut config = crate::commands::config::read_engine_config_snapshot(app.clone())?;
    if let Some(options) = config.as_object_mut() {
        crate::proxy_bypass::normalize_options(options)?;
    }

    // Kill any leftover supported engine process on the RPC port before starting
    let port = config
        .get("rpc-listen-port")
        .and_then(|v| v.as_str())
        .unwrap_or(DEFAULT_RPC_PORT_STR);
    cleanup_port(port);

    let args = prepare_engine_args(app, &config).map_err(AppError::Engine)?;
    if !process_spawn_allowed(app) {
        return Ok(StartEngineOutcome::Cancelled);
    }
    let (receiver, child) = spawn_engine(app, &args).map_err(AppError::Engine)?;

    log::info!(target: "engine", event = "engine_started", pid = child.pid(); "engine_started");

    let spawned_pid = child.pid();
    *child_lock = Some(child);
    state.clear_stderr();
    let my_gen = state.next_generation();
    monitor_engine(app.clone(), receiver, spawned_pid, my_gen);

    Ok(StartEngineOutcome::Started)
}

/// Stop the owned sidecar. Parent-process monitoring also covers abrupt app exit.
pub fn stop_engine(app: &tauri::AppHandle, for_exit: bool) -> Result<(), String> {
    let state = app.state::<EngineState>();
    state.invalidate_generation();
    let mut child_lock = state.child.lock().map_err(|e| e.to_string())?;

    #[cfg(windows)]
    {
        let _ = for_exit;
        if let Some(pid) = child_lock.as_ref().map(CommandChild::pid) {
            super::windows_process::stop_owned_engine(pid)?;
            *child_lock = None;
            log::info!("stopped engine process: PID {pid}");
        }
    }
    #[cfg(not(windows))]
    if let Some(child) = child_lock.take() {
        let pid = child.pid();
        if for_exit {
            child
                .kill()
                .map_err(|error| format!("Failed to stop engine PID {pid}: {error}"))?;
        } else {
            kill_process_by_pid(pid)?;
        }
        log::info!("stopped engine process: PID {pid}");
    }

    Ok(())
}

pub fn wait_for_engine_exit(app: &tauri::AppHandle, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let stopped = app
            .try_state::<EngineState>()
            .and_then(|state| state.child.lock().ok().map(|child| child.is_none()))
            .unwrap_or(true);
        if stopped {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_session_shutdown_codes_are_classified_by_unsigned_value() {
        assert!(is_windows_session_shutdown_exit(
            WINDOWS_DBG_TERMINATE_PROCESS as i32
        ));
        assert!(is_windows_session_shutdown_exit(
            WINDOWS_STATUS_DLL_INIT_FAILED_LOGOFF as i32
        ));
        assert!(!is_windows_session_shutdown_exit(0));
        assert!(!is_windows_session_shutdown_exit(1));
        assert!(!is_windows_session_shutdown_exit(0xC000_0142_u32 as i32));
    }

    #[test]
    fn ensure_download_session_creates_an_empty_file_without_overwriting_existing_state() {
        let temp = tempfile::tempdir().unwrap();
        let session = temp.path().join("download.session");

        ensure_download_session(&session).unwrap();
        assert_eq!(std::fs::read(&session).unwrap(), b"");

        std::fs::write(&session, "existing session").unwrap();
        ensure_download_session(&session).unwrap();
        assert_eq!(
            std::fs::read_to_string(&session).unwrap(),
            "existing session"
        );
    }

    #[test]
    fn sanitized_engine_proxy_env_clears_lowercase_and_uppercase_proxy_vars() {
        let env = sanitized_engine_proxy_env();

        for key in PROXY_ENV_VARS {
            assert!(env
                .iter()
                .any(|(name, value)| name == key && value.is_empty()));
        }
    }
}
