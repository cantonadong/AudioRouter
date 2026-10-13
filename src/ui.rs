use crate::audio::{AudioClient, Command};
use crate::model::{Model, Scenario};
use crate::paint::{self, BG, BLUE, Canvas, DEVICE_BG, MENU_BG, MUTED, WHITE};
use crate::win32::*;
use std::collections::{HashMap, HashSet};

pub const PANEL_WIDTH: i32 = 360;
pub const TOP: i32 = 0;
pub const ROW: i32 = 44;
const FOOTER: i32 = 0;
const HEADER: i32 = 98;
const SETTINGS_HEIGHT: i32 = 372;

#[derive(Clone, PartialEq, Eq)]
pub enum Action {
    Collapse(usize),
    Slide(usize, Option<usize>),
    Mute(usize, Option<usize>),
    Default(usize),
    Route(usize, usize, usize, bool),
    AppMenu(usize, usize),
    HideApp(usize, usize),
    ShowApp(String),
    DeviceMenu,
    Settings,
    Panel,
    OldSettings,
    SystemSettings(Option<u8>),
    Startup,
    Refresh,
    Hide,
    Exit,
}
pub struct Hit {
    pub rect: Rect,
    pub action: Action,
    pub hint: String,
}
pub struct MenuItem {
    pub label: String,
    pub action: Option<Action>,
    pub checked: bool,
    pub separator: bool,
    pub arrow: bool,
}
impl MenuItem {
    fn row(label: &str, action: Action) -> Self {
        Self {
            label: label.into(),
            action: Some(action),
            checked: false,
            separator: false,
            arrow: false,
        }
    }
    fn separator() -> Self {
        Self {
            label: String::new(),
            action: None,
            checked: false,
            separator: true,
            arrow: false,
        }
    }
    fn label(label: &str) -> Self {
        Self {
            label: label.into(),
            action: None,
            checked: false,
            separator: false,
            arrow: false,
        }
    }
}
pub struct Menu {
    pub items: Vec<MenuItem>,
    pub width: i32,
    pub requested_width: i32,
    pub viewport_height: i32,
    pub scroll: i32,
    pub selected: Option<usize>,
}
impl Menu {
    pub fn height(&self) -> i32 {
        12 + self
            .items
            .iter()
            .map(|i| if i.separator { 12 } else { 34 })
            .sum::<i32>()
    }
    pub fn index_at(&self, p: Point) -> Option<usize> {
        if p.x < 4 || p.x >= self.width - 4 || p.y < 6 || p.y >= self.viewport_height - 6 {
            return None;
        }
        let mut y = 6 - self.scroll;
        for (index, item) in self.items.iter().enumerate() {
            let height = if item.separator { 12 } else { 34 };
            if p.y >= y && p.y < y + height {
                return if item.separator { None } else { Some(index) };
            }
            y += height;
        }
        None
    }
    pub fn max_scroll(&self) -> i32 {
        (self.height() - self.viewport_height).max(0)
    }
    pub fn reveal_selected(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        let top = 6 + self.items[..index]
            .iter()
            .map(|i| if i.separator { 12 } else { 34 })
            .sum::<i32>();
        if top - self.scroll < 6 {
            self.scroll = top - 6;
        }
        if top + 34 - self.scroll > self.viewport_height - 6 {
            self.scroll = top + 40 - self.viewport_height;
        }
        self.scroll = self.scroll.clamp(0, self.max_scroll());
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Panel,
    Settings,
}
pub struct Ui {
    pub hwnd: Handle,
    pub popup: Handle,
    pub instance: Handle,
    pub icon: Handle,
    pub volume_icons: [Handle; 4],
    pub tray_icons: [Handle; 4],
    pub wheel_remainder: i32,
    pub pending_volume: Option<Command>,
    pub last_volume_sent: std::time::Instant,
    pub tray_level: Option<(usize, u8)>,
    pub tray_hook: Handle,
    pub tray_wheel_remainder: i32,
    pub tooltip: Handle,
    pub tooltip_name: String,
    pub tooltip_display: String,
    pub tooltip_start: Option<std::time::Instant>,
    pub tooltip_ready: bool,
    pub audio: Option<AudioClient>,
    pub pending_model: Option<Model>,
    pub icons: HashMap<String, usize>,
    pub model: Model,
    pub scale: f64,
    pub popup_scale: f64,
    pub width: i32,
    pub height: i32,
    pub scroll: i32,
    pub view: View,
    pub hits: Vec<Hit>,
    pub hovered: Option<Action>,
    pub context_app: Option<(usize, usize)>,
    pub display_collapses: HashMap<String, bool>,
    pub focused: Option<Action>,
    pub dragging: Option<(usize, Option<usize>)>,
    pub menu: Option<Menu>,
    pub menu_opening: bool,
    pub tray_present: bool,
    pub taskbar_message: u32,
    pub show_message: u32,
    pub hint: String,
}
pub enum Effect {
    None,
    Resize,
    Show,
    Hide,
    Menu(Action),
    Capture,
    Release,
    Quit,
    OpenSettings(Option<u8>),
    CloseMenu,
}

impl Ui {
    pub fn new(instance: Handle, icon: Handle) -> Self {
        Self {
            hwnd: 0,
            popup: 0,
            instance,
            icon,
            volume_icons: [0; 4],
            tray_icons: [0; 4],
            wheel_remainder: 0,
            pending_volume: None,
            last_volume_sent: std::time::Instant::now(),
            tray_level: None,
            tray_hook: 0,
            tray_wheel_remainder: 0,
            tooltip: 0,
            tooltip_name: String::new(),
            tooltip_display: String::new(),
            tooltip_start: None,
            tooltip_ready: false,
            audio: None,
            pending_model: None,
            icons: HashMap::new(),
            model: Model::new(),
            scale: 1.0,
            popup_scale: 1.0,
            width: PANEL_WIDTH,
            height: 280,
            scroll: 0,
            view: View::Panel,
            hits: vec![],
            hovered: None,
            context_app: None,
            display_collapses: HashMap::new(),
            focused: None,
            dragging: None,
            menu: None,
            menu_opening: false,
            tray_present: false,
            taskbar_message: 0,
            show_message: 0,
            hint: "正在读取音频设备…".into(),
        }
    }
    pub fn send(&self, command: Command) {
        if let Some(audio) = &self.audio {
            audio.send(command);
        }
    }

