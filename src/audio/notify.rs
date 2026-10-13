use super::com::*;
use crate::win32::{CloseHandle, Handle};
use std::ffi::c_void;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering},
};

pub const STRUCTURE: u32 = 1;
pub const ENDPOINT: u32 = 2;
pub const SESSION_VOLUME: u32 = 4;
pub const COMMAND: u32 = 8;
pub const DEFAULT: u32 = 16;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateEventW(
        attributes: *const c_void,
        manual: i32,
        initial: i32,
        name: *const u16,
    ) -> Handle;
    fn SetEvent(event: Handle) -> i32;
    pub fn WaitForSingleObject(handle: Handle, timeout: u32) -> u32;
}

pub struct Wake {
    pub event: Handle,
    pub flags: AtomicU32,
    created: AtomicPtr<CreatedSession>,
    accepting: AtomicBool,
    callbacks: AtomicU32,
}
pub struct CreatedSession {
    pub control: Raw,
    pub device: String,
    next: *mut CreatedSession,
}
impl Wake {
    pub fn new() -> Result<Arc<Self>, String> {
        let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
        if event == 0 {
            return Err("无法创建音频通知事件".into());
        }
        Ok(Arc::new(Self {
            event,
            flags: AtomicU32::new(0),
            created: AtomicPtr::new(std::ptr::null_mut()),
            accepting: AtomicBool::new(true),
            callbacks: AtomicU32::new(0),
        }))
    }
    pub fn notify(&self, flags: u32) {
        self.flags.fetch_or(flags, Ordering::Release);
        unsafe {
            SetEvent(self.event);
        }
    }
    pub fn take_sessions(&self) -> Vec<(String, Com)> {
        let mut node = self.created.swap(std::ptr::null_mut(), Ordering::AcqRel);
        let mut sessions = Vec::new();
        while !node.is_null() {
            unsafe {
                let item = Box::from_raw(node);
                node = item.next;
                if let Ok(control) = Com::from_result(item.control) {
                    sessions.push((item.device, control));
                }
            }
        }
        sessions
    }
    pub fn stop_sessions(&self) {
        self.accepting.store(false, Ordering::SeqCst);
        while self.callbacks.load(Ordering::SeqCst) != 0 {
            std::thread::yield_now();
        }
    }
}
impl Drop for Wake {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.event);
        }
    }
}

#[repr(C)]
struct Sink {
    vtable: *const c_void,
    refs: AtomicU32,
    wake: Arc<Wake>,
    iid: Guid,
    device: String,
}

#[repr(C)]
struct UnknownTable {
    query: unsafe extern "system" fn(Raw, *const Guid, *mut Raw) -> Hr,
    add: unsafe extern "system" fn(Raw) -> u32,
    release: unsafe extern "system" fn(Raw) -> u32,
}
const BASE: UnknownTable = UnknownTable {
    query,
    add,
    release,
};

unsafe extern "system" fn query(this: Raw, iid: *const Guid, result: *mut Raw) -> Hr {
    unsafe {
        if result.is_null() || iid.is_null() {
            return 0x80004003u32 as Hr;
        }
        *result = std::ptr::null_mut();
        let sink = &*(this as *const Sink);
        if *iid == UNKNOWN || *iid == sink.iid {
            *result = this;
            add(this);
            0
        } else {
            0x80004002u32 as Hr
        }
    }
}
unsafe extern "system" fn add(this: Raw) -> u32 {
    unsafe {
        (*(this as *const Sink))
            .refs
            .fetch_add(1, Ordering::Relaxed)
            + 1
    }
}
unsafe extern "system" fn release(this: Raw) -> u32 {
    unsafe {
        let previous = (*(this as *const Sink)).refs.fetch_sub(1, Ordering::AcqRel);
        if previous == 1 {
            drop(Box::from_raw(this as *mut Sink));
        }
        previous - 1
    }
}
unsafe fn signal(this: Raw, flags: u32) -> Hr {
    unsafe {
        (*(this as *const Sink)).wake.notify(flags);
    }
    0
}
unsafe fn own(context: *const Guid) -> bool {
    unsafe { !context.is_null() && *context == CONTEXT }
}

#[repr(C)]
struct DeviceTable {
    base: UnknownTable,
    state: unsafe extern "system" fn(Raw, *const u16, u32) -> Hr,
    added: unsafe extern "system" fn(Raw, *const u16) -> Hr,
    removed: unsafe extern "system" fn(Raw, *const u16) -> Hr,
    default: unsafe extern "system" fn(Raw, i32, i32, *const u16) -> Hr,
    property: unsafe extern "system" fn(Raw, *const u16, PropertyKey) -> Hr,
}
unsafe extern "system" fn device_state(this: Raw, _: *const u16, _: u32) -> Hr {
    unsafe { signal(this, STRUCTURE) }
}
unsafe extern "system" fn device_change(this: Raw, _: *const u16) -> Hr {
    unsafe { signal(this, STRUCTURE) }
}
unsafe extern "system" fn default_change(this: Raw, flow: i32, _: i32, _: *const u16) -> Hr {
    if flow == 0 {
        unsafe { signal(this, DEFAULT) }
    } else {
        0
    }
}
unsafe extern "system" fn property_change(this: Raw, _: *const u16, _: PropertyKey) -> Hr {
    unsafe { signal(this, STRUCTURE) }
}
static DEVICE_TABLE: DeviceTable = DeviceTable {
    base: BASE,
    state: device_state,
    added: device_change,
    removed: device_change,
    default: default_change,
    property: property_change,
};

