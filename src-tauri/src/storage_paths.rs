use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const PORTABLE_MARKER: &str = "portable.flag";
const PORTABLE_DATA_DIR: &str = "data";
const PORTABLE_CONFIG_DIR: &str = "config";

fn portable_root_from_exe(exe_path: &Path) -> Option<PathBuf> {
    let root = exe_path.parent()?;
    if root.join(PORTABLE_MARKER).is_file() {
        Some(root.to_path_buf())
    } else {
        None
    }
}


pub fn portable_root() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|path| portable_root_from_exe(&path))
}


pub fn is_portable_mode() -> bool {
    portable_root().is_some()
}


pub fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(root) = portable_root() {
        Ok(root.join(PORTABLE_DATA_DIR))
    } else {
        app.path().app_data_dir().map_err(|e| format!("Gagal menentukan folder data aplikasi: {e}"))
    }
}


pub fn app_config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(root) = portable_root() {
        Ok(root.join(PORTABLE_CONFIG_DIR))
    } else {
        app.path().app_config_dir().map_err(|e| format!("Gagal menentukan folder konfigurasi aplikasi: {e}"))
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_root_requires_marker_file() {
        let temp = std::env::temp_dir().join(format!("flip-portable-path-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        let exe = temp.join("FlipV2.exe");
        std::fs::write(&exe, b"test").unwrap();
        assert!(portable_root_from_exe(&exe).is_none());
        std::fs::write(temp.join(PORTABLE_MARKER), b"").unwrap();
        assert_eq!(portable_root_from_exe(&exe), Some(temp.clone()));
        let _ = std::fs::remove_dir_all(&temp);
    }
}
