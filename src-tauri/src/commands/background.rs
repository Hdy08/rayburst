use crate::error::AppError;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

const MAX_LOCAL_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_LOCAL_IMAGE_PIXELS: u64 = 32_000_000;
const LOCAL_IMAGE_CACHE_DIRECTORY: &str = "rayburst-background";
static LOCAL_IMAGE_CACHE_LOCK: Mutex<()> = Mutex::new(());

fn local_image_format(extension: &str) -> Option<image::ImageFormat> {
    match extension {
        "png" => Some(image::ImageFormat::Png),
        "jpg" | "jpeg" => Some(image::ImageFormat::Jpeg),
        "webp" => Some(image::ImageFormat::WebP),
        "bmp" => Some(image::ImageFormat::Bmp),
        "gif" => Some(image::ImageFormat::Gif),
        _ => None,
    }
}

fn read_validated_local_image(path: &Path) -> Result<(Vec<u8>, String), AppError> {
    let extension = path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| AppError::Io("Unsupported background image format".into()))?;
    let image_format = local_image_format(&extension)
        .ok_or_else(|| AppError::Io("Unsupported background image format".into()))?;
    let canonical_path = dunce::canonicalize(path)
        .map_err(|error| AppError::Io(format!("Failed to resolve background image: {error}")))?;
    let mut file = std::fs::File::open(&canonical_path)
        .map_err(|error| AppError::Io(format!("Failed to open background image: {error}")))?;
    let metadata = file
        .metadata()
        .map_err(|error| AppError::Io(format!("Failed to inspect background image: {error}")))?;
    if !metadata.is_file() || metadata.len() > MAX_LOCAL_IMAGE_BYTES {
        return Err(AppError::Io(
            "Background image exceeds the supported file limit".into(),
        ));
    }

    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(MAX_LOCAL_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| AppError::Io(format!("Failed to read background image: {error}")))?;
    if bytes.len() as u64 > MAX_LOCAL_IMAGE_BYTES {
        return Err(AppError::Io(
            "Background image exceeds the supported file limit".into(),
        ));
    }
    let (width, height) =
        image::ImageReader::with_format(std::io::Cursor::new(&bytes), image_format)
            .into_dimensions()
            .map_err(|error| {
                AppError::Io(format!(
                    "Failed to inspect background image dimensions: {error}"
                ))
            })?;
    let pixels = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 || pixels > MAX_LOCAL_IMAGE_PIXELS {
        return Err(AppError::Io(
            "Background image exceeds the supported pixel limit".into(),
        ));
    }
    Ok((bytes, extension))
}

fn remove_stale_cache_entries(cache_dir: &Path, current_path: &Path) {
    let Ok(entries) = std::fs::read_dir(cache_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path != current_path && path.is_file() {
            if let Err(error) = std::fs::remove_file(path) {
                log::debug!("background: stale cache removal failed: {error}");
            }
        }
    }
}

fn cache_local_image(cache_dir: &Path, bytes: &[u8], extension: &str) -> Result<PathBuf, AppError> {
    std::fs::create_dir_all(cache_dir)
        .map_err(|error| AppError::Io(format!("Failed to create background cache: {error}")))?;
    let mut file = tempfile::Builder::new()
        .prefix("background-")
        .suffix(&format!(".{extension}"))
        .tempfile_in(cache_dir)
        .map_err(|error| AppError::Io(format!("Failed to create cached background: {error}")))?;
    file.write_all(bytes)
        .map_err(|error| AppError::Io(format!("Failed to cache background: {error}")))?;
    file.as_file()
        .sync_all()
        .map_err(|error| AppError::Io(format!("Failed to finalize background cache: {error}")))?;
    let (_, path) = file
        .keep()
        .map_err(|error| AppError::Io(format!("Failed to retain cached background: {error}")))?;
    remove_stale_cache_entries(cache_dir, &path);
    Ok(path)
}

#[tauri::command]
pub fn prepare_local_background(app: AppHandle, path: String) -> Result<String, AppError> {
    let _guard = LOCAL_IMAGE_CACHE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (bytes, extension) = read_validated_local_image(Path::new(&path))?;
    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| AppError::Io(format!("Failed to resolve background cache: {error}")))?
        .join(LOCAL_IMAGE_CACHE_DIRECTORY);
    let cached_path = cache_local_image(&cache_dir, &bytes, &extension)?;
    cached_path
        .into_os_string()
        .into_string()
        .map_err(|_| AppError::Io("Cached background path is not valid UTF-8".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_oversized_and_malformed_images() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let oversized = directory.path().join("oversized.png");
        let file = std::fs::File::create(&oversized).expect("image file");
        file.set_len(MAX_LOCAL_IMAGE_BYTES + 1)
            .expect("resize fixture");
        assert!(read_validated_local_image(&oversized).is_err());

        let malformed = directory.path().join("malformed.png");
        std::fs::write(&malformed, b"not an image").expect("malformed fixture");
        assert!(read_validated_local_image(&malformed).is_err());
    }

    #[test]
    fn accepts_common_wallpaper_dimensions() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let image_path = directory.path().join("background.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255]))
            .save_with_format(&image_path, image::ImageFormat::Png)
            .expect("save fixture");
        let (bytes, extension) = read_validated_local_image(&image_path).expect("valid image");
        assert!(!bytes.is_empty());
        assert_eq!(extension, "png");
    }
}
