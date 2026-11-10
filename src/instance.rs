//! One resident backend per Windows user/session, with activation of its UI child.
use std::hash::{DefaultHasher, Hash, Hasher};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, ERROR_ALREADY_EXISTS, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject};

struct OwnedHandle(isize);
impl OwnedHandle {
    fn raw(&self) -> HANDLE { HANDLE(self.0 as *mut _) }
}
impl Drop for OwnedHandle {
    fn drop(&mut self) { unsafe { let _ = CloseHandle(self.raw()); } }
}

pub struct Instance {
    _mutex: OwnedHandle,
    main: OwnedHandle,
    brightness: OwnedHandle,
}

fn wide(value: &str) -> Vec<u16> { value.encode_utf16().chain(Some(0)).collect() }

impl Instance {
    /// None means the existing process received the requested activation.
    pub fn acquire(tray: bool, brightness: bool) -> windows::core::Result<Option<Self>> {
        let mut hash = DefaultHasher::new();
        std::env::var_os("USERPROFILE").hash(&mut hash);
        // Explicit test/portable profiles must not activate the user's real app.
        if let Some(path) = std::env::var_os("MIMONITOR_SETTINGS_PATH") {
            path.hash(&mut hash);
        }
        Self::acquire_named(&format!("Local\\MimonitorToolbox.{:x}", hash.finish()), tray, brightness)
    }

    fn acquire_named(base: &str, tray: bool, brightness: bool) -> windows::core::Result<Option<Self>> {
        unsafe {
            let name = wide(base);
            let mutex = CreateMutexW(None, false, PCWSTR(name.as_ptr()))?;
            let exists = GetLastError() == ERROR_ALREADY_EXISTS;
            let mutex = OwnedHandle(mutex.0 as isize);
            let main_name = wide(&format!("{base}.activate"));
            let brightness_name = wide(&format!("{base}.brightness"));
            let instance = Self {
                _mutex: mutex,
                main: OwnedHandle(CreateEventW(None, false, false, PCWSTR(main_name.as_ptr()))?.0 as isize),
                brightness: OwnedHandle(CreateEventW(None, false, false, PCWSTR(brightness_name.as_ptr()))?.0 as isize),
            };
            if exists {
                if brightness { SetEvent(instance.brightness.raw())?; }
                else if !tray { SetEvent(instance.main.raw())?; }
                return Ok(None);
            }
            Ok(Some(instance))
        }
    }

    pub fn take_main(&self) -> bool {
        unsafe { WaitForSingleObject(self.main.raw(), 0) == WAIT_OBJECT_0 }
    }
    pub fn take_brightness(&self) -> bool {
        unsafe { WaitForSingleObject(self.brightness.raw(), 0) == WAIT_OBJECT_0 }
    }

    #[cfg(test)]
    fn test_base() -> String { format!("Local\\MimonitorToolbox.Test.{}", uuid::Uuid::new_v4()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn second_launch_activates_the_first_and_releases_on_exit() {
        let base = Instance::test_base();
        let first = Instance::acquire_named(&base, true, false).unwrap().unwrap();
        assert!(Instance::acquire_named(&base, false, false).unwrap().is_none());
        assert!(first.take_main());
        assert!(!first.take_main());
        assert!(Instance::acquire_named(&base, false, true).unwrap().is_none());
        assert!(first.take_brightness());
        assert!(Instance::acquire_named(&base, true, false).unwrap().is_none());
        assert!(!first.take_main());
        drop(first);
        assert!(Instance::acquire_named(&base, true, false).unwrap().is_some());
    }
}
