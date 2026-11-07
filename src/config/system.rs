use windows::{core::w, Win32::System::Registry::*};
pub fn system_is_light() -> bool {
    unsafe {
        let mut value = 0u32;
        let mut length = 4;
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut _ as *mut _),
            Some(&mut length),
        )
        .is_ok()
            && value != 0
    }
}
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    unsafe {
        let mut key = HKEY::default();
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()
        .map_err(|e| e.to_string())?;
        let result = if enabled {
            let exe = std::env::current_exe().map_err(|e| e.to_string());
            match exe {
                Ok(exe) => {
                    let value = format!("\"{}\" --tray", exe.display())
                        .encode_utf16()
                        .chain(Some(0))
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>();
                    RegSetValueExW(key, w!("MimonitorToolboxGPUI"), None, REG_SZ, Some(&value))
                        .ok()
                        .map_err(|e| e.to_string())
                }
                Err(error) => Err(error),
            }
        } else {
            let code = RegDeleteValueW(key, w!("MimonitorToolboxGPUI"));
            if code.0 == 2 {
                Ok(())
            } else {
                code.ok().map_err(|e| e.to_string())
            }
        };
        let _ = RegCloseKey(key);
        result
    }
}
