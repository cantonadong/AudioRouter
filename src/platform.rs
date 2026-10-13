use crate::win32::*;
use std::ptr::{null, null_mut};

const HKCU: Handle = -2147483647;
const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(name: *const u16) -> Handle;
    fn GetProcAddress(library: Handle, name: *const u8) -> *const std::ffi::c_void;
}

// Same menu theme entry points used by Microsoft's PowerToys ZoomIt.
// Resolve dynamically so missing theme APIs leave the standard system menu intact.
pub fn refresh_menu_theme(hwnd: Handle) {
    unsafe {
        let mut key = 0;
        let mut light = 1u32;
        let mut bytes = 4;
        let mut kind = 0;
        if RegOpenKeyExW(HKCU,
            wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize").as_ptr(),
            0, 1, &mut key) == 0 {
            RegQueryValueExW(key, wide("SystemUsesLightTheme").as_ptr(), null_mut(),
                &mut kind, (&mut light as *mut u32).cast(), &mut bytes);
            RegCloseKey(key);
        }
        static THEME: std::sync::OnceLock<Handle> = std::sync::OnceLock::new();
        let library = *THEME.get_or_init(|| LoadLibraryW(wide("uxtheme.dll").as_ptr()));
        if library == 0 { return; }
        let mode = GetProcAddress(library, 135usize as *const u8);
        let allow = GetProcAddress(library, 133usize as *const u8);
        let flush = GetProcAddress(library, 136usize as *const u8);
        if !mode.is_null() {
            let set: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(mode);
            set(if light == 0 { 2 } else { 3 });
        }
        if !allow.is_null() {
            let set: unsafe extern "system" fn(Handle, bool) -> bool = std::mem::transmute(allow);
            set(hwnd, light == 0);
        }
        if !flush.is_null() {
            let refresh: unsafe extern "system" fn() = std::mem::transmute(flush);
            refresh();
        }
    }
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegOpenKeyExW(
        key: Handle,
        subkey: *const u16,
        options: u32,
        access: u32,
        result: *mut Handle,
    ) -> i32;
    fn RegCreateKeyExW(
        key: Handle,
        subkey: *const u16,
        reserved: u32,
        class: *mut u16,
        options: u32,
        access: u32,
        attributes: *const std::ffi::c_void,
        result: *mut Handle,
        disposition: *mut u32,
    ) -> i32;
    fn RegSetValueExW(
        key: Handle,
        name: *const u16,
        reserved: u32,
        kind: u32,
        data: *const u8,
        size: u32,
    ) -> i32;
    fn RegQueryValueExW(
        key: Handle,
        name: *const u16,
        reserved: *mut u32,
        kind: *mut u32,
        data: *mut u8,
        size: *mut u32,
    ) -> i32;
    fn RegDeleteValueW(key: Handle, name: *const u16) -> i32;
    fn RegCloseKey(key: Handle) -> i32;
}
#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteW(
        hwnd: Handle,
        verb: *const u16,
        file: *const u16,
        args: *const u16,
        directory: *const u16,
        show: i32,
    ) -> Handle;
}

fn startup_command() -> Result<String, String> {
    let path = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(format!("\"{}\" --background", path.display()))
}

pub fn startup_enabled() -> bool {
    unsafe {
        let mut key = 0;
        if RegOpenKeyExW(HKCU, wide(RUN).as_ptr(), 0, 1, &mut key) != 0 {
            return false;
        }
        let mut bytes = 4096;
        let mut kind = 0;
        let mut data = [0u16; 2048];
        let result = RegQueryValueExW(
            key,
            wide("AudioRouter").as_ptr(),
            null_mut(),
            &mut kind,
            data.as_mut_ptr().cast(),
            &mut bytes,
        );
        RegCloseKey(key);
        if result != 0 || kind != 1 {
            return false;
        }
        let length = data.iter().position(|c| *c == 0).unwrap_or(data.len());
        startup_command().is_ok_and(|command| {
            String::from_utf16_lossy(&data[..length]).eq_ignore_ascii_case(&command)
        })
    }
}

pub fn set_startup(enabled: bool) -> Result<(), String> {
    unsafe {
        let mut key = 0;
        let status = RegCreateKeyExW(
            HKCU,
            wide(RUN).as_ptr(),
            0,
            null_mut(),
            0,
            2,
            null(),
            &mut key,
            null_mut(),
        );
        if status != 0 {
            return Err(format!("无法打开开机启动设置（{status}）"));
        }
        let result = if enabled {
            match startup_command() {
                Ok(command) => {
                    let data = wide(&command);
                    RegSetValueExW(
                        key,
                        wide("AudioRouter").as_ptr(),
                        0,
                        1,
                        data.as_ptr().cast(),
                        (data.len() * 2) as u32,
                    )
                }
                Err(error) => {
                    RegCloseKey(key);
                    return Err(error);
                }
            }
        } else {
            RegDeleteValueW(key, wide("AudioRouter").as_ptr())
        };
        RegCloseKey(key);
        if result == 0 || (!enabled && result == 2) {
            Ok(())
        } else {
            Err(format!("无法更改开机启动设置（{result}）"))
        }
    }
}

pub fn open_settings(hwnd: Handle, page: Option<u8>) -> Result<(), String> {
    unsafe {
        let (file, arguments) = if let Some(page) = page {
            (
                wide("control.exe"),
                wide(&format!("mmsys.cpl,,{}", page.min(3))),
            )
        } else {
            (wide("ms-settings:sound"), wide(""))
        };
        let result = ShellExecuteW(
            hwnd,
            wide("open").as_ptr(),
            file.as_ptr(),
            arguments.as_ptr(),
            null(),
            SW_SHOW,
        );
        if result > 32 {
            Ok(())
        } else {
            Err(format!("无法打开系统声音设置（{result}）"))
        }
    }
}
