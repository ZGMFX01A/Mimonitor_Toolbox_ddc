//! Embedded companion tools: users only need the application EXE.
use parking_lot::Mutex;
use std::{fs, io, path::{Path, PathBuf}};

include!(concat!(env!("OUT_DIR"), "/licenses.rs"));

const RUNTIME_FILES: &[(&str, &[u8])] = &[
    ("adb.exe", include_bytes!("../assets/runtime/adb.exe")),
    ("AdbWinApi.dll", include_bytes!("../assets/runtime/AdbWinApi.dll")),
    ("AdbWinUsbApi.dll", include_bytes!("../assets/runtime/AdbWinUsbApi.dll")),
    ("MtkDirectTool.jar", include_bytes!("../assets/runtime/MtkDirectTool.jar")),
    ("ColorfulLedTool.jar", include_bytes!("../assets/runtime/ColorfulLedTool.jar")),
    ("adbguardian-signed.apk", include_bytes!("../assets/adb_guardian/adbguardian-signed.apk")),
];

fn materialize(directory: &Path, files: &[(&str, &[u8])]) -> io::Result<()> {
    for (name, bytes) in files {
        let path = directory.join(name);
        if fs::read(&path).ok().as_deref() == Some(*bytes) { continue; }
        fs::create_dir_all(path.parent().unwrap())?;
        let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        if let Err(error) = fs::write(&temporary, bytes) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        if let Err(error) = fs::rename(&temporary, &path) {
            let _ = fs::remove_file(&temporary);
            // A concurrent process may have deployed the identical embedded file.
            if fs::read(&path).ok().as_deref() != Some(*bytes) { return Err(error); }
        }
    }
    Ok(())
}

fn deployed_file(
    directory: &Path,
    files: &[(&str, &[u8])],
    name: &str,
    deployed: &Mutex<Option<PathBuf>>,
) -> io::Result<PathBuf> {
    let path = directory.join(name);
    let mut previous = deployed.lock();
    if previous.as_deref() != Some(directory) || !path.is_file() {
        materialize(directory, files)?;
        // Do not cache failures: antivirus/file locks and disk errors can be transient.
        *previous = Some(directory.to_path_buf());
    }
    Ok(path)
}

pub fn runtime_file(name: &str) -> Result<PathBuf, String> {
    if !RUNTIME_FILES.iter().any(|(file, _)| *file == name) {
        return Err(format!("未知内置资源：{name}"));
    }
    let directory = crate::config::get_app_data_dir().join("runtime").join(env!("CARGO_PKG_VERSION"));
    static DEPLOY: Mutex<Option<PathBuf>> = Mutex::new(None);
    deployed_file(&directory, RUNTIME_FILES, name, &DEPLOY).map_err(|e| e.to_string())
}

pub fn helper_path(name: &str) -> Option<PathBuf> {
    runtime_file(name).map_err(|error| crate::logger::log_info(&format!("内置资源部署失败：{error}"))).ok()
}

pub fn license_directory() -> Result<PathBuf, String> {
    let directory = crate::config::get_app_data_dir().join("notices").join(env!("CARGO_PKG_VERSION"));
    // Recheck on demand so users can restore notices after deleting the cache.
    materialize(&directory, LICENSE_FILES).map_err(|e| e.to_string())?;
    Ok(directory)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_deployment_can_retry_and_deleted_payloads_are_restored() {
        let directory = std::env::temp_dir().join(format!("mimonitor-retry-{}", uuid::Uuid::new_v4()));
        let state = Mutex::new(None);
        let files: &[(&str, &[u8])] = &[("tool.bin", b"embedded")];
        fs::write(&directory, b"temporary obstacle").unwrap();
        assert!(deployed_file(&directory, files, "tool.bin", &state).is_err());
        fs::remove_file(&directory).unwrap();
        let path = deployed_file(&directory, files, "tool.bin", &state).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"embedded");
        fs::remove_file(&path).unwrap();
        deployed_file(&directory, files, "tool.bin", &state).unwrap();
        assert_eq!(fs::read(path).unwrap(), b"embedded");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn embedded_bundle_needs_no_adjacent_files_and_repairs_stale_payloads() {
        let directory = std::env::temp_dir().join(format!("mimonitor-bundle-{}", uuid::Uuid::new_v4()));
        materialize(&directory, RUNTIME_FILES).unwrap();
        materialize(&directory, LICENSE_FILES).unwrap();
        for (name, bytes) in RUNTIME_FILES.iter().chain(LICENSE_FILES.iter()) {
            assert!(!bytes.is_empty(), "{name}");
            assert_eq!(fs::read(directory.join(name)).unwrap(), *bytes, "{name}");
        }
        let jar = directory.join("MtkDirectTool.jar");
        fs::write(&jar, b"stale").unwrap();
        materialize(&directory, RUNTIME_FILES).unwrap();
        assert_eq!(fs::read(jar).unwrap(), RUNTIME_FILES[3].1);
        assert!(directory.join("licenses/components/Redist.txt").is_file());
        fs::remove_dir_all(directory).unwrap();
    }
}
