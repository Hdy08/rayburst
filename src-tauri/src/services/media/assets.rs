//! Bounded, resumable browser capture ingress. Only the native engine consumes sealed files.
use super::{error::Error, routes::authorize};
use crate::services::http_api::ApiContext;
use axum::{
    body::Bytes,
    extract::{Path, Request, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};
use tauri::Manager;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use uuid::Uuid;

static WRITER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const MAX_STREAM: u8 = 31;
const MAX_FILE: u64 = 64 * 1024 * 1024 * 1024;

fn directory(ctx: &ApiContext, id: Uuid) -> Result<PathBuf, Error> {
    Ok(ctx
        .app
        .path()
        .app_local_data_dir()
        .map_err(|_| Error::Unavailable)?
        .join("media-captures")
        .join(id.to_string()))
}
pub async fn create(
    State(ctx): State<Arc<ApiContext>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, Error> {
    authorize(&ctx, &headers)?;
    super::service(&ctx.app).await?.capabilities().await?;
    let path = directory(&ctx, id)?;
    tokio::fs::create_dir_all(path)
        .await
        .map_err(|_| Error::Unavailable)?;
    Ok(Json(json!({"id":id})))
}
pub async fn append(
    State(ctx): State<Arc<ApiContext>>,
    headers: HeaderMap,
    Path((id, stream)): Path<(Uuid, u8)>,
    bytes: Bytes,
) -> Result<Json<Value>, Error> {
    authorize(&ctx, &headers)?;
    if stream > MAX_STREAM || bytes.is_empty() || bytes.len() > 1024 * 1024 {
        return Err(Error::UnsupportedSource);
    }
    let offset: u64 = headers
        .get("x-upload-offset")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .ok_or(Error::UnsupportedSource)?;
    let end = offset
        .checked_add(bytes.len() as u64)
        .filter(|v| *v <= MAX_FILE)
        .ok_or(Error::UnsupportedSource)?;
    let _guard = WRITER.lock().await;
    let path = directory(&ctx, id)?;
    if !path.is_dir() || path.join("sealed").exists() {
        return Err(Error::Conflict);
    }
    append_file(&path.join(stream.to_string()), offset, &bytes).await?;
    Ok(Json(json!({"offset":end})))
}
async fn append_file(path: &std::path::Path, offset: u64, bytes: &[u8]) -> Result<(), Error> {
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .await
        .map_err(|_| Error::Unavailable)?;
    let length = file.metadata().await.map_err(|_| Error::Unavailable)?.len();
    if length < offset {
        return Err(Error::Conflict);
    }
    file.seek(std::io::SeekFrom::Start(offset))
        .await
        .map_err(|_| Error::Unavailable)?;
    let prefix = (length - offset).min(bytes.len() as u64) as usize;
    if prefix > 0 {
        let mut saved = vec![0; prefix];
        file.read_exact(&mut saved)
            .await
            .map_err(|_| Error::Unavailable)?;
        if saved != bytes[..prefix] {
            return Err(Error::Conflict);
        }
    }
    if prefix < bytes.len() {
        file.write_all(&bytes[prefix..])
            .await
            .map_err(|_| Error::Unavailable)?;
    }
    file.sync_data().await.map_err(|_| Error::Unavailable)?;
    Ok(())
}
pub async fn seal(
    State(ctx): State<Arc<ApiContext>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, Error> {
    authorize(&ctx, &headers)?;
    let _guard = WRITER.lock().await;
    let path = directory(&ctx, id)?;
    let mut streams = Vec::new();
    for stream in 0..=MAX_STREAM {
        if let Ok(metadata) = tokio::fs::metadata(path.join(stream.to_string())).await {
            if metadata.len() > 0 {
                streams.push(json!({"stream":stream,"size":metadata.len()}));
            }
        }
    }
    if streams.is_empty() {
        return Err(Error::UnsupportedSource);
    }
    tokio::fs::write(path.join("sealed"), b"sealed")
        .await
        .map_err(|_| Error::Unavailable)?;
    Ok(Json(json!({"id":id,"streams":streams})))
}
pub async fn read(
    State(ctx): State<Arc<ApiContext>>,
    Path((id, stream)): Path<(Uuid, u8)>,
    request: Request,
) -> Result<Response, Error> {
    authorize(&ctx, request.headers())?;
    let path = directory(&ctx, id)?;
    if stream > MAX_STREAM || !path.join("sealed").exists() {
        return Err(Error::NotFound);
    }
    let response = tower_http::services::ServeFile::new(path.join(stream.to_string()))
        .try_call(request)
        .await
        .map_err(|_| Error::Unavailable)?;
    Ok(response.into_response())
}
pub async fn discard(
    State(ctx): State<Arc<ApiContext>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, Error> {
    authorize(&ctx, &headers)?;
    let _guard = WRITER.lock().await;
    let path = directory(&ctx, id)?;
    // Sealed assets may belong to a queued or paused engine task.
    if path.join("sealed").exists() {
        return Err(Error::Conflict);
    }
    if path.exists() {
        tokio::fs::remove_dir_all(path)
            .await
            .map_err(|_| Error::Unavailable)?;
    }
    Ok(Json(json!({"id":id})))
}

pub(super) fn capture_id(value: &str) -> Option<Uuid> {
    let url = url::Url::parse(value).ok()?;
    if url.scheme() != "http" || !matches!(url.host_str(), Some("127.0.0.1" | "localhost")) {
        return None;
    }
    let parts: Vec<_> = url.path_segments()?.collect();
    if parts.len() != 5 || parts[..3] != ["media", "v2", "assets"] {
        return None;
    }
    parts[3].parse().ok()
}
pub(super) async fn cleanup(
    app: &tauri::AppHandle,
    protected: &std::collections::HashSet<Uuid>,
) -> Result<(), Error> {
    let _guard = WRITER.lock().await;
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| Error::Unavailable)?
        .join("media-captures");
    if !root.is_dir() {
        return Ok(());
    }
    let mut entries = tokio::fs::read_dir(&root)
        .await
        .map_err(|_| Error::Unavailable)?;
    while let Some(entry) = entries.next_entry().await.map_err(|_| Error::Unavailable)? {
        let Some(id) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<Uuid>().ok())
        else {
            continue;
        };
        if protected.contains(&id)
            || entry
                .file_type()
                .await
                .map_err(|_| Error::Unavailable)?
                .is_symlink()
        {
            continue;
        }
        let path = root.join(id.to_string());
        if !path.is_dir() {
            continue;
        }
        let mut newest = entry
            .metadata()
            .await
            .map_err(|_| Error::Unavailable)?
            .modified()
            .map_err(|_| Error::Unavailable)?;
        let mut files = tokio::fs::read_dir(&path)
            .await
            .map_err(|_| Error::Unavailable)?;
        while let Some(file) = files.next_entry().await.map_err(|_| Error::Unavailable)? {
            if let Ok(time) = file
                .metadata()
                .await
                .and_then(|metadata| metadata.modified())
            {
                newest = newest.max(time);
            }
        }
        if newest.elapsed().is_ok_and(|age| age.as_secs() > 86_400) {
            tokio::fs::remove_dir_all(path)
                .await
                .map_err(|_| Error::Unavailable)?;
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn capture_replays_resume_partial_writes_without_duplication() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("stream");
        append_file(&path, 0, b"first").await.expect("first chunk");
        append_file(&path, 0, b"first")
            .await
            .expect("replayed chunk");
        tokio::fs::write(&path, b"firstse")
            .await
            .expect("interrupted second chunk");
        append_file(&path, 5, b"second")
            .await
            .expect("resume second chunk");
        assert_eq!(
            tokio::fs::read(&path).await.expect("saved capture"),
            b"firstsecond"
        );
        assert!(matches!(
            append_file(&path, 5, b"different").await,
            Err(Error::Conflict)
        ));
        assert!(matches!(
            append_file(&path, 99, b"gap").await,
            Err(Error::Conflict)
        ));
        assert_eq!(
            tokio::fs::read(&path).await.expect("unchanged capture"),
            b"firstsecond"
        );
    }
}
