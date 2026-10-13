#![allow(dead_code)]

use std::ffi::c_void;
use std::ptr::null_mut;

pub type Hr = i32;
pub type Raw = *mut c_void;
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Guid {
    pub a: u32,
    pub b: u16,
    pub c: u16,
    pub d: [u8; 8],
}
pub const fn guid(a: u32, b: u16, c: u16, d: [u8; 8]) -> Guid {
    Guid { a, b, c, d }
}

pub const UNKNOWN: Guid = guid(0, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
pub const ENUMERATOR_CLASS: Guid = guid(
    0xBCDE0395,
    0xE52F,
    0x467C,
    [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E],
);
pub const ENUMERATOR: Guid = guid(
    0xA95664D2,
    0x9614,
    0x4F35,
    [0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6],
);
pub const ENDPOINT_VOLUME: Guid = guid(
    0x5CDF2C82,
    0x841E,
    0x4546,
    [0x97, 0x22, 0x0C, 0xF7, 0x40, 0x78, 0x22, 0x9A],
);
pub const SESSION_MANAGER: Guid = guid(
    0x77AA99A0,
    0x1BD6,
    0x484F,
    [0x8B, 0xC7, 0x2C, 0x65, 0x4C, 0x9A, 0x9B, 0x6F],
);
pub const SESSION_CONTROL: Guid = guid(
    0xBFB7FF88,
    0x7239,
    0x4FC9,
    [0x8F, 0xA2, 0x07, 0xC9, 0x50, 0xBE, 0x9C, 0x6D],
);
pub const SIMPLE_VOLUME: Guid = guid(
    0x87CE5498,
    0x68D6,
    0x44E5,
    [0x92, 0x15, 0x6D, 0xA4, 0x7E, 0xF8, 0x83, 0xD8],
);
pub const AUDIO_METER: Guid = guid(
    0xC02216F6, 0x8C67, 0x4B5B,
    [0x9D, 0x00, 0xD0, 0x08, 0xE7, 0x3E, 0x00, 0x64],
);
pub const DEVICE_NOTIFY: Guid = guid(
    0x7991EEC9,
    0x7E89,
    0x4D85,
    [0x83, 0x90, 0x6C, 0x70, 0x3C, 0xEC, 0x60, 0xC0],
);
pub const ENDPOINT_NOTIFY: Guid = guid(
    0x657804FA,
    0xD6AD,
    0x4496,
    [0x8A, 0x60, 0x35, 0x27, 0x52, 0xAF, 0x4F, 0x89],
);
pub const SESSION_NOTIFY: Guid = guid(
    0x641DD20B,
    0x4D41,
    0x49CC,
    [0xAB, 0xA3, 0x17, 0x4B, 0x94, 0x77, 0xBB, 0x08],
);
pub const SESSION_EVENTS: Guid = guid(
    0x24918ACC,
    0x64B3,
    0x37C1,
    [0x8C, 0xA9, 0x74, 0xA6, 0x6E, 0x99, 0x57, 0xA8],
);
pub const CONTEXT: Guid = guid(
    0xA6CE2D32,
    0x287E,
    0x4B13,
    [0xA2, 0x03, 0x81, 0x55, 0x65, 0x32, 0xB0, 0xC2],
);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PropertyKey {
    pub fmtid: Guid,
    pub pid: u32,
}
pub const FRIENDLY_NAME: PropertyKey = PropertyKey {
    fmtid: guid(
        0xA45C254E,
        0xDF1C,
        0x4EFD,
        [0x80, 0x20, 0x67, 0xD1, 0x46, 0xA8, 0x50, 0xE0],
    ),
    pid: 14,
};
#[repr(C)]
pub struct PropVariant {
    pub kind: u16,
    pub reserved: [u16; 3],
    pub data: [usize; 2],
}

#[link(name = "ole32")]
unsafe extern "system" {
    pub fn CoInitializeEx(reserved: Raw, mode: u32) -> Hr;
    pub fn CoUninitialize();
    fn CoCreateInstance(
        class: *const Guid,
        outer: Raw,
        context: u32,
        iid: *const Guid,
        result: *mut Raw,
    ) -> Hr;
    pub fn CoTaskMemFree(value: Raw);
    pub fn PropVariantClear(value: *mut PropVariant) -> Hr;
}

pub fn check(hr: Hr, action: &str) -> Result<(), String> {
    if hr >= 0 {
        Ok(())
    } else {
        Err(format!("{action}失败（0x{:08X}）", hr as u32))
    }
}

/// COM pointers stay on the dedicated MTA thread and are never sent to the UI.
pub struct Com(pub Raw);
impl Com {
    pub fn create(class: &Guid, iid: &Guid) -> Result<Self, String> {
        let mut ptr = null_mut();
        unsafe {
            check(
                CoCreateInstance(class, null_mut(), 23, iid, &mut ptr),
                "创建音频接口",
            )?;
        }
        Self::from_result(ptr)
    }
    pub fn from_result(ptr: Raw) -> Result<Self, String> {
        if ptr.is_null() {
            Err("音频接口返回空指针".into())
        } else {
            Ok(Self(ptr))
        }
    }
    pub unsafe fn method<F: Copy>(&self, index: usize) -> F {
        unsafe {
            let table = *(self.0 as *const *const Raw);
            let address = *table.add(index);
            std::mem::transmute_copy(&address)
        }
    }
    pub fn query(&self, iid: &Guid) -> Result<Self, String> {
        let mut ptr = null_mut();
        unsafe {
            let query: unsafe extern "system" fn(Raw, *const Guid, *mut Raw) -> Hr = self.method(0);
            check(query(self.0, iid, &mut ptr), "查询音频接口")?;
        }
        Self::from_result(ptr)
    }
    pub fn activate(&self, iid: &Guid) -> Result<Self, String> {
        let mut ptr = null_mut();
        unsafe {
            let activate: unsafe extern "system" fn(Raw, *const Guid, u32, Raw, *mut Raw) -> Hr =
                self.method(3);
            check(
                activate(self.0, iid, 23, null_mut(), &mut ptr),
                "激活音频设备",
            )?;
        }
        Self::from_result(ptr)
    }
    pub fn string(&self, index: usize) -> Result<String, String> {
        let mut text = null_mut();
        unsafe {
            let method: unsafe extern "system" fn(Raw, *mut *mut u16) -> Hr = self.method(index);
            let hr = method(self.0, &mut text);
            let result = if hr >= 0 {
                Ok(read_wide(text))
            } else {
                Err(format!("读取音频名称失败（0x{:08X}）", hr as u32))
            };
            if !text.is_null() {
                CoTaskMemFree(text.cast());
            }
            result
        }
    }
    pub fn scalar(&self, index: usize) -> Result<f32, String> {
        let mut value = 0.0;
        unsafe {
            let method: unsafe extern "system" fn(Raw, *mut f32) -> Hr = self.method(index);
            check(method(self.0, &mut value), "读取音量")?;
        }
        Ok(value.clamp(0.0, 1.0))
    }
    pub fn number(&self, index: usize) -> Result<u32, String> {
        let mut value = 0;
        unsafe {
            let method: unsafe extern "system" fn(Raw, *mut u32) -> Hr = self.method(index);
            check(method(self.0, &mut value), "读取音频状态")?;
        }
        Ok(value)
    }
    pub fn set_scalar(&self, index: usize, value: f32) -> Result<(), String> {
        unsafe {
            let method: unsafe extern "system" fn(Raw, f32, *const Guid) -> Hr = self.method(index);
            check(method(self.0, value.clamp(0.0, 1.0), &CONTEXT), "设置音量")
        }
    }
    pub fn set_mute(&self, index: usize, value: bool) -> Result<(), String> {
        unsafe {
            let method: unsafe extern "system" fn(Raw, i32, *const Guid) -> Hr = self.method(index);
            check(method(self.0, i32::from(value), &CONTEXT), "设置静音")
        }
    }
    pub fn callback(&self, index: usize, ptr: Raw) -> Result<(), String> {
        unsafe {
            let method: unsafe extern "system" fn(Raw, Raw) -> Hr = self.method(index);
            check(method(self.0, ptr), "订阅音频通知")
        }
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        unsafe {
            let release: unsafe extern "system" fn(Raw) -> u32 = self.method(2);
            release(self.0);
        }
    }
}

pub unsafe fn read_wide(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let mut len = 0;
        while len < 32768 && *ptr.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
    }
}

pub fn device_name(device: &Com) -> Result<String, String> {
    unsafe {
        let mut store = null_mut();
        let open: unsafe extern "system" fn(Raw, u32, *mut Raw) -> Hr = device.method(4);
        check(open(device.0, 0, &mut store), "读取设备属性")?;
        let store = Com::from_result(store)?;
        let mut value: PropVariant = std::mem::zeroed();
        let get: unsafe extern "system" fn(Raw, *const PropertyKey, *mut PropVariant) -> Hr =
            store.method(5);
        // The endpoint description is the user-editable name, without the adapter suffix.
        let alias_key = PropertyKey { fmtid: FRIENDLY_NAME.fmtid, pid: 2 };
        let mut hr = get(store.0, &alias_key, &mut value);
        if hr < 0 || value.kind != 31 || read_wide(value.data[0] as *const u16).is_empty() {
            PropVariantClear(&mut value);
            value = std::mem::zeroed();
            hr = get(store.0, &FRIENDLY_NAME, &mut value);
        }
        let name = if hr >= 0 && value.kind == 31 {
            read_wide(value.data[0] as *const u16)
        } else {
            String::new()
        };
        PropVariantClear(&mut value);
        check(hr, "读取设备名称")?;
        Ok(name)
    }
}