#[repr(C)]
struct EndpointTable {
    base: UnknownTable,
    notify: unsafe extern "system" fn(Raw, *const Guid) -> Hr,
}
unsafe extern "system" fn endpoint_change(this: Raw, data: *const Guid) -> Hr {
    if unsafe { own(data) } {
        0
    } else {
        unsafe { signal(this, ENDPOINT) }
    }
}
static ENDPOINT_TABLE: EndpointTable = EndpointTable {
    base: BASE,
    notify: endpoint_change,
};

#[repr(C)]
struct NewSessionTable {
    base: UnknownTable,
    created: unsafe extern "system" fn(Raw, Raw) -> Hr,
}
unsafe extern "system" fn session_created(this: Raw, control: Raw) -> Hr {
    if control.is_null() {
        return 0;
    }
    unsafe {
        let sink = &*(this as *const Sink);
        sink.wake.callbacks.fetch_add(1, Ordering::SeqCst);
        if !sink.wake.accepting.load(Ordering::SeqCst) {
            sink.wake.callbacks.fetch_sub(1, Ordering::SeqCst);
            return 0;
        }
        let table = *(control as *const *const *const c_void);
        let retain: unsafe extern "system" fn(Raw) -> u32 = std::mem::transmute(*table.add(1));
        retain(control);
        let node = Box::into_raw(Box::new(CreatedSession {
            control,
            device: sink.device.clone(),
            next: std::ptr::null_mut(),
        }));
        let mut head = sink.wake.created.load(Ordering::Acquire);
        loop {
            (*node).next = head;
            match sink.wake.created.compare_exchange_weak(
                head,
                node,
                Ordering::Release,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(current) => head = current,
            }
        }
        let result = signal(this, STRUCTURE);
        sink.wake.callbacks.fetch_sub(1, Ordering::SeqCst);
        result
    }
}
static NEW_SESSION_TABLE: NewSessionTable = NewSessionTable {
    base: BASE,
    created: session_created,
};

#[repr(C)]
struct SessionTable {
    base: UnknownTable,
    name: unsafe extern "system" fn(Raw, *const u16, *const Guid) -> Hr,
    icon: unsafe extern "system" fn(Raw, *const u16, *const Guid) -> Hr,
    volume: unsafe extern "system" fn(Raw, f32, i32, *const Guid) -> Hr,
    channel: unsafe extern "system" fn(Raw, u32, *const f32, u32, *const Guid) -> Hr,
    grouping: unsafe extern "system" fn(Raw, *const Guid, *const Guid) -> Hr,
    state: unsafe extern "system" fn(Raw, i32) -> Hr,
    disconnected: unsafe extern "system" fn(Raw, i32) -> Hr,
}
unsafe extern "system" fn session_name(this: Raw, _: *const u16, _: *const Guid) -> Hr {
    unsafe { signal(this, STRUCTURE) }
}
unsafe extern "system" fn session_volume(this: Raw, _: f32, _: i32, context: *const Guid) -> Hr {
    if unsafe { own(context) } {
        0
    } else {
        unsafe { signal(this, SESSION_VOLUME) }
    }
}
unsafe extern "system" fn session_channel(
    this: Raw,
    _: u32,
    _: *const f32,
    _: u32,
    context: *const Guid,
) -> Hr {
    if unsafe { own(context) } {
        0
    } else {
        unsafe { signal(this, SESSION_VOLUME) }
    }
}
unsafe extern "system" fn session_group(this: Raw, _: *const Guid, _: *const Guid) -> Hr {
    unsafe { signal(this, STRUCTURE) }
}
unsafe extern "system" fn session_state(this: Raw, _: i32) -> Hr {
    unsafe { signal(this, STRUCTURE) }
}
static SESSION_TABLE: SessionTable = SessionTable {
    base: BASE,
    name: session_name,
    icon: session_name,
    volume: session_volume,
    channel: session_channel,
    grouping: session_group,
    state: session_state,
    disconnected: session_state,
};

pub struct Subscription(pub Raw);
impl Subscription {
    pub fn new(iid: Guid, wake: &Arc<Wake>) -> Self {
        Self::for_device(iid, wake, String::new())
    }
    pub fn for_device(iid: Guid, wake: &Arc<Wake>, device: String) -> Self {
        let vtable = if iid == DEVICE_NOTIFY {
            &DEVICE_TABLE as *const _ as *const c_void
        } else if iid == ENDPOINT_NOTIFY {
            &ENDPOINT_TABLE as *const _ as *const c_void
        } else if iid == SESSION_NOTIFY {
            &NEW_SESSION_TABLE as *const _ as *const c_void
        } else {
            &SESSION_TABLE as *const _ as *const c_void
        };
        Self(
            Box::into_raw(Box::new(Sink {
                vtable,
                refs: AtomicU32::new(1),
                wake: wake.clone(),
                iid,
                device,
            }))
            .cast(),
        )
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        unsafe {
            release(self.0);
        }
    }
}
