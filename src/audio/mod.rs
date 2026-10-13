mod com;
mod notify;
mod policy;

use crate::config::{self, Config, VolumePreference};
use crate::model::{App, Device, Model, Scenario};
use crate::win32::{CloseHandle, Handle, PostMessageW};
use com::*;
use notify::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ptr::null_mut;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

pub const WM_AUDIO_UPDATE: u32 = 0x8004;
pub const WM_AUDIO_PEAK: u32 = 0x8005;
#[derive(Clone)]
pub enum Command {
    Volume {
        device: String,
        app: Option<String>,
        value: u8,
    },
    Mute {
        device: String,
        app: Option<String>,
        muted: bool,
    },
    Default(String),
    Route {
        app: String,
        device: Option<String>,
    },
    Startup(bool),
    Refresh,
    Meter(bool),
    HideApp { key: String, name: String },
    ShowApp(String),
    Shutdown,
}
struct Updates {
    latest: Mutex<Option<Model>>,
    posted: AtomicBool,
    hwnd: Handle,
    peaks: Mutex<Option<HashMap<String, f32>>>,
    peaks_posted: AtomicBool,
}
impl Updates {
    fn publish_peaks(&self, peaks: HashMap<String, f32>) {
        *self.peaks.lock().unwrap_or_else(|e| e.into_inner()) = Some(peaks);
        if !self.peaks_posted.swap(true, Ordering::AcqRel) {
            unsafe { PostMessageW(self.hwnd, WM_AUDIO_PEAK, 0, 0); }
        }
    }
    fn publish(&self, model: Model) {
        *self.latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(model);
        if !self.posted.swap(true, Ordering::AcqRel) {
            unsafe {
                PostMessageW(self.hwnd, WM_AUDIO_UPDATE, 0, 0);
            }
        }
    }
}
pub struct AudioClient {
    sender: mpsc::Sender<Command>,
    wake: Arc<Wake>,
    updates: Arc<Updates>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl AudioClient {
    pub fn start(hwnd: Handle) -> Result<Self, String> {
        let wake = Wake::new()?;
        let updates = Arc::new(Updates {
            latest: Mutex::new(None),
            posted: AtomicBool::new(false),
            hwnd,
            peaks: Mutex::new(None),
            peaks_posted: AtomicBool::new(false),
        });
        let (sender, receiver) = mpsc::channel();
        let thread_wake = wake.clone();
        let thread_updates = updates.clone();
        let thread = std::thread::Builder::new()
            .name("AudioRouter Audio".into())
            .spawn(move || worker(receiver, thread_wake, thread_updates))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            sender,
            wake,
            updates,
            thread: Some(thread),
        })
    }
    pub fn send(&self, command: Command) {
        if self.sender.send(command).is_ok() {
            self.wake.notify(COMMAND);
        }
    }
    pub fn take(&self) -> Option<Model> {
        self.updates.posted.store(false, Ordering::Release);
        self.updates
            .latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }
    pub fn take_peaks(&self) -> Option<HashMap<String, f32>> {
        self.updates.peaks_posted.store(false, Ordering::Release);
        self.updates.peaks.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}
