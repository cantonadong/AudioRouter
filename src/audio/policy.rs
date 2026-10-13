//! Internal Windows routing ABI verified against EarTrumpet's original interfaces.
//! https://github.com/File-New-Project/EarTrumpet/tree/master/EarTrumpet/Interop/MMDeviceAPI
use super::com::*;
use crate::win32::{Handle, wide};
use std::ffi::c_void;
use std::ptr::null_mut;

const POLICY_CLASS: Guid = guid(
    0x870AF99C,
    0x171D,
    0x4F9E,
    [0xAF, 0x0D, 0xE6, 0x3D, 0xF4, 0x0C, 0x2B, 0xC9],
);
const POLICY_IID: Guid = guid(
    0xF8679F50,
    0x850A,
    0x41CF,
    [0x9C, 0x72, 0x43, 0x0F, 0x29, 0x02, 0x90, 0xC8],
);
const FACTORY_NEW: Guid = guid(
    0xAB3D4648,
    0xE242,
    0x459F,
    [0xB0, 0x2F, 0x54, 0x1C, 0x70, 0x30, 0x63, 0x24],
);
const FACTORY_OLD: Guid = guid(
    0x2A59116D,
    0x6C4F,
    0x45E0,
    [0xA7, 0x4F, 0x70, 0x7E, 0x3F, 0xEF, 0x92, 0x58],
);
const PREFIX: &str = "\\\\?\\SWD#MMDEVAPI#";
const SUFFIX: &str = "#{e6327cad-dcec-4949-ae8a-991e976a79d2}";

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(name: *const u16) -> Handle;
    fn FreeLibrary(library: Handle) -> i32;
    fn GetProcAddress(library: Handle, name: *const u8) -> *const c_void;
}
type CreateString = unsafe extern "system" fn(*const u16, u32, *mut Raw) -> Hr;
type DeleteString = unsafe extern "system" fn(Raw) -> Hr;
type GetString = unsafe extern "system" fn(Raw, *mut u32) -> *const u16;
type GetFactory = unsafe extern "system" fn(Raw, *const Guid, *mut Raw) -> Hr;
type RoInit = unsafe extern "system" fn(u32) -> Hr;
type RoUninit = unsafe extern "system" fn();

pub struct Routing {
    factory: Option<Com>,
    library: Handle,
    create: CreateString,
    delete: DeleteString,
    get: GetString,
    uninit: RoUninit,
}

unsafe fn function<F: Copy>(library: Handle, name: &[u8]) -> Result<F, String> {
    unsafe {
        let ptr = GetProcAddress(library, name.as_ptr());
        if ptr.is_null() {
            return Err("Windows 音频路由接口不可用".into());
        }
        Ok(std::mem::transmute_copy(&ptr))
    }
}

impl Routing {
    pub fn new() -> Result<Self, String> {
        unsafe {
            let library = LoadLibraryW(wide("combase.dll").as_ptr());
            if library == 0 {
                return Err("无法加载 Windows 音频路由组件".into());
            }
            let result = (|| {
                let init: RoInit = function(library, b"RoInitialize\0")?;
                let uninit: RoUninit = function(library, b"RoUninitialize\0")?;
                let create: CreateString = function(library, b"WindowsCreateString\0")?;
                let delete: DeleteString = function(library, b"WindowsDeleteString\0")?;
                let get: GetString = function(library, b"WindowsGetStringRawBuffer\0")?;
                let factory: GetFactory = function(library, b"RoGetActivationFactory\0")?;
                check(init(1), "初始化音频路由")?;
                let mut string = null_mut();
                let class = wide("Windows.Media.Internal.AudioPolicyConfig");
                let hr = create(class.as_ptr(), (class.len() - 1) as u32, &mut string);
                if hr < 0 {
                    uninit();
                    return Err("无法创建音频路由名称".into());
                }
                let mut ptr = null_mut();
                let mut hr = factory(string, &FACTORY_NEW, &mut ptr);
                if hr < 0 {
                    hr = factory(string, &FACTORY_OLD, &mut ptr);
                }
                delete(string);
                if hr < 0 {
                    uninit();
                    return Err(format!("Windows 应用输出路由不可用（0x{:08X}）", hr as u32));
                }
                match Com::from_result(ptr) {
                    Ok(factory) => Ok(Self {
                        factory: Some(factory),
                        library,
                        create,
                        delete,
                        get,
                        uninit,
                    }),
                    Err(error) => {
                        uninit();
                        Err(error)
                    }
                }
            })();
            if result.is_err() {
                FreeLibrary(library);
            }
            result
        }
    }

    pub fn set(&self, pid: u32, device: Option<&str>) -> Result<(), String> {
        unsafe {
            let desired = device.map(str::to_owned);
            let changed: Vec<_> = (0..3)
                .filter(|role| self.get_role(pid, *role) != Some(desired.clone()))
                .collect();
            if changed.is_empty() { return Ok(()); }
            let mut string = null_mut();
            if let Some(device) = device {
                let text = wide(&format!("{PREFIX}{device}{SUFFIX}"));
                check(
                    (self.create)(text.as_ptr(), (text.len() - 1) as u32, &mut string),
                    "创建播放设备标识",
                )?;
            }
            let factory = self.factory.as_ref().unwrap();
            let set: unsafe extern "system" fn(Raw, u32, i32, i32, Raw) -> Hr = factory.method(25);
            let mut result = Ok(());
            for role in changed {
                if let Err(error) = check(set(factory.0, pid, 0, role, string), "切换应用输出设备")
                {
                    result = Err(error);
                    break;
                }
            }
            (self.delete)(string);
            result
        }
    }

    pub fn get(&self, pid: u32) -> Option<String> {
        self.get_role(pid, 1).flatten()
    }

    // Outer None means the read failed; Some(None) is a confirmed default route.
    // Never mistake a failed read for a matching route when deciding to skip a write.
    fn get_role(&self, pid: u32, role: i32) -> Option<Option<String>> {
        unsafe {
            let factory = self.factory.as_ref()?;
            let get: unsafe extern "system" fn(Raw, u32, i32, i32, *mut Raw) -> Hr =
                factory.method(26);
            let mut string = null_mut();
            if get(factory.0, pid, 0, role, &mut string) < 0 {
                return None;
            }
            let mut length = 0;
            let ptr = (self.get)(string, &mut length);
            let text = if ptr.is_null() {
                String::new()
            } else {
                String::from_utf16_lossy(std::slice::from_raw_parts(ptr, length as usize))
            };
            (self.delete)(string);
            if text.is_empty() {
                Some(None)
            } else {
                Some(Some(
                    text.strip_prefix(PREFIX)
                        .unwrap_or(&text)
                        .strip_suffix(SUFFIX)
                        .unwrap_or(text.strip_prefix(PREFIX).unwrap_or(&text))
                        .to_string(),
                ))
            }
        }
    }
}
impl Drop for Routing {
    fn drop(&mut self) {
        self.factory.take();
        unsafe {
            (self.uninit)();
            FreeLibrary(self.library);
        }
    }
}

pub fn set_default(device: &str) -> Result<(), String> {
    let policy = Com::create(&POLICY_CLASS, &POLICY_IID)?;
    let text = wide(device);
    unsafe {
        let set: unsafe extern "system" fn(Raw, *const u16, i32) -> Hr = policy.method(13);
        for role in 0..3 {
            check(set(policy.0, text.as_ptr(), role), "设置默认播放设备")?;
        }
    }
    Ok(())
}