    pub fn apply_model(&mut self, mut model: Model) -> bool {
        for device in &mut model.devices {
            device.peak = if device.muted { 0.0 } else {
                self.model.devices.iter().find(|old| old.id == device.id).map(|old| old.peak).unwrap_or(0.0)
            };
            device.collapsed = self.display_collapses.get(&device.id).copied().unwrap_or(device.apps.is_empty());
        }
        let old_structure: Vec<_> = self
            .model
            .devices
            .iter()
            .map(|d| {
                (
                    d.id.clone(),
                    d.collapsed,
                    d.apps.iter().map(|a| a.key.clone()).collect::<Vec<_>>(),
                )
            })
            .collect();
        let new_structure: Vec<_> = model
            .devices
            .iter()
            .map(|d| {
                (
                    d.id.clone(),
                    d.collapsed,
                    d.apps.iter().map(|a| a.key.clone()).collect::<Vec<_>>(),
                )
            })
            .collect();
        let structure_changed =
            old_structure != new_structure || self.model.scenario != model.scenario;
        let mut used = HashSet::new();
        for device in &mut model.devices {
            for app in &mut device.apps {
                app.peak = if app.muted { 0.0 } else {
                    self.model.devices.iter().flat_map(|d| d.apps.iter())
                        .find(|old| old.key == app.key).map(|old| old.peak).unwrap_or(0.0)
                };
                let path = if app.name.to_ascii_lowercase().contains("audiodg") || app.icon_path.to_ascii_lowercase().ends_with("audiodg.exe") {
                    "__blank_application__".to_string()
                } else if app.key == "__system_sounds__" {
                    "C:/Windows/System32/mmsys.cpl".to_string()
                } else {
                    app.icon_path.clone()
                };
                let path = if path.is_empty() { "__blank_application__".to_string() } else { path };
                if !path.is_empty() {
                    app.icon = *self
                        .icons
                        .entry(path.clone())
                        .or_insert_with(|| extract_icon(&path));
                    used.insert(path);
                }
            }
        }
        self.icons.retain(|path, icon| {
            if used.contains(path) {
                true
            } else {
                if *icon != 0 {
                    unsafe {
                        DestroyIcon(*icon as isize);
                    }
                }
                false
            }
        });
        if structure_changed {
            self.focused = None;
            self.hovered = None;
        }
        self.hint = model
            .error
            .clone()
            .unwrap_or_else(|| "右键应用选择输出设备".into());
        self.model = model;
        self.relayout();
        structure_changed
    }
    pub fn reset_collapses(&mut self) {
        self.display_collapses.clear();
        for device in &mut self.model.devices { device.collapsed = device.apps.is_empty(); }
        self.relayout();
    }
    pub fn apply_peaks(&mut self, peaks: HashMap<String,f32>) -> bool {
        let mut changed = false;
        for device in &mut self.model.devices {
            let value = if device.muted { 0.0 } else {
                peaks.get(&format!("device:{}", device.id)).copied().unwrap_or(0.0).min(device.volume as f32/100.0)
            };
            let next = if value <= 0.0001 { 0.0 } else { value.max(device.peak*0.72) };
            if (next-device.peak).abs() > 0.002 { changed = true; }
            device.peak = next;
            for app in &mut device.apps {
                let value = if app.muted || device.muted { 0.0 } else {
                    peaks.get(&app.key).copied().unwrap_or(0.0).min(app.volume as f32/100.0)
                };
                let next = if value <= 0.0001 { 0.0 } else { value.max(app.peak*0.72) };
                if (next-app.peak).abs() > 0.002 { changed = true; }
                app.peak = next;
            }
        }
        changed
    }
    pub fn desired_height(&self) -> i32 {
        if self.view == View::Settings {
            SETTINGS_HEIGHT
        } else if self.model.devices.is_empty() {
            264
        } else {
            TOP + FOOTER + self.content_height()
        }
    }
    pub fn content_height(&self) -> i32 {
        self.model
            .devices
            .iter()
            .map(|d| {
                HEADER
                    + if d.collapsed {
                        0
                    } else if d.apps.is_empty() {
                        32
                    } else {
                        d.apps.len() as i32 * ROW
                    }
            })
            .sum()
    }
    pub fn max_scroll(&self) -> i32 {
        if self.view == View::Settings {
            (SETTINGS_HEIGHT - self.height).max(0)
        } else {
            (self.content_height() - (self.height - TOP - FOOTER)).max(0)
        }
    }
    fn push(&mut self, rect: Rect, action: Action, hint: &str) {
        self.hits.push(Hit {
            rect,
            action,
            hint: hint.into(),
        });
    }
    pub fn relayout(&mut self) {
        self.scroll = self.scroll.clamp(0, self.max_scroll());
        self.hits.clear();
        if self.view == View::Settings {
            for (y, action, hint) in [
                (98, Action::DeviceMenu, "选择默认播放设备"),
                (152, Action::SystemSettings(None), "Windows 新版音量设置"),
                (194, Action::OldSettings, "Windows 旧版声音设置"),
                (248, Action::Startup, "开机自动启动"),
                (312, Action::Panel, "返回音量面板"),
            ] {
                self.push(
                    Rect::new(14, y - self.scroll, self.width - 28, 38),
                    action,
                    hint,
                );
            }
            return;
        }
        if self.model.scenario == Scenario::Error {
            self.push(
                Rect::new(self.width / 2 - 56, 188, 112, 34),
                Action::Refresh,
                "重新读取真实音频设备",
            );
        }
        let mut y = TOP - self.scroll;
        // Build hits independently of drawing; their stable identities are resolved before commands leave the UI.
        for di in 0..self.model.devices.len() {
            let name = self.model.devices[di].name.clone();
            self.push(
                Rect::new(3, y, self.width - 6, 48),
                Action::Collapse(di),
                &name,
            );
            self.push(
                Rect::new(16, y + 50, 42, ROW),
                Action::Mute(di, None),
                &format!("{name} · 静音 / 恢复"),
            );
            self.push(
                Rect::new(60, y + 50, self.width - 70, ROW),
                Action::Slide(di, None),
                &format!("{name} · 主音量"),
            );
            y += HEADER;
            if !self.model.devices[di].collapsed {
                if self.model.devices[di].apps.is_empty() {
                    y += 32;
                }
                for ai in 0..self.model.devices[di].apps.len() {
                    let app = &self.model.devices[di].apps[ai];
                    let hint = format!(
                        "{} · {} · 右键更改输出",
                        app.name,
                        "\u{72ec}\u{7acb}\u{97f3}\u{91cf}"
                    );
                    self.push(Rect::new(16, y, 42, ROW), Action::Mute(di, Some(ai)), &hint);
                    self.push(
                        Rect::new(60, y, self.width - 70, ROW),
                        Action::Slide(di, Some(ai)),
                        &hint,
                    );
                    y += ROW;
                }
            }
        }
    }
    pub fn hit(&self, p: Point) -> Option<&Hit> {
        self.hits.iter().find(|hit| {
            if hit.rect.top != 0 && p.y < TOP {
                return false;
            }
            if matches!(
                hit.action,
                Action::Collapse(_) | Action::Slide(..) | Action::Mute(..)
            ) && p.y >= self.height - FOOTER
            {
                return false;
            }
            hit.rect.contains(p)
        })
    }
    fn set_volume(&mut self, d: usize, a: Option<usize>, value: u8) {
        let Some(device) = self.model.devices.get(d) else {
            return;
        };
        let app = match a {
            Some(a) => match device.apps.get(a) {
                Some(app) => Some(app.key.clone()),
                None => return,
            },
            None => None,
        };
        let current = a.and_then(|i| device.apps.get(i)).map(|a| a.volume).unwrap_or(device.volume);
        let value = value.min(100);
        let muted = a.and_then(|i| device.apps.get(i)).map(|a| a.muted).unwrap_or(device.muted);
        if current == value && !muted { return; }
        let command = Command::Volume { device: device.id.clone(), app, value };
        self.model.set_volume(d, a, value);
        if self.dragging.is_some() {
            self.pending_volume = Some(command);
            if self.last_volume_sent.elapsed().as_millis() >= 16 { self.flush_volume(); }
        } else { self.send(command); }
    }
    pub fn flush_volume(&mut self) {
        if let Some(command) = self.pending_volume.take() {
            self.send(command);
            self.last_volume_sent = std::time::Instant::now();
        }
    }
    pub fn tray_wheel_volume(&mut self, delta: i32) {
        let Some(d) = self.model.default else { return; };
        self.tray_wheel_remainder += delta;
        let steps = self.tray_wheel_remainder / 120;
        self.tray_wheel_remainder %= 120;
        if steps == 0 { return; }
        let Some(device) = self.model.devices.get(d) else { return; };
        let value = (device.volume as i32+steps*2).clamp(0,100) as u8;
        self.set_volume(d,None,value);
    }
    pub fn wheel_volume(&mut self, point: Point, delta: i32) -> bool {
        let target = match self.hit(point).map(|hit| hit.action.clone()) {
            Some(Action::Slide(d, a) | Action::Mute(d, a)) => Some((d, a)),
            Some(Action::Collapse(d)) => Some((d, None)),
            _ => None,
        };
        let Some((d, a)) = target else { self.wheel_remainder = 0; return false; };
        let action = Action::Slide(d, a);
        if self.focused != Some(action.clone()) { self.wheel_remainder = 0; }
        self.focused = Some(action);
        self.wheel_remainder += delta;
        let steps = self.wheel_remainder / 120;
        self.wheel_remainder %= 120;
        if steps != 0 { self.adjust_focused(steps * 2, None); }
        true
    }