impl Drop for AudioClient {
    fn drop(&mut self) {
        self.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    fn GetExitCodeProcess(process: Handle, code: *mut u32) -> i32;
    fn QueryFullProcessImageNameW(
        process: Handle,
        flags: u32,
        path: *mut u16,
        size: *mut u32,
    ) -> i32;
}
fn process_path(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    unsafe {
        let process = OpenProcess(0x1000, 0, pid);
        if process == 0 {
            return String::new();
        }
        let mut path = [0u16; 32768];
        let mut length = path.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length);
        CloseHandle(process);
        if ok == 0 {
            String::new()
        } else {
            String::from_utf16_lossy(&path[..length as usize])
        }
    }
}
struct Session {
    control: Com,
    volume: Com,
    meter: Option<Com>,
    sink: Subscription,
    id: String,
    key: String,
    name: String,
    path: String,
    pid: u32,
    scalar: f32,
    muted: bool,
    needs_apply: bool,
}
struct TrackedProcess {
    handle: Handle,
    key: String,
}
impl Drop for TrackedProcess {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}
impl TrackedProcess {
    fn alive(&self) -> bool {
        let mut code = 0;
        unsafe { GetExitCodeProcess(self.handle, &mut code) != 0 && code == 259 }
    }
}
impl Session {
    fn new(control: Com, wake: &Arc<Wake>) -> Result<Self, String> {
        let volume = control.query(&SIMPLE_VOLUME)?;
        let meter = control.query(&AUDIO_METER).ok();
        let id = control.string(13)?;
        let pid = control.number(14).unwrap_or(0);
        let is_system = unsafe {
            let method: unsafe extern "system" fn(Raw) -> Hr = control.method(15);
            method(control.0) == 0
        };
        let path = process_path(pid);
        let name = if is_system {
            "系统声音".to_string()
        } else {
            let label = control.string(4).unwrap_or_default();
            if !label.is_empty() && !label.starts_with('@') {
                label
            } else if !path.is_empty() {
                std::path::Path::new(&path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| format!("应用 {pid}"))
            } else {
                format!("应用 {pid}")
            }
        };
        let key = if is_system {
            "__system_sounds__".into()
        } else if !path.is_empty() {
            path.to_lowercase()
        } else {
            control.string(12).unwrap_or_else(|_| format!("pid:{pid}"))
        };
        let sink = Subscription::new(SESSION_EVENTS, wake);
        control.callback(10, sink.0)?;
        let scalar = volume.scalar(4).unwrap_or(1.0);
        let muted = volume.number(6).unwrap_or(0) != 0;
        Ok(Self {
            control,
            volume,
            meter,
            sink,
            id,
            key,
            name,
            path,
            pid: if is_system { 0 } else { pid },
            scalar,
            muted,
            needs_apply: true,
        })
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.control.callback(11, self.sink.0);
    }
}
struct Endpoint {
    id: String,
    name: String,
    endpoint: Com,
    meter: Option<Com>,
    manager: Com,
    endpoint_sink: Subscription,
    session_sink: Subscription,
    sessions: Vec<Session>,
    scalar: f32,
    muted: bool,
    mute_loaded: bool,
}
impl Endpoint {
    fn new(device: Com, wake: &Arc<Wake>) -> Result<Self, String> {
        let id = device.string(5)?;
        let name = device_name(&device).unwrap_or_else(|_| id.clone());
        let endpoint = device.activate(&ENDPOINT_VOLUME)?;
        let meter = device.activate(&AUDIO_METER).ok();
        let manager = device.activate(&SESSION_MANAGER)?;
        let endpoint_sink = Subscription::new(ENDPOINT_NOTIFY, wake);
        let session_sink = Subscription::for_device(SESSION_NOTIFY, wake, id.clone());
        endpoint.callback(3, endpoint_sink.0)?;
        if let Err(error) = manager.callback(6, session_sink.0) {
            let _ = endpoint.callback(4, endpoint_sink.0);
            return Err(error);
        }
        let scalar = endpoint.scalar(9).unwrap_or(0.0);
        let muted = endpoint.number(15).unwrap_or(0) != 0;
        Ok(Self {
            id,
            name,
            endpoint,
            meter,
            manager,
            endpoint_sink,
            session_sink,
            sessions: vec![],
            scalar,
            muted,
            mute_loaded: false,
        })
    }
    fn enumerate_sessions(&mut self, wake: &Arc<Wake>) -> Result<(), String> {
        unsafe {
            let mut ptr = null_mut();
            let get: unsafe extern "system" fn(Raw, *mut Raw) -> Hr = self.manager.method(5);
            check(get(self.manager.0, &mut ptr), "读取应用音频会话")?;
            let enumerator = Com::from_result(ptr)?;
            // GetCount is mandatory before session-created notifications can arrive.
            let count = enumerator.number(3)?;
            let mut old: HashMap<_, _> = std::mem::take(&mut self.sessions)
                .into_iter()
                .map(|s| (s.id.clone(), s))
                .collect();
            for i in 0..count {
                let get: unsafe extern "system" fn(Raw, i32, *mut Raw) -> Hr = enumerator.method(4);
                let mut ptr = null_mut();
                if get(enumerator.0, i as i32, &mut ptr) < 0 {
                    continue;
                }
                let control = Com::from_result(ptr)?.query(&SESSION_CONTROL)?;
                if control.number(3).unwrap_or(2) == 2 {
                    continue;
                }
                let id = control.string(13)?;
                if let Some(session) = old.remove(&id) {
                    self.sessions.push(session);
                } else {
                    match Session::new(control, wake) {
                        Ok(session) => self.sessions.push(session),
                        Err(error) => config::log(&error),
                    }
                }
            }
            self.sessions.extend(
                old.into_values()
                    .filter(|s| s.control.number(3).unwrap_or(2) != 2),
            );
            self.sessions
                .sort_by(|a, b| (&a.key, &a.id).cmp(&(&b.key, &b.id)));
            Ok(())
        }
    }
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        self.sessions.clear();
        let _ = self.manager.callback(7, self.session_sink.0);
        let _ = self.endpoint.callback(4, self.endpoint_sink.0);
    }
}
struct Backend {
    enumerator: Com,
    sink: Subscription,
    wake: Arc<Wake>,
    endpoints: Vec<Endpoint>,
    routing: Option<policy::Routing>,
    default: Option<String>,
    config: Config,
    seen_keys: HashSet<String>,
    applied: HashMap<(u32, String), Option<String>>,
    processes: HashMap<u32, TrackedProcess>,
    startup: bool,
    dirty: Option<Instant>,
    error: Option<String>,
}
impl Backend {
    fn new(wake: &Arc<Wake>) -> Result<Self, String> {
        let enumerator = Com::create(&ENUMERATOR_CLASS, &ENUMERATOR)?;
        let sink = Subscription::new(DEVICE_NOTIFY, wake);
        enumerator.callback(6, sink.0)?;
        let (routing, error) = match policy::Routing::new() {
            Ok(routing) => (Some(routing), None),
            Err(error) => {
                config::log(&error);
                (None, Some(error))
            }
        };
        let mut backend = Self {
            enumerator,
            sink,
            wake: wake.clone(),
            endpoints: vec![],
            routing,
            default: None,
            config: Config::load(),
            seen_keys: HashSet::new(),
            applied: HashMap::new(),
            processes: HashMap::new(),
            startup: crate::platform::startup_enabled(),
            dirty: None,
            error,
        };
        backend.rebuild()?;
        if let Some(saved) = backend.config.default_device.clone() {
            if backend.default.as_ref() != Some(&saved) && backend.endpoints.iter().any(|d| d.id == saved) {
                if let Err(error) = policy::set_default(&saved) { backend.failure(error); }
                backend.read_default();
            }
        } else { backend.config.default_device = backend.default.clone(); }
        backend.mark_dirty();
        backend.read_values(STRUCTURE)?;
        if let Err(error) = backend.enforce_routes(false) {
            backend.failure(error);
        }
        Ok(backend)
    }
    fn read_default(&mut self) {
        unsafe {
            let get: unsafe extern "system" fn(Raw, i32, i32, *mut Raw) -> Hr =
                self.enumerator.method(4);
            let mut ptr = null_mut();
            self.default = if get(self.enumerator.0, 0, 0, &mut ptr) >= 0 {
                Com::from_result(ptr).ok().and_then(|d| d.string(5).ok())
            } else {
                None
            };
        }
    }
    fn rebuild(&mut self) -> Result<(), String> {
        unsafe {
            let enumerate: unsafe extern "system" fn(Raw, i32, u32, *mut Raw) -> Hr =
                self.enumerator.method(3);
            let mut ptr = null_mut();
            check(enumerate(self.enumerator.0, 0, 1, &mut ptr), "枚举播放设备")?;
            let collection = Com::from_result(ptr)?;
            let count = collection.number(3)?;
            let mut old: HashMap<_, _> = std::mem::take(&mut self.endpoints)
                .into_iter()
                .map(|d| (d.id.clone(), d))
                .collect();
            for i in 0..count {
                let get: unsafe extern "system" fn(Raw, u32, *mut Raw) -> Hr = collection.method(4);
                let mut ptr = null_mut();
                if get(collection.0, i, &mut ptr) < 0 {
                    continue;
                }
                let device = Com::from_result(ptr)?;
                let id = device.string(5)?;
                let mut endpoint = match old.remove(&id) {
                    Some(mut endpoint) => {
                        if let Ok(name) = device_name(&device) {
                            endpoint.name = name;
                        }
                        endpoint
                    }
                    None => match Endpoint::new(device, &self.wake) {
                        Ok(endpoint) => endpoint,
                        Err(error) => {
                            config::log(&error);
                            self.error = Some(error);
                            continue;
                        }
                    },
                };
                if let Err(error) = endpoint.enumerate_sessions(&self.wake) {
                    config::log(&error);
                    self.error = Some(error);
                }
                self.endpoints.push(endpoint);
            }
        }
        self.endpoints
            .sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
        self.read_default();
        for (device, control) in self.wake.take_sessions() {
            if let Some(endpoint) = self.endpoints.iter_mut().find(|d| d.id == device) {
                if let Ok(control) = control.query(&SESSION_CONTROL) {
                    if let Ok(id) = control.string(13) {
                        if !endpoint.sessions.iter().any(|s| s.id == id)
                            && control.number(3).unwrap_or(2) != 2
                        {
                            match Session::new(control, &self.wake) {
                                Ok(session) => endpoint.sessions.push(session),
                                Err(error) => config::log(&error),
                            }
                        }
                    }
                }
            }
        }
        self.processes.retain(|_, process| process.alive());
        for session in self
            .endpoints
            .iter()
            .flat_map(|d| &d.sessions)
            .filter(|s| s.pid != 0)
        {
            if !self.processes.contains_key(&session.pid) {
                unsafe {
                    let handle = OpenProcess(0x1000, 0, session.pid);
                    if handle != 0 {
                        self.processes.insert(
                            session.pid,
                            TrackedProcess {
                                handle,
                                key: session.key.clone(),
                            },
                        );
                    }
                }
            }
        }
        let active: HashSet<_> = self
            .processes
            .iter()
            .map(|(pid, p)| (*pid, p.key.clone()))
            .collect();
        self.applied.retain(|key, _| active.contains(key));
        Ok(())
    }
    fn mark_dirty(&mut self) {
        self.dirty = Some(Instant::now() + Duration::from_millis(400));
    }
    fn failure(&mut self, error: String) {
        config::log(&error);
        self.error = Some(error);
    }
    fn read_values(&mut self, flags: u32) -> Result<(), String> {
        let mut changed = false;
        for endpoint in &mut self.endpoints {
            let master = endpoint.endpoint.scalar(9)?;
            endpoint.muted = endpoint.endpoint.number(15)? != 0;
            if !endpoint.mute_loaded {
                if !self.config.load_failed {
                    if let Some(&desired) = self.config.device_mutes.get(&endpoint.id) {
                        if endpoint.muted != desired { endpoint.endpoint.set_mute(14,desired)?; endpoint.muted = desired; }
                    }
                }
                endpoint.mute_loaded = true;
            }
            for session in &mut endpoint.sessions {
                let scalar = session.volume.scalar(4)?;
                session.muted = session.volume.number(6)? != 0;
                let key = session.key.clone();
                if session.needs_apply && !self.config.load_failed {
                    if let Some(&desired) = self.config.app_mutes.get(&key) {
                        if session.muted != desired { session.volume.set_mute(5,desired)?; session.muted = desired; }
                    }
                }
                if !self.config.volumes.contains_key(&key) {
                    self.config.volumes.insert(
                        key.clone(),
                        VolumePreference {
                            target: 100,
                        },
                    );
                    changed = true;
                }
                if !session.needs_apply && flags & SESSION_VOLUME != 0 && (scalar - session.scalar).abs() > 0.0001 {
                    self.config.volumes.insert(
                        key.clone(),
                        VolumePreference {
                            target: (scalar * 100.0).round() as u8,
                        },
                    );
                    changed = true;
                }
                session.scalar = scalar;
                if !session.muted && session.needs_apply && !self.config.load_failed {
                    let preference = self.config.volumes[&key];
                    let desired = preference.target as f32 / 100.0;
                    if (desired - scalar).abs() > 0.0001 {
                        session.volume.set_scalar(3, desired)?;
                        session.scalar = desired;
                    }
                }
                session.needs_apply = false;
            }
            endpoint.scalar = master;
        }
        if changed {
            self.mark_dirty();
        }
        Ok(())
    }
    fn enforce_routes(&mut self, clear_unpinned: bool) -> Result<(), String> {
        let Some(routing) = &self.routing else {
            return Ok(());
        };
        let active: HashSet<_> = self.endpoints.iter().map(|d| d.id.clone()).collect();
        let mut processes = BTreeMap::new();
        for (pid, process) in &self.processes {
            processes.insert((*pid, process.key.clone()), ());
        }
        for endpoint in &self.endpoints {
            for session in &endpoint.sessions {
                if session.pid != 0 {
                    processes.insert((session.pid, session.key.clone()), ());
                }
            }
        }
        let mut imported = false;
        let mut error = None;
        for ((pid, key), _) in processes {
            if self.seen_keys.insert(key.clone())
                && !clear_unpinned && !self.config.managed_default
                && !self.config.routes.contains_key(&key)
            {
                if let Some(device) = routing.get(pid) {
                    self.config.routes.insert(key.clone(), device);
                    imported = true;
                }
            }
            let desired = self.config.routes.get(&key).and_then(|id| {
                if active.contains(id) {
                    Some(id.clone())
                } else {
                    self.default
                        .clone()
                        .or_else(|| self.endpoints.first().map(|d| d.id.clone()))
                }
            });
            let cache_key = (pid, key);
            if self.applied.get(&cache_key) == Some(&desired) {
                continue;
            }
            if desired.is_none() && self.default.is_none() {
                continue;
            }
            match routing.set(pid, desired.as_deref()) {
                Ok(()) => {
                    self.applied.insert(cache_key, desired);
                }
                Err(problem) => {
                    if error.is_none() {
                        error = Some(problem);
                    }
                }
            }
        }
        if imported {
            self.mark_dirty();
        }
        if let Some(error) = error {
            Err(error)
        } else {
            Ok(())
        }
    }
    fn command(&mut self, command: Command) -> Result<(), String> {
        self.error = None;
        match command {
            Command::Volume { device, app, value } => {
                if let Some(key) = app {
                    let mut sessions: Vec<_> = self.endpoints.iter_mut().flat_map(|d| d.sessions.iter_mut()).filter(|s| s.key == key).collect();
                    if sessions.is_empty() { return Err("Audio session ended".into()); }
                    let target = value.min(100);
                    for session in &mut sessions {
                        session.volume.set_scalar(3,target as f32 / 100.0)?;
                        session.scalar = target as f32 / 100.0;
                        session.volume.set_mute(5,false)?;
                        session.muted = false;
                    }
                    if self.config.app_mutes.contains_key(&key) { self.config.app_mutes.insert(key.clone(),false); }
                    self.config.volumes.insert(key,VolumePreference { target });
                    self.mark_dirty();
                } else {
                    let endpoint = self.endpoints.iter_mut().find(|d| d.id == device).ok_or("Device disconnected")?;
                    endpoint.endpoint.set_scalar(7,value as f32 / 100.0)?;
                    endpoint.endpoint.set_mute(14,false)?;
                    if self.config.device_mutes.contains_key(&device) {
                        self.config.device_mutes.insert(device,false);
                        self.mark_dirty();
                    }
                }
                self.read_values(ENDPOINT)?;
            }
            Command::Mute { device, app, muted } => {
                if let Some(key) = app {
                    let mut found = false;
                    for session in self.endpoints.iter_mut().flat_map(|d| d.sessions.iter_mut()).filter(|s| s.key == key) {
                        session.volume.set_mute(5,muted)?;
                        session.muted = muted;
                        found = true;
                    }
                    if !found { return Err("Audio session ended".into()); }
                    self.config.app_mutes.insert(key,muted);
                } else {
                    let endpoint = self.endpoints.iter_mut().find(|d| d.id == device).ok_or("Device disconnected")?;
                    endpoint.endpoint.set_mute(14,muted)?;
                    self.config.device_mutes.insert(device,muted);
                }
                self.mark_dirty();
                self.read_values(0)?;
            }
            Command::Default(device) => {
                if !self.endpoints.iter().any(|d| d.id == device) {
                    return Err("播放设备已断开".into());
                }
                policy::set_default(&device)?;
                self.read_default();
                self.config.default_device = self.default.clone();
                self.config.managed_default = true;
                self.config.routes.retain(|key,_| self.config.explicit_routes.contains(key));
                self.mark_dirty();
                self.applied.clear();
                self.enforce_routes(true)?;
            }
            Command::Route { app, device } => {
                if app == "__system_sounds__" {
                    return Err("系统声音由 Windows 管理，跟随默认播放设备".into());
                }
                let routing = self
                    .routing
                    .as_ref()
                    .ok_or("当前 Windows 版本不支持应用输出路由，请使用系统音量设置")?;
                if device
                    .as_ref()
                    .is_some_and(|id| !self.endpoints.iter().any(|d| &d.id == id))
                {
                    return Err("播放设备已断开".into());
                }
                let pids: HashSet<_> = self
                    .endpoints
                    .iter()
                    .flat_map(|d| d.sessions.iter())
                    .filter(|s| s.key == app && s.pid != 0)
                    .map(|s| s.pid)
                    .collect();
                if pids.is_empty() {
                    return Err("应用音频会话已结束，请重新播放后再选择设备".into());
                }
                for pid in pids {
                    routing.set(pid, device.as_deref())?;
                }
                if let Some(device) = device {
                    self.config.explicit_routes.insert(app.clone());
                    self.config.routes.insert(app.clone(), device);
                } else {
                    self.config.explicit_routes.remove(&app);
                    self.config.routes.remove(&app);
                }
                self.seen_keys.insert(app.clone());
                self.applied.retain(|(_, key), _| key != &app);
                self.mark_dirty();
                self.rebuild()?;
                self.enforce_routes(false)?;
            }
            Command::Startup(enabled) => {
                crate::platform::set_startup(enabled)?;
                self.startup = enabled;
            }
            Command::Refresh => {
                self.rebuild()?;
                self.read_values(STRUCTURE)?;
                self.enforce_routes(false)?;
            }
            Command::HideApp { key, name } => {
                if !self.config.hidden_apps.contains_key(&key) {
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                        .map(|time| time.as_millis().min(u64::MAX as u128) as u64).unwrap_or(0);
                    let last = self.config.hidden_apps.values().map(|app| app.hidden_at).max().unwrap_or(0);
                    let hidden_at = now.max(last.saturating_add(1));
                    self.config.hidden_apps.insert(key.clone(),crate::model::HiddenApp { key,name,hidden_at });
                    self.mark_dirty();
                }
            }
            Command::ShowApp(key) => {
                if self.config.hidden_apps.remove(&key).is_some() {
                    self.mark_dirty();
                    self.rebuild()?;
                    self.read_values(STRUCTURE)?;
                }
            }
            Command::Meter(_) | Command::Shutdown => {}
        }
        Ok(())
    }
    fn snapshot(&self) -> Model {
        let mut devices = vec![];
        let mut ordered: Vec<_> = self.endpoints.iter().collect();
        ordered.sort_by_key(|endpoint| self.default.as_ref() == Some(&endpoint.id));
        let default = self.default.as_ref().and_then(|id| ordered.iter().position(|d| &d.id == id)).unwrap_or(0);
        let mut groups: BTreeMap<&str,Vec<&Session>> = BTreeMap::new();
        for endpoint in &ordered {
            for session in &endpoint.sessions { groups.entry(&session.key).or_default().push(session); }
            devices.push(Device { id: endpoint.id.clone(), name: endpoint.name.clone(),
                volume: (endpoint.scalar*100.0).round() as u8, peak: 0.0, muted: endpoint.muted,
                collapsed: false, apps: vec![] });
        }
        for (key,sessions) in groups {
            if self.config.hidden_apps.contains_key(key) { continue; }
            let preferred_route = if key == "__system_sounds__" { None } else { self.config.routes.get(key).cloned() };
            let route = preferred_route.as_ref().and_then(|id| ordered.iter().position(|d| &d.id == id));
            let target = route.unwrap_or(default);
            let representative = ordered.get(target).and_then(|d| d.sessions.iter().find(|s| s.key == key)).unwrap_or(sessions[0]);
            let muted = sessions.iter().all(|s| s.muted);
            if let Some(device) = devices.get_mut(target) {
                device.apps.push(App { key: key.into(), name: representative.name.clone(), icon_path: representative.path.clone(),
                    volume: if muted {0} else {(representative.scalar*100.0).round().clamp(0.0,100.0) as u8}, peak: 0.0, muted,
                    icon:0,route,preferred_route });
            }
        }
        for device in &mut devices { device.apps.sort_by(|a,b| (&a.name,&a.key).cmp(&(&b.name,&b.key))); }
        let mut hidden_apps: Vec<_> = self.config.hidden_apps.values().cloned().collect();
        hidden_apps.sort_by_key(|app| app.hidden_at);
        Model {
            hidden_apps,
            default: self
                .default
                .as_ref()
                .and_then(|id| devices.iter().position(|d| &d.id == id)),
            scenario: if devices.is_empty() {
                Scenario::Empty
            } else {
                Scenario::Normal
            },
            devices,
            startup: self.startup,
            routing_available: self.routing.is_some(),
            error: self.error.clone(),
        }
    }
    fn flush(&mut self) {
        if self.dirty.take().is_some() {
            if let Err(error) = self.config.save() {
                self.failure(format!("配置保存失败：{error}"));
            }
        }
    }
    fn peaks(&self) -> HashMap<String, f32> {
        let mut values = HashMap::new();
        for endpoint in &self.endpoints {
            // Endpoint meters report before master attenuation; scale to output level.
            let peak = if endpoint.muted { 0.0 } else {
                endpoint.meter.as_ref().and_then(|meter| meter.scalar(3).ok())
                    .filter(|peak| peak.is_finite()).unwrap_or(0.0).clamp(0.0,1.0)*endpoint.scalar
            };
            values.insert(format!("device:{}", endpoint.id), peak);
            for session in &endpoint.sessions {
                let value = if endpoint.muted || session.muted || session.control.number(3).unwrap_or(0) != 1 {
                    0.0
                } else {
                    session.meter.as_ref().and_then(|meter| meter.scalar(3).ok())
                        .filter(|peak| peak.is_finite()).unwrap_or(0.0).clamp(0.0, 1.0)
                };
                let peak = values.entry(session.key.clone()).or_insert(0.0_f32);
                *peak = peak.max(value);
            }
        }
        values
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        self.flush();
        self.endpoints.clear();
        let _ = self.enumerator.callback(7, self.sink.0);
    }
}
fn worker(receiver: mpsc::Receiver<Command>, wake: Arc<Wake>, updates: Arc<Updates>) {
    unsafe {
        let hr = CoInitializeEx(null_mut(), 0);
        if hr < 0 {
            let mut model = Model::new();
            model.scenario = Scenario::Error;
            model.error = Some(format!("初始化音频服务失败（0x{:08X}）", hr as u32));
            updates.publish(model);
            return;
        }
    }
    let mut backend = match Backend::new(&wake) {
        Ok(backend) => {
            updates.publish(backend.snapshot());
            Some(backend)
        }
        Err(error) => {
            config::log(&error);
            let mut model = Model::new();
            model.scenario = Scenario::Error;
            model.error = Some(error);
            updates.publish(model);
            None
        }
    };
    let mut metering = false;
    let mut next_peak = Instant::now();
    loop {
        let timeout = backend
            .as_ref()
            .and_then(|b| b.dirty)
            .map(|d| {
                d.saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(u32::MAX as u128 - 1) as u32
            })
            .unwrap_or(u32::MAX);
        let timeout = if metering { timeout.min(next_peak.saturating_duration_since(Instant::now()).as_millis().min(u32::MAX as u128) as u32) } else { timeout };
        unsafe {
            WaitForSingleObject(wake.event, timeout);
        }
        let flags = wake.flags.swap(0, Ordering::AcqRel);
        let mut commands: Vec<_> = receiver.try_iter().collect();
        commands.retain(|command| {
            if let Command::Meter(enabled) = command {
                metering = *enabled;
                next_peak = Instant::now();
                false
            } else { true }
        });
        if commands.iter().any(|c| matches!(c, Command::Shutdown)) {
            break;
        }
        if backend.is_none() && commands.iter().any(|c| matches!(c, Command::Refresh)) {
            match Backend::new(&wake) {
                Ok(value) => backend = Some(value),
                Err(error) => {
                    let mut model = Model::new();
                    model.scenario = Scenario::Error;
                    model.error = Some(error);
                    updates.publish(model);
                }
            }
        }
        let Some(backend) = &mut backend else {
            continue;
        };
        let mut publish = flags & !COMMAND != 0 || !commands.is_empty();
        if flags & (STRUCTURE | DEFAULT) != 0 {
            if let Err(error) = backend
                .rebuild()
                .and_then(|_| backend.enforce_routes(false))
            {
                backend.failure(error);
            }
        }
        if flags & DEFAULT != 0 {
            backend.config.default_device = backend.default.clone();
            backend.mark_dirty();
        }
        if flags & (STRUCTURE | DEFAULT | ENDPOINT | SESSION_VOLUME) != 0 {
            if let Err(error) = backend.read_values(flags) {
                backend.failure(error);
            }
        }
        for (i, command) in commands.iter().enumerate() {
            if let Command::Volume { device, app, .. } = command {
                if commands[i + 1..].iter().take_while(|c| matches!(c, Command::Volume { .. })).any(|c| matches!(c, Command::Volume { device: d, app: a, .. } if d == device && a == app)) { continue; }
            }
            if let Err(error) = backend.command(command.clone()) {
                backend.failure(error);
                let _ = backend.read_values(0);
            }
        }
        if backend
            .dirty
            .is_some_and(|deadline| deadline <= Instant::now())
        {
            backend.flush();
            publish = true;
        }
        if publish {
            updates.publish(backend.snapshot());
        }
        if metering && Instant::now() >= next_peak {
            updates.publish_peaks(backend.peaks());
            next_peak = Instant::now()+Duration::from_millis(40);
        }
    }
    wake.stop_sessions();
    drop(backend);
    drop(wake.take_sessions());
    unsafe {
        CoUninitialize();
    }
}
