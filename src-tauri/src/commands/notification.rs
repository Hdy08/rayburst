use crate::error::AppError;
use crate::services::config::{RuntimeConfig, RuntimeConfigState};
use crate::services::notification::{
    send_app_notification, send_task_start_notification as send_task_start_notification_for_tasks,
    TaskStartNotificationTask,
};
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn send_task_start_notification(
    app: AppHandle,
    task_names: Vec<String>,
    tasks: Option<Vec<TaskStartNotificationTask>>,
) -> Result<bool, AppError> {
    let config = match app.try_state::<RuntimeConfigState>() {
        Some(state) => state.snapshot().await,
        None => {
            log::warn!("notification:runtime-config-unavailable fallback=defaults");
            RuntimeConfig::default()
        }
    };

    send_task_start_notification_for_tasks(
        &app,
        &task_names,
        tasks.as_deref().unwrap_or_default(),
        &config,
    )
}

#[tauri::command]
pub async fn send_app_system_notification(
    app: AppHandle,
    title: String,
    body: String,
) -> Result<(), AppError> {
    send_app_notification(&app, &title, &body)
}