    pub fn volume_at(&mut self, x: i32) {
        if let Some((d, a)) = self.dragging {
            let value = (((x - 68) as f64 * 100.0 / (self.width - 130).max(1) as f64).round()
                as i32)
                .clamp(0, 100) as u8;
            self.set_volume(d, a, value);
        }
    }
    pub fn adjust_focused(&mut self, change: i32, absolute: Option<u8>) {
        let Some(Action::Slide(d, a)) = self.focused else {
            return;
        };
        let Some(device) = self.model.devices.get(d) else {
            return;
        };
        let value = if let Some(a) = a {
            device.apps.get(a).map(|a| a.volume).unwrap_or(0)
        } else {
            device.volume
        };
        self.set_volume(
            d,
            a,
            absolute.unwrap_or((value as i32 + change).clamp(0, 100) as u8),
        );
    }
    pub fn next_focus(&mut self, backwards: bool) {
        let sliders: Vec<_> = self
            .hits
            .iter()
            .filter(|h| matches!(h.action, Action::Slide(..)))
            .collect();
        if sliders.is_empty() {
            return;
        }
        let current = sliders
            .iter()
            .position(|h| Some(&h.action) == self.focused.as_ref());
        let index = match current {
            Some(i) if backwards => (i + sliders.len() - 1) % sliders.len(),
            Some(i) => (i + 1) % sliders.len(),
            None if backwards => sliders.len() - 1,
            None => 0,
        };
        let hit = sliders[index];
        self.focused = Some(hit.action.clone());
        if hit.rect.top < TOP {
            self.scroll = (self.scroll + hit.rect.top - TOP).max(0);
        } else if hit.rect.bottom > self.height - FOOTER {
            self.scroll += hit.rect.bottom - (self.height - FOOTER);
        }
        self.relayout();
    }
    pub fn activate(&mut self, action: Action) -> Effect {
        match action {
            Action::Collapse(d) => {
                if let Some(device) = self.model.devices.get_mut(d) {
                    device.collapsed = !device.collapsed;
                    self.display_collapses.insert(device.id.clone(), device.collapsed);
                }
                self.relayout();
                Effect::Resize
            }
            Action::Mute(d, a) => {
                let Some(device) = self.model.devices.get_mut(d) else {
                    return Effect::None;
                };
                let (app, muted) = if let Some(a) = a {
                    let Some(app) = device.apps.get_mut(a) else {
                        return Effect::None;
                    };
                    app.muted = !app.muted;
                    if app.muted { app.volume = 0; }
                    (Some(app.key.clone()), app.muted)
                } else {
                    device.muted = !device.muted;
                    (None, device.muted)
                };
                let id = device.id.clone();
                self.send(Command::Mute {
                    device: id,
                    app,
                    muted,
                });
                Effect::None
            }
            Action::Slide(d, a) => {
                self.focused = Some(Action::Slide(d, a));
                self.dragging = Some((d, a));
                Effect::Capture
            }
            Action::Default(d) => {
                if let Some(device) = self.model.devices.get(d) {
                    self.send(Command::Default(device.id.clone()));
                    self.hint = "正在切换默认播放设备…".into();
                }
                self.focused = None;
                Effect::None
            }
            Action::Route(d, a, target, explicit) => {
                if let Some(app) = self.model.devices.get(d).and_then(|d| d.apps.get(a)) {
                    let device = if explicit {
                        self.model.devices.get(target).map(|d| d.id.clone())
                    } else {
                        None
                    };
                    self.send(Command::Route {
                        app: app.key.clone(),
                        device,
                    });
                    self.hint = "正在更改应用输出设备…".into();
                }
                self.focused = None;
                Effect::None
            }
            Action::Settings => {
                self.view = View::Settings;
                self.scroll = 0;
                self.relayout();
                Effect::Resize
            }
            Action::Panel => {
                self.view = View::Panel;
                self.scroll = 0;
                self.relayout();
                Effect::Show
            }
            Action::HideApp(d,a) => {
                if let Some(app) = self.model.devices.get(d).and_then(|device| device.apps.get(a)) {
                    self.send(Command::HideApp { key: app.key.clone(), name: app.name.clone() });
                }
                Effect::None
            }
            Action::ShowApp(key) => {
                self.send(Command::ShowApp(key));
                Effect::None
            }
            Action::Startup => {
                self.send(Command::Startup(!self.model.startup));
                Effect::None
            }
            Action::Refresh => {
                self.send(Command::Refresh);
                self.hint = "正在刷新…".into();
                Effect::None
            }
            Action::SystemSettings(page) => Effect::OpenSettings(page),
            Action::Hide => Effect::Hide,
            Action::Exit => Effect::Quit,
            action => Effect::Menu(action),
        }
    }
    pub fn make_menu(&self, kind: Action) -> Menu {
        let mut items = vec![];
        let mut width = 330;
        match kind {
            Action::OldSettings => {
                width = 228;
                for (page, label) in ["播放设备", "录制设备", "声音", "通信"].iter().enumerate()
                {
                    items.push(MenuItem::row(
                        label,
                        Action::SystemSettings(Some(page as u8)),
                    ));
                }
            }
            Action::AppMenu(d, a) => {
                if let Some(app) = self.model.devices.get(d).and_then(|d| d.apps.get(a)) {
                    items.push(MenuItem::label(&format!("{} · 输出设备", app.name)));
                    items.push(MenuItem::separator());
                    if app.key == "__system_sounds__" {
                        items.push(MenuItem::label("系统声音跟随 Windows 默认设备"));
                    } else if !self.model.routing_available {
                        items.push(MenuItem::row(
                            "打开系统音量设置",
                            Action::SystemSettings(None),
                        ));
                    } else {
                        let mut item =
                            MenuItem::row("跟随默认播放设备", Action::Route(d, a, 0, false));
                        item.checked = app.preferred_route.is_none();
                        items.push(item);
                        for (target, device) in self.model.devices.iter().enumerate() {
                            let mut item =
                                MenuItem::row(&device.name, Action::Route(d, a, target, true));
                            item.checked = app.preferred_route.as_ref() == Some(&device.id);
                            items.push(item);
                        }
                        if app.preferred_route.is_some() && app.route.is_none() {
                            items.push(MenuItem::label("指定设备已断开，重连后自动恢复"));
                        }
                    }
                }
            }
            _ => {
                for (d, device) in self.model.devices.iter().enumerate() {
                    let mut item = MenuItem::row(&device.name, Action::Default(d));
                    item.checked = self.model.default == Some(d);
                    items.push(item);
                }
                if self.model.devices.is_empty() {
                    items.push(MenuItem::label("没有可用的播放设备"));
                }
                items.push(MenuItem::separator());
                let mut old = MenuItem::row("Windows 旧版声音设置", Action::OldSettings);
                old.arrow = true;
                items.push(old);
                items.push(MenuItem::separator());
                items.push(MenuItem::row("音量合成器", Action::Panel));
                items.push(MenuItem::row("设置", Action::Settings));
                items.push(MenuItem::row("退出", Action::Exit));
            }
        }
        let height = 12
            + items
                .iter()
                .map(|i| if i.separator { 12 } else { 34 })
                .sum::<i32>();
        Menu {
            items,
            width,
            requested_width: width,
            viewport_height: height,
            scroll: 0,
            selected: None,
        }
    }
    pub fn draw(&self, c: &mut Canvas, width: i32, height: i32) {
        c.fill(Rect::new(0, 0, width, height), BG);
        if self.view == View::Settings {
            self.draw_settings(c, width, height);
            return;
        }
        unsafe {
            let saved = SaveDC(c.dc);
            IntersectClipRect(c.dc, 0, c.px(TOP), c.px(width), c.px(height - FOOTER));
            if self.model.devices.is_empty() {
                self.draw_status(c, width);
            }
            let mut y = TOP - self.scroll;
            for (di, device) in self.model.devices.iter().enumerate() {
                c.fill(Rect::new(1, y, width - 2, HEADER), DEVICE_BG);
                if self.hovered == Some(Action::Collapse(di)) {
                    c.fill(Rect::new(1, y, width - 2, 48), 0x80531A);
                }
                let assigned = device.apps.iter().filter(|app| app.preferred_route.as_ref() == Some(&device.id)).count();
                let label = if self.model.default == Some(di) {
                    format!("{}（默认）", device.name)
                } else if assigned > 0 {
                    format!("{}（{}）", device.name, assigned)
                } else {
                    device.name.clone()
                };
                c.text(
                    Rect::new(14, y, width - 56, 48),
                    &label,
                    16,
                    WHITE,
                    DT_LEFT,
                );
                let ax = width - 21;
                let ay = y + 24;
                c.chevron(ax, ay, device.collapsed);
                let saved_icon = SaveDC(c.dc);
                IntersectClipRect(c.dc,c.px(24),c.px(y+59),c.px(50),c.px(y+85));
                c.volume_symbol(24,y+59,self.volume_icons[paint::volume_level(device.volume,device.muted)]);
                RestoreDC(c.dc,saved_icon);
                c.slider(
                    y + 50,
                    width,
                    device.volume,
                    device.peak,
                    device.muted,
                    true,
                    matches!(self.hovered, Some(Action::Slide(d, None) | Action::Mute(d, None) | Action::Collapse(d)) if d == di),
                );
                y += HEADER;
                if !device.collapsed {
                    if device.apps.is_empty() {
                        c.text(
                            Rect::new(14, y, width - 28, 32),
                            "暂无应用使用该设备",
                            12,
                            MUTED,
                            DT_CENTER,
                        );
                        y += 32;
                    }
                    for (ai, app) in device.apps.iter().enumerate() {
                        if y + ROW >= TOP && y < height - FOOTER {
                            if self.context_app == Some((di, ai)) || matches!(self.hovered, Some(Action::Slide(d, Some(a)) | Action::Mute(d, Some(a))) if d == di && a == ai)
                            {
                                c.fill(Rect::new(1, y, width - 2, ROW), 0x222222);
                            }
                            if app.icon != 0 {
                                if app.muted { c.dim_icon(app.icon,Rect::new(24,y+9,26,26)); }
                                else { c.icon(app.icon, Rect::new(24, y + 9, 26, 26)); }
                            } else {
                                c.circle(Rect::new(24, y + 9, 26, 26), 0x9E723A);
                                c.text(Rect::new(24, y + 9, 26, 26), "♪", 16, WHITE, DT_CENTER);
                            }
                            if app.muted {
                                c.volume_symbol(24, y + 9,self.volume_icons[0]);
                            }
                            c.slider(
                                y,
                                width,
                                app.volume,
                                app.peak,
                                app.muted,
                                false,
                                self.context_app == Some((di, ai)) || matches!(self.hovered, Some(Action::Slide(d, Some(a)) | Action::Mute(d, Some(a))) if d == di && a == ai),
                            );
                        }
                        y += ROW;
                    }
                }
            }
            RestoreDC(c.dc, saved);
        }
        self.scrollbar(c, width, height - FOOTER);
        self.border(c, width, height);
    }
    fn border(&self, c: &Canvas, width: i32, height: i32) {
        c.fill(Rect::new(0, 0, 1, height), 0x777777);
        c.fill(Rect::new(width - 1, 0, 1, height), 0x777777);
        c.fill(Rect::new(0, height - 1, width, 1), 0x777777);
    }
    fn scrollbar(&self, c: &Canvas, width: i32, bottom: i32) {
        if self.max_scroll() > 0 {
            let available = bottom - TOP;
            let total = if self.view == View::Settings {
                SETTINGS_HEIGHT - TOP
            } else {
                self.content_height()
            };
            let thumb = (available * available / total.max(1)).max(22);
            let y = TOP + (available - thumb) * self.scroll / self.max_scroll();
            c.rounded(Rect::new(width - 5, y, 3, thumb), 0x888888, 2);
        }
    }
    fn draw_status(&self, c: &mut Canvas, width: i32) {
        let (symbol, heading, description) = match self.model.scenario {
            Scenario::Loading => ("···", "正在加载音频设备", "请稍候"),
            Scenario::Empty => ("♪", "没有可用的播放设备", "连接扬声器或耳机后会自动更新"),
            Scenario::Error => ("!", "无法读取音频设备", "点击重试，或查看声音服务是否运行"),
            Scenario::Normal => ("♪", "暂无播放设备", "点击底部刷新"),
        };
        c.text(
            Rect::new(14, 44, width - 28, 52),
            symbol,
            30,
            BLUE,
            DT_CENTER,
        );
        c.text(
            Rect::new(14, 102, width - 28, 34),
            heading,
            18,
            WHITE,
            DT_CENTER,
        );
        c.text(
            Rect::new(14, 142, width - 28, 30),
            description,
            12,
            MUTED,
            DT_CENTER,
        );
        if self.model.scenario == Scenario::Error {
            c.rounded(Rect::new(width / 2 - 56, 188, 112, 34), DEVICE_BG, 4);
            c.text(
                Rect::new(width / 2 - 56, 188, 112, 34),
                "重试",
                14,
                WHITE,
                DT_CENTER,
            );
        }
    }
    fn draw_settings(&self, c: &mut Canvas, width: i32, height: i32) {
        let saved = unsafe { SaveDC(c.dc) };
        unsafe {
            IntersectClipRect(c.dc, 0, c.px(TOP), c.px(width), c.px(height));
        }
        let s = self.scroll;
        c.text(
            Rect::new(16, 40 - s, width - 32, 34),
            "设置",
            22,
            WHITE,
            DT_LEFT,
        );
        c.text(
            Rect::new(16, 72 - s, width - 32, 22),
            "应用单独指定的输出设备会保留",
            12,
            MUTED,
            DT_LEFT,
        );
        for hit in &self.hits {
            if hit.rect.top == 0 {
                continue;
            }
            c.rounded(
                hit.rect,
                if Some(&hit.action) == self.hovered.as_ref() {
                    0x383838
                } else {
                    0x2A2A2A
                },
                4,
            );
        }
        let device = self
            .model
            .default
            .and_then(|d| self.model.devices.get(d))
            .map(|d| d.name.as_str())
            .unwrap_or("未设置默认设备");
        c.text(
            Rect::new(24, 98 - s, width - 64, 38),
            &format!("默认设备：{device}"),
            13,
            WHITE,
            DT_LEFT,
        );
        c.text(
            Rect::new(width - 44, 98 - s, 20, 38),
            "›",
            20,
            MUTED,
            DT_CENTER,
        );
        c.text(
            Rect::new(24, 152 - s, width - 48, 38),
            "Windows 新版音量设置  ›",
            14,
            WHITE,
            DT_LEFT,
        );
        c.text(
            Rect::new(24, 194 - s, width - 48, 38),
            "Windows 旧版声音设置  ›",
            14,
            WHITE,
            DT_LEFT,
        );
        c.text(
            Rect::new(24, 248 - s, width - 104, 38),
            "开机自动启动",
            14,
            WHITE,
            DT_LEFT,
        );
        c.rounded(
            Rect::new(width - 68, 258 - s, 38, 20),
            if self.model.startup { BLUE } else { 0x666666 },
            10,
        );
        c.circle(
            Rect::new(
                width - if self.model.startup { 48 } else { 64 },
                261 - s,
                14,
                14,
            ),
            WHITE,
        );
        c.text(
            Rect::new(24, 312 - s, width - 48, 38),
            "‹  返回音量面板",
            14,
            WHITE,
            DT_LEFT,
        );
        unsafe {
            RestoreDC(c.dc, saved);
        }
        self.scrollbar(c, width, height);
        self.border(c, width, height);
    }
    pub fn draw_menu(&self, c: &mut Canvas, width: i32, height: i32) {
        c.fill(Rect::new(0, 0, width, height), MENU_BG);
        self.border(c, width, height);
        let Some(menu) = &self.menu else {
            return;
        };
        let saved = unsafe { SaveDC(c.dc) };
        unsafe {
            IntersectClipRect(c.dc, c.px(1), c.px(6), c.px(width - 1), c.px(height - 6));
        }
        let mut y = 6 - menu.scroll;
        for (index, item) in menu.items.iter().enumerate() {
            if item.separator {
                c.fill(Rect::new(20, y + 5, width - 40, 1), 0x777777);
                y += 12;
                continue;
            }
            if menu.selected == Some(index) && item.action.is_some() {
                c.fill(Rect::new(4, y, width - 8, 34), 0x414141);
            }
            if item.checked {
                c.text(Rect::new(8, y, 22, 34), "✓", 14, 0xCCCCCC, DT_CENTER);
            }
            c.text(
                Rect::new(36, y, width - 64, 34),
                &item.label,
                14,
                if item.action.is_some() { WHITE } else { MUTED },
                DT_LEFT,
            );
            if item.arrow {
                c.text(Rect::new(width - 28, y, 20, 34), "›", 20, MUTED, DT_CENTER);
            }
            y += 34;
        }
        unsafe {
            RestoreDC(c.dc, saved);
        }
        if menu.max_scroll() > 0 {
            let available = height - 12;
            let thumb = (available * available / (menu.height() - 12)).max(20);
            let y = 6 + (available - thumb) * menu.scroll / menu.max_scroll();
            c.rounded(Rect::new(width - 5, y, 3, thumb), 0x888888, 2);
        }
    }
}
impl Drop for Ui {
    fn drop(&mut self) {
        self.audio.take();
        for icon in self.icons.values() {
            if *icon != 0 {
                unsafe {
                    DestroyIcon(*icon as isize);
                }
            }
        }
    }
}
fn extract_icon(path: &str) -> usize {
    let mut icon = 0;
    if path == "__blank_application__" { return blank_app_icon() as usize; }
    unsafe {
        PrivateExtractIconsW(wide(path).as_ptr(), 0, 128, 128, &mut icon, std::ptr::null_mut(), 1, 0);
        if icon == 0 { ExtractIconExW(wide(path).as_ptr(), 0, &mut icon, std::ptr::null_mut(), 1); }
    }
    if icon == 0 || icon == -1 { icon = blank_app_icon(); }
    icon as usize
}
fn blank_app_icon() -> Handle {
    let mut icon = 0;
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| "C:/Windows".into());
    unsafe { PrivateExtractIconsW(wide(&format!("{system}/System32/shell32.dll")).as_ptr(),2,128,128,&mut icon,std::ptr::null_mut(),1,0); }
    icon
}
pub unsafe fn draw_window(ptr: *mut Ui, hwnd: Handle, popup: bool) {
    unsafe {
        let ui = &*ptr;
        paint::buffered(
            hwnd,
            if popup { ui.popup_scale } else { ui.scale },
            |canvas, width, height| {
                if popup {
                    ui.draw_menu(canvas, width, height);
                } else {
                    ui.draw(canvas, width, height);
                }
            },
        );
    }
}
