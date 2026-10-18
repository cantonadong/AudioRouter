#![windows_subsystem = "windows"]

mod audio;
mod config;
mod model;
mod paint;
mod platform;
mod ui;
mod win32;

use std::{
    ffi::c_void,
    ptr::{null, null_mut},
};
use ui::{Action, Effect, Ui};
use win32::*;

const CLASS: &str = "AudioRouter.Native.v1";
static TRAY_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

unsafe extern "system" fn tray_mouse_hook(code: i32, wp: usize, lp: isize) -> isize {
    unsafe {
        if code >= 0 && wp as u32 == WM_MOUSEWHEEL {
            let hwnd = TRAY_HWND.load(std::sync::atomic::Ordering::Acquire);
            if hwnd != 0 {
                let data = &*(lp as *const MouseHookData);
                let identifier = NotifyIconIdentifier { size: size_of::<NotifyIconIdentifier>() as u32, hwnd, id: 1, guid: [0;16] };
                let mut rect = Rect::default();
                if Shell_NotifyIconGetRect(&identifier,&mut rect) >= 0 && rect.contains(data.point) {
                    let delta = (data.mouse_data >> 16) as u16 as i16 as i32;
                    if PostMessageW(hwnd,WM_TRAY_WHEEL,delta as usize,0) != 0 { return 1; }
                }
            }
        }
        CallNextHookEx(0,code,wp,lp)
    }
}

fn main() {
    if std::env::args().any(|arg| arg == "--compact-config") {
        if let Err(error) = config::Config::compact_file() {
            config::log(&error);
            std::process::exit(1);
        }
        return;
    }
    unsafe {
        SetProcessDpiAwarenessContext(-4);
        let mutex = CreateMutexW(null(), 0, wide("Local\\AudioRouter.Native").as_ptr());
        if mutex == 0 {
            MessageBoxW(
                0,
                wide("无法创建单实例锁。请重新运行 AudioRouter。").as_ptr(),
                wide("AudioRouter").as_ptr(),
                0x10,
            );
            return;
        }
        if GetLastError() == 183 {
            // FindWindow is unnecessary: broadcast the registered message to the existing instance.
            let message = RegisterWindowMessageW(wide("AudioRouter.Show.Native.v1").as_ptr());
            PostMessageW(0xFFFF, message, 0, 0);
            CloseHandle(mutex);
            return;
        }
        let instance = GetModuleHandleW(null());
        let icon = LoadImageW(instance, 1usize as *const u16, 1, 32, 32, 0);
        let class_name = wide(CLASS);
        let class = WndClass {
            size: size_of::<WndClass>() as u32,
            style: 0x0008,
            proc: Some(window_proc),
            class_extra: 0,
            window_extra: 0,
            instance,
            icon,
            cursor: LoadCursorW(0, 32512usize as *const u16),
            background: 0,
            menu_name: null(),
            class_name: class_name.as_ptr(),
            small_icon: icon,
        };
        if RegisterClassExW(&class) == 0 {
            MessageBoxW(
                0,
                wide("无法注册窗口。").as_ptr(),
                wide("AudioRouter").as_ptr(),
                0x10,
            );
            if icon != 0 {
                DestroyIcon(icon);
            }
            CloseHandle(mutex);
            return;
        }
        let mut state = Box::new(Ui::new(instance, icon));
        let ptr = (&mut *state) as *mut Ui;
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class_name.as_ptr(),
            wide("AudioRouter").as_ptr(),
            WS_POPUP | WS_CLIPCHILDREN,
            0,
            0,
            ui::PANEL_WIDTH,
            800,
            0,
            0,
            instance,
            null_mut(),
        );
        if hwnd == 0 {
            MessageBoxW(
                0,
                wide("无法创建窗口。").as_ptr(),
                wide("AudioRouter").as_ptr(),
                0x10,
            );
            release_resources(ptr);
            CloseHandle(mutex);
            return;
        }
        state.hwnd = hwnd;
        for (level, handle) in state.volume_icons.iter_mut().enumerate() {
            *handle = paint::volume_icon(instance, level);
            if *handle == 0 { config::log(&format!("创建音量图标失败: level={level}, error={}", GetLastError())); }
        }
        state.scale = GetDpiForWindow(hwnd).max(96) as f64 / 96.0;
        state.taskbar_message = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
        state.show_message = RegisterWindowMessageW(wide("AudioRouter.Show.Native.v1").as_ptr());
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as isize);
        match audio::AudioClient::start(hwnd) {
            Ok(audio) => state.audio = Some(audio),
            Err(error) => {
                state.model.scenario = model::Scenario::Error;
                state.model.error = Some(error.clone());
                state.hint = error;
            }
        }
        add_tray(ptr);
        if !std::env::args().any(|a| a == "--background") || !state.tray_present {
            show_panel(ptr, true);
        }
        let mut msg = Msg::default();
        loop {
            let status = GetMessageW(&mut msg, 0, 0, 0);
            if status <= 0 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        release_resources(ptr);
        CloseHandle(mutex);
    }
}

unsafe fn release_resources(ptr: *mut Ui) {
    unsafe {
        TRAY_HWND.store(0,std::sync::atomic::Ordering::Release);
        if (*ptr).tray_hook != 0 { UnhookWindowsHookEx((*ptr).tray_hook); }
        for handle in (*ptr).volume_icons {
            if handle != 0 { DestroyIcon(handle); }
        }
        for handle in (*ptr).tray_icons {
            if handle != 0 { DestroyIcon(handle); }
        }
        if (*ptr).icon != 0 {
            DestroyIcon((*ptr).icon);
        }
    }
}

unsafe fn tray_data(ptr: *mut Ui) -> NotifyIcon {
    unsafe {
        let mut data: NotifyIcon = std::mem::zeroed();
        data.size = size_of::<NotifyIcon>() as u32;
        data.hwnd = (*ptr).hwnd;
        data.id = 1;
        data.flags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.callback = WM_TRAY;
        let state = &*ptr;
        let level = state.model.default.and_then(|i| state.model.devices.get(i))
            .map(|d| paint::volume_level(d.volume, d.muted)).unwrap_or(0);
        data.icon = if state.tray_icons[level] != 0 { state.tray_icons[level] } else { state.icon };
        set_tray_tip(&mut data,&state.model);
        data
    }
}

unsafe fn add_tray(ptr: *mut Ui) {
    unsafe {
        (*ptr).tray_level = None;
        rebuild_tray_icons(ptr);
        let data = tray_data(ptr);
        // TaskbarCreated also occurs on DPI changes, when the icon may still exist.
        (*ptr).tray_present = Shell_NotifyIconW(NIM_ADD, &data) != 0
            || Shell_NotifyIconW(1, &data) != 0;
        if !(*ptr).tray_present {
            config::log(&format!("Tray registration failed: error={}", GetLastError()));
            SetTimer((*ptr).hwnd, 10, 2000, null());
        }
        if (*ptr).tray_present && (*ptr).tray_hook == 0 {
            TRAY_HWND.store((*ptr).hwnd,std::sync::atomic::Ordering::Release);
            (*ptr).tray_hook = SetWindowsHookExW(14,Some(tray_mouse_hook),(*ptr).instance,0);
            if (*ptr).tray_hook == 0 { config::log(&format!("托盘滚轮监听失败：{}",GetLastError())); }
        }
    }
}

unsafe fn rebuild_tray_icons(ptr: *mut Ui) {
    unsafe {
        // The hidden panel can be on a different monitor from the taskbar.
        let taskbar = FindWindowW(wide("Shell_TrayWnd").as_ptr(), null());
        let dpi = GetDpiForWindow(if taskbar != 0 { taskbar } else { (*ptr).hwnd }).max(96);
        let width = GetSystemMetricsForDpi(49, dpi).max(16); // SM_CXSMICON
        let height = GetSystemMetricsForDpi(50, dpi).max(16); // SM_CYSMICON
        let icons = std::array::from_fn::<_, 4, _>(|level| paint::tray_volume_icon(level, width, height));
        if icons.contains(&0) {
            config::log(&format!("Tray icon creation failed: dpi={dpi}, size={width}x{height}, error={}", GetLastError()));
            for icon in icons { if icon != 0 { DestroyIcon(icon); } }
            return;
        }
        let previous = std::mem::replace(&mut (*ptr).tray_icons, icons);
        for icon in previous { if icon != 0 { DestroyIcon(icon); } }
        (*ptr).tray_level = None;
        config::log(&format!("Tray icons rebuilt: dpi={dpi}, size={width}x{height}"));
    }
}

unsafe fn update_tray_model(ptr: *mut Ui, model: &model::Model) {
    unsafe {
        if (*ptr).tray_present {
            let level = model.default.and_then(|i| model.devices.get(i))
                .map(|d| paint::volume_level(d.volume, d.muted)).unwrap_or(0);
            let mut data = tray_data(ptr);
            let volume = model.default.and_then(|i| model.devices.get(i)).map(|d| d.volume).unwrap_or(0);
            if (*ptr).tray_level == Some((level,volume)) { return; }
            // A tooltip-only change must not resubmit the icon to Explorer.
            if (*ptr).tray_level.is_some_and(|(previous, _)| previous == level) {
                data.flags = NIF_TIP;
            }
            set_tray_tip(&mut data,model);
            if (*ptr).tray_icons[level] != 0 { data.icon = (*ptr).tray_icons[level]; }
            if Shell_NotifyIconW(1, &data) == 0 { config::log("更新托盘音量图标失败"); add_tray(ptr); }
            else {
                (*ptr).tray_level = Some((level,volume));
                refresh_native_tray_tip(ptr);
            }
        }
    }
}
unsafe fn update_tray(ptr: *mut Ui) {
    unsafe { update_tray_model(ptr, &(*ptr).model); }
}
fn set_tray_tip(data: &mut NotifyIcon, model: &model::Model) {
    let tip = wide(&tray_tip_text(model));
    data.tip.fill(0);
    data.tip[..tip.len()].copy_from_slice(&tip);
}
fn tray_tip_text(model: &model::Model) -> String {
    let volume = model.default.and_then(|i| model.devices.get(i)).map(|d| d.volume).unwrap_or(0);
    format!("音量 {}%", volume)
}

unsafe extern "system" fn redraw_shell_tip(hwnd: Handle, process: isize) -> i32 {
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == process as u32 && IsWindowVisible(hwnd) != 0 {
            let mut class = [0u16; 64];
            let length = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
            if length > 0 && String::from_utf16_lossy(&class[..length as usize]) == "tooltips_class32" {
                // TTM_UPDATE: repaint Explorer's existing native tooltip.
                // No custom window, text buffer, or synthetic mouse movement.
                PostMessageW(hwnd, 0x041D, 0, 0);
            }
        }
        1
    }
}

unsafe fn refresh_native_tray_tip(ptr: *mut Ui) {
    unsafe {
        let identifier = NotifyIconIdentifier { size: size_of::<NotifyIconIdentifier>() as u32,
            hwnd: (*ptr).hwnd, id: 1, guid: [0;16] };
        let mut rect = Rect::default();
        let mut cursor = Point::default();
        if GetCursorPos(&mut cursor) == 0
            || Shell_NotifyIconGetRect(&identifier, &mut rect) < 0
            || !rect.contains(cursor) { return; }
        let taskbar = FindWindowW(wide("Shell_TrayWnd").as_ptr(), null());
        let mut pid = 0;
        if taskbar != 0 && GetWindowThreadProcessId(taskbar, &mut pid) != 0 {
            EnumWindows(Some(redraw_shell_tip), pid as isize);
        }
    }
}

unsafe fn monitor_work(point: Point) -> Rect {
    unsafe {
        let mut info = MonitorInfo {
            size: size_of::<MonitorInfo>() as u32,
            monitor: Rect::default(),
            work: Rect::new(0, 0, 1280, 720),
            flags: 0,
        };
        GetMonitorInfoW(MonitorFromPoint(point, 2), &mut info);
        info.work
    }
}

unsafe fn resize_panel(ptr: *mut Ui, reposition: bool) {
    unsafe {
        let hwnd = (*ptr).hwnd;
        let mut rect = Rect::default();
        let mut cursor = Point::default();
        GetCursorPos(&mut cursor);
        GetWindowRect(hwnd, &mut rect);
        let point = if reposition {
            cursor
        } else {
            Point {
                x: (rect.left + rect.right) / 2,
                y: (rect.top + rect.bottom) / 2,
            }
        };
        let work = monitor_work(point);
        let scale = (*ptr).scale;
        let width = ((ui::PANEL_WIDTH as f64 * scale).round() as i32)
            .min(work.right - work.left - 20)
            .max(200);
        let height = (((*ptr).desired_height() as f64 * scale).round() as i32)
            .min(work.bottom - work.top - 20)
            .max(200);
        let x = if reposition {
            work.right - width - 10
        } else {
            rect.left
                .clamp(work.left, (work.right - width).max(work.left))
        };
        let y = work.bottom - height - 10;
        {
            let state = &mut *ptr;
            state.width = (width as f64 / state.scale).round() as i32;
            state.height = (height as f64 / state.scale).round() as i32;
            state.relayout();
        }
        if rect.left != x || rect.top != y || rect.right-rect.left != width || rect.bottom-rect.top != height {
            // Do not copy the old layout into the resized surface or redraw midway.
            SetWindowPos(hwnd, 0, x, y, width, height, 0x0010 | 0x0004 | 0x0008 | 0x0100);
        }
        InvalidateRect(hwnd, null(), 0);
        if IsWindowVisible(hwnd) != 0 { UpdateWindow(hwnd); }
    }
}

unsafe fn show_panel(ptr: *mut Ui, reposition: bool) {
    unsafe {
        (&mut *ptr).reset_collapses();
        resize_panel(ptr, reposition);
        let hwnd = (*ptr).hwnd;
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
        // Activation can be denied, or deactivation skipped during a menu/drag.
        // Recheck after Windows has completed the foreground transition.
        SetTimer(hwnd, 9, 150, null());
    }
}

unsafe fn menu_action(menu: Handle, actions: &mut Vec<Action>, label: &str, action: Action, checked: bool) {
    unsafe {
        actions.push(action);
        AppendMenuW(menu, if checked { 8 } else { 0 }, actions.len(), wide(label).as_ptr());
    }
}
unsafe fn tray_menu(ptr: *mut Ui) {
    unsafe {
        platform::refresh_menu_theme((*ptr).hwnd);
        clear_tip(ptr);
        close_menu(ptr);
        (&*ptr).send(audio::Command::Refresh);
        let root = CreatePopupMenu();
        if root == 0 { return; }
        let mut actions = Vec::new();
        let state = &*ptr;
        AppendMenuW(root, 2, 0, wide("默认设备").as_ptr());
        for (d,device) in state.model.devices.iter().enumerate() {
            menu_action(root,&mut actions,&device.name,Action::Default(d),state.model.default == Some(d));
        }
        if state.model.devices.is_empty() { AppendMenuW(root,2,0,wide("没有可用播放设备").as_ptr()); }
        AppendMenuW(root,0x800,0,null());
        menu_action(root,&mut actions,if state.model.startup { "开机启动：是" } else { "开机启动：否" },Action::Startup,false);
        let hidden = CreatePopupMenu();
        if state.model.hidden_apps.is_empty() {
            AppendMenuW(hidden,2,0,wide("暂无隐藏应用").as_ptr());
        } else {
            for app in &state.model.hidden_apps {
                menu_action(hidden,&mut actions,&app.name,Action::ShowApp(app.key.clone()),false);
            }
        }
        AppendMenuW(root,0x10,hidden as usize,wide("隐藏应用").as_ptr());
        AppendMenuW(root,0x800,0,null());
        let sounds = CreatePopupMenu();
        menu_action(sounds,&mut actions,"新版",Action::SystemSettings(None),false);
        menu_action(sounds,&mut actions,"旧版",Action::SystemSettings(Some(0)),false);
        AppendMenuW(root,0x10,sounds as usize,wide("Windows 声音设置").as_ptr());
        AppendMenuW(root,0x800,0,null());
        menu_action(root,&mut actions,"退出",Action::Exit,false);
        let mut point = Point::default(); GetCursorPos(&mut point);
        (*ptr).menu_opening = true;
        SetForegroundWindow((*ptr).hwnd);
        let chosen = TrackPopupMenuEx(root,0x100|2,point.x,point.y,(*ptr).hwnd,null());
        DestroyMenu(root);
        (*ptr).menu_opening = false;
        // Resolve against the snapshot used to build the menu before applying deferred updates.
        if chosen > 0 {
            if let Some(action) = actions.get(chosen as usize-1) {
                let result = (&mut *ptr).activate(action.clone());
                effect(ptr,result);
            }
        }
        apply_pending(ptr);
        PostMessageW((*ptr).hwnd,0,0,0);
    }
}

unsafe fn app_menu(ptr: *mut Ui, d: usize, a: usize) {
    unsafe {
        clear_tip(ptr);
        platform::refresh_menu_theme((*ptr).hwnd);
        let Some(app) = (&(*ptr).model.devices).get(d).and_then(|d| d.apps.get(a)) else { return; };
        let system = app.key == "__system_sounds__";
        let menu = CreatePopupMenu();
        if menu == 0 { return; }
        let mut actions = Vec::new();
        menu_action(menu,&mut actions,"默认",Action::Route(d,a,0,false),app.preferred_route.is_none());
        AppendMenuW(menu,0x800,0,null());
        for (target,device) in (*ptr).model.devices.iter().enumerate() {
            if system || !(*ptr).model.routing_available {
                AppendMenuW(menu,2,0,wide(&device.name).as_ptr());
            } else {
                menu_action(menu,&mut actions,&device.name,Action::Route(d,a,target,true),app.preferred_route.as_ref()==Some(&device.id));
            }
        }
        AppendMenuW(menu,0x800,0,null());
        menu_action(menu,&mut actions,"隐藏",Action::HideApp(d,a),false);
        let mut point = Point::default(); GetCursorPos(&mut point);
        (*ptr).context_app = Some((d, a));
        InvalidateRect((*ptr).hwnd, null(), 0);
        UpdateWindow((*ptr).hwnd);
        (*ptr).menu_opening = true;
        SetForegroundWindow((*ptr).hwnd);
        let chosen = TrackPopupMenuEx(menu,0x100|2,point.x,point.y,(*ptr).hwnd,null());
        DestroyMenu(menu);
        (*ptr).menu_opening = false;
        (*ptr).context_app = None;
        (*ptr).hovered = None;
        InvalidateRect((*ptr).hwnd, null(), 0);
        if chosen > 0 {
            if let Some(action) = actions.get(chosen as usize-1) {
                if !system || matches!(action,Action::HideApp(..)) {
                    let result = (&mut *ptr).activate(action.clone());
                    effect(ptr,result);
                }
            }
        }
        apply_pending(ptr);
        PostMessageW((*ptr).hwnd,0,0,0);
    }
}

unsafe fn close_menu(ptr: *mut Ui) {
    unsafe {
        (*ptr).menu_opening = true;
        let popup = (*ptr).popup;
        if popup != 0 {
            ShowWindow(popup, SW_HIDE);
        }
        (*ptr).menu_opening = false;
        apply_pending(ptr);
    }
}

unsafe fn apply_pending(ptr: *mut Ui) {
    unsafe {
        if (*ptr).dragging.is_some() || ((*ptr).popup != 0 && IsWindowVisible((*ptr).popup) != 0) {
            return;
        }
        if let Some(model) = (*ptr).pending_model.take() {
            let changed = (&mut *ptr).apply_model(model);
            update_tray(ptr);
            if changed && IsWindowVisible((*ptr).hwnd) != 0 {
                resize_panel(ptr, false);
            }
            InvalidateRect((*ptr).hwnd, null(), 0);
        }
    }
}

unsafe fn open_menu(ptr: *mut Ui, action: Action) {
    unsafe {
        close_menu(ptr);
        let (instance, owner) = {
            let state = &mut *ptr;
            state.menu = Some(state.make_menu(action));
            (state.instance, state.hwnd)
        };
        (*ptr).menu_opening = true;
        let mut cursor = Point::default();
        GetCursorPos(&mut cursor);
        if (*ptr).popup == 0 {
            let popup = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                wide(CLASS).as_ptr(),
                wide("AudioRouter menu").as_ptr(),
                WS_POPUP,
                cursor.x,
                cursor.y,
                1,
                1,
                owner,
                0,
                instance,
                null_mut::<c_void>(),
            );
            if popup == 0 {
                (*ptr).menu_opening = false;
                return;
            }
            (*ptr).popup = popup;
            SetWindowLongPtrW(popup, GWLP_USERDATA, ptr as isize);
        }
        let popup = (*ptr).popup;
        // Move the hidden popup first so GetDpiForWindow uses the cursor monitor.
        SetWindowPos(popup, -1, cursor.x, cursor.y, 1, 1, 0x0010);
        (*ptr).popup_scale = GetDpiForWindow(popup).max(96) as f64 / 96.0;
        position_menu(ptr, cursor);
        ShowWindow(popup, SW_SHOW);
        SetForegroundWindow(popup);
        SetFocus(popup);
        (*ptr).menu_opening = false;
        InvalidateRect(popup, null(), 0);
    }
}

unsafe fn position_menu(ptr: *mut Ui, anchor: Point) {
    unsafe {
        let work = monitor_work(anchor);
        let scale = (*ptr).popup_scale;
        let (width, height) = {
            let menu = (*ptr).menu.as_mut().unwrap();
            let w = ((menu.requested_width as f64 * scale).round() as i32)
                .min(work.right - work.left - 16)
                .max(100);
            let h = ((menu.height() as f64 * scale).round() as i32)
                .min(work.bottom - work.top - 16)
                .max(100);
            menu.width = (w as f64 / scale).round() as i32;
            menu.viewport_height = (h as f64 / scale).round() as i32;
            menu.scroll = menu.scroll.clamp(0, menu.max_scroll());
            (w, h)
        };
        let x = anchor
            .x
            .clamp(work.left, (work.right - width).max(work.left));
        let y = anchor
            .y
            .clamp(work.top, (work.bottom - height).max(work.top));
        SetWindowPos((*ptr).popup, -1, x, y, width, height, 0x0010);
    }
}

unsafe fn effect(ptr: *mut Ui, effect: Effect) {
    unsafe {
        let hwnd = (*ptr).hwnd;
        match effect {
            Effect::None => {
                update_tray(ptr);
                InvalidateRect(hwnd, null(), 0);
            }
            Effect::Resize => {
                resize_panel(ptr, false);
            }
            Effect::Show => {
                show_panel(ptr, true);
            }
            Effect::Hide => {
                clear_tip(ptr);
                close_menu(ptr);
                if (*ptr).tray_present {
                    ShowWindow(hwnd, SW_HIDE);
                } else {
                    MessageBoxW(
                        hwnd,
                        wide("托盘图标不可用，窗口将保持可见。请使用菜单中的“退出”关闭软件。")
                            .as_ptr(),
                        wide("AudioRouter").as_ptr(),
                        0x30,
                    );
                }
            }
            Effect::Menu(action) => {
                clear_tip(ptr);
                open_menu(ptr, action);
            }
            Effect::Capture => {
                SetTimer(hwnd,8,16,null());
                update_tray(ptr);
                SetFocus(hwnd);
                SetCapture(hwnd);
                InvalidateRect(hwnd, null(), 0);
            }
            Effect::Release => {
                (&mut *ptr).flush_volume();
                KillTimer(hwnd,8);
                ReleaseCapture();
                apply_pending(ptr);
                InvalidateRect(hwnd, null(), 0);
            }
            Effect::Quit => {
                close_menu(ptr);
                (*ptr).audio.take();
                if (*ptr).popup != 0 {
                    DestroyWindow((*ptr).popup);
                }
                DestroyWindow(hwnd);
            }
            Effect::OpenSettings(page) => {
                (*ptr).menu_opening = true;
                if let Err(error) = platform::open_settings(hwnd, page) {
                    (*ptr).hint = error;
                }
                (*ptr).menu_opening = false;
                InvalidateRect(hwnd, null(), 0);
            }
            Effect::CloseMenu => {
                close_menu(ptr);
            }
        }
    }
}

unsafe fn clear_tip(ptr: *mut Ui) {
    unsafe {
        KillTimer((*ptr).hwnd, 7);
        if (*ptr).tooltip != 0 { ShowWindow((*ptr).tooltip, SW_HIDE); }
        (*ptr).tooltip_start = None;
        (*ptr).tooltip_ready = false;
        (*ptr).tooltip_name.clear();
    }
}
unsafe fn show_tip(ptr: *mut Ui) {
    unsafe {
        if (*ptr).tooltip == 0 {
            (*ptr).tooltip = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | 0x08000000,
                wide(CLASS).as_ptr(), wide("").as_ptr(), WS_POPUP, 0,0,1,1,
                (*ptr).hwnd,0,(*ptr).instance,null_mut());
            if (*ptr).tooltip == 0 { return; }
            SetWindowLongPtrW((*ptr).tooltip, GWLP_USERDATA, ptr as isize);
        }
        let mut cursor = Point::default(); GetCursorPos(&mut cursor);
        let work = monitor_work(cursor);
        let scale = (*ptr).scale;
        let (width,height,display) = paint::tooltip_size((*ptr).hwnd,scale,&(*ptr).tooltip_name,work.right-work.left-20);
        (*ptr).tooltip_display = display;
        SetWindowPos((*ptr).tooltip,-1,(cursor.x+16).clamp(work.left,work.right-width),
            (cursor.y+20).clamp(work.top,work.bottom-height),width,height,0x0010|0x0040);
        InvalidateRect((*ptr).tooltip,null(),0);
    }
}
unsafe fn hover_tip(ptr: *mut Ui, point: Point) {
    unsafe {
        let target = if (*ptr).view == ui::View::Panel {
            (&*ptr).hit(point).and_then(|hit| match hit.action {
                Action::Mute(d,Some(a)) if point.x >= 24 && point.x < 50
                    && (((*ptr).tooltip_start.is_some() || (*ptr).tooltip_ready)
                        || (point.y >= hit.rect.top+9 && point.y < hit.rect.top+35)) =>
                    (&(*ptr).model.devices).get(d).and_then(|d| d.apps.get(a)).map(|a| a.name.clone()),
                _ => None,
            })
        } else { None };
        let Some(name) = target else { clear_tip(ptr); return; };
        if (*ptr).tooltip_name == name { return; }
        (*ptr).tooltip_name = name;
        if (*ptr).tooltip_ready { show_tip(ptr); }
        else if (*ptr).tooltip_start.is_none() {
            (*ptr).tooltip_start = Some(std::time::Instant::now());
            SetTimer((*ptr).hwnd,7,50,null());
        }
    }
}

unsafe fn logical_point(ptr: *mut Ui, lp: isize, popup: bool) -> Point {
    unsafe {
        let point = mouse_point(lp);
        let scale = if popup {
            (*ptr).popup_scale
        } else {
            (*ptr).scale
        };
        Point {
            x: (point.x as f64 / scale).round() as i32,
            y: (point.y as f64 / scale).round() as i32,
        }
    }
}

unsafe extern "system" fn window_proc(hwnd: Handle, msg: u32, wp: usize, lp: isize) -> isize {
    unsafe {
        if msg == WM_ERASEBKGND {
            return 1;
        }
        if msg == WM_NCCREATE {
            return 1;
        }
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ui;
        if ptr.is_null() {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        if hwnd == (*ptr).tooltip {
            if msg == WM_PAINT {
                paint::buffered(hwnd, (*ptr).scale, |c,w,h| {
                    c.fill(Rect::new(0,0,w,h),0x303030);
                    c.text(Rect::new(10,6,w-20,h-12), &(*ptr).tooltip_display,11,paint::WHITE,0x80000000 | 0x10);
                });
                return 0;
            }
            return DefWindowProcW(hwnd,msg,wp,lp);
        }
        let popup = hwnd == (*ptr).popup;
        if msg == (*ptr).taskbar_message && !popup {
            config::log("TaskbarCreated received");
            add_tray(ptr);
            return 0;
        }
        if msg == (*ptr).show_message && !popup {
            show_panel(ptr, true);
            return 0;
        }
        match msg {
            // Coalesce display/settings/resume changes; Explorer may need time
            // to finish rebuilding its taskbar before accepting the new icon.
            0x007E | 0x001A | 0x0218 if !popup => {
                config::log(&format!("Tray environment change: message=0x{msg:04X}, wparam={wp}"));
                SetTimer(hwnd, 10, 1500, null());
            }
            WM_TIMER if !popup && wp == 10 => {
                KillTimer(hwnd, 10);
                rebuild_tray_icons(ptr);
                if (*ptr).tray_present { update_tray(ptr); }
                else { add_tray(ptr); }
                return 0;
            }
            WM_TRAY_WHEEL if !popup => {
                (&mut *ptr).tray_wheel_volume(wp as i32);
                update_tray(ptr);
                if IsWindowVisible(hwnd) != 0 { InvalidateRect(hwnd,null(),0); }
                return 0;
            }
            0x0018 if !popup => {
                if wp == 0 {
                    KillTimer(hwnd, 9);
                    (&mut *ptr).reset_collapses();
                }
                (&*ptr).send(audio::Command::Meter(wp != 0));
            }
            audio::WM_AUDIO_PEAK if !popup => {
                let peaks = (*ptr).audio.as_ref().and_then(|audio| audio.take_peaks());
                if IsWindowVisible(hwnd) != 0 {
                    if let Some(peaks) = peaks {
                        if (&mut *ptr).apply_peaks(peaks) { InvalidateRect(hwnd,null(),0); }
                    }
                }
                return 0;
            }
            WM_TIMER if !popup && wp == 9 => {
                if IsWindowVisible(hwnd) == 0 {
                    KillTimer(hwnd, 9);
                    return 0;
                }
                if (*ptr).tray_present && !(*ptr).menu_opening && (*ptr).dragging.is_none() {
                    let foreground = GetForegroundWindow();
                    let menu = (*ptr).popup;
                    if foreground != 0 && foreground != hwnd
                        && !(menu != 0 && foreground == menu && IsWindowVisible(menu) != 0)
                    {
                        effect(ptr, Effect::Hide);
                    }
                }
                return 0;
            }
            WM_TIMER if !popup && wp == 8 => {
                (&mut *ptr).flush_volume();
                return 0;
            }
            WM_TIMER if !popup && wp == 7 => {
                if IsWindowVisible(hwnd) == 0 { clear_tip(ptr); return 0; }
                let mut point = Point::default(); GetCursorPos(&mut point); ScreenToClient(hwnd,&mut point);
                let point = Point { x: (point.x as f64 / (*ptr).scale).round() as i32,
                    y: (point.y as f64 / (*ptr).scale).round() as i32 };
                hover_tip(ptr,point);
                if (*ptr).tooltip_start.is_some_and(|start| start.elapsed().as_millis() >= 500) {
                    (*ptr).tooltip_ready = true;
                    KillTimer(hwnd,7);
                    show_tip(ptr);
                }
                return 0;
            }
            audio::WM_AUDIO_UPDATE if !popup => {
                let model = (*ptr).audio.as_ref().and_then(|audio| audio.take());
                if let Some(model) = model {
                    update_tray_model(ptr, &model);
                    if (*ptr).dragging.is_some() || (*ptr).menu_opening
                        || ((*ptr).popup != 0 && IsWindowVisible((*ptr).popup) != 0)
                    {
                        (*ptr).pending_model = Some(model);
                    } else {
                        let changed = (&mut *ptr).apply_model(model);
                        update_tray(ptr);
                        if changed && IsWindowVisible(hwnd) != 0 {
                            resize_panel(ptr, false);
                        }
                        if IsWindowVisible(hwnd) != 0 {
                            InvalidateRect(hwnd, null(), 0);
                        }
                    }
                }
                return 0;
            }
            WM_PAINT => {
                ui::draw_window(ptr, hwnd, popup);
                return 0;
            }
            WM_SIZE if !popup => {
                let mut rect = Rect::default();
                GetClientRect(hwnd, &mut rect);
                {
                    let state = &mut *ptr;
                    state.width = (rect.right as f64 / state.scale).round() as i32;
                    state.height = (rect.bottom as f64 / state.scale).round() as i32;
                    state.relayout();
                }
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            WM_DPICHANGED => {
                let scale = (wp & 0xFFFF).max(96) as f64 / 96.0;
                let rect = *(lp as *const Rect);
                if popup {
                    (*ptr).popup_scale = scale;
                    if !(*ptr).menu_opening {
                        (*ptr).menu_opening = true;
                        position_menu(
                            ptr,
                            Point {
                                x: rect.left,
                                y: rect.top,
                            },
                        );
                        (*ptr).menu_opening = false;
                        InvalidateRect(hwnd, null(), 0);
                    }
                } else {
                    (*ptr).scale = scale;
                    SetWindowPos(
                        hwnd,
                        -1,
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                        0x0010,
                    );
                    resize_panel(ptr, false);
                }
                return 0;
            }
            WM_NCHITTEST if !popup => {
                let mut point = mouse_point(lp);
                ScreenToClient(hwnd, &mut point);
                let scale = (*ptr).scale;
                let x = (point.x as f64 / scale) as i32;
                let y = (point.y as f64 / scale) as i32;
                if y > 0 && y < ui::TOP && x > 0 && x < (*ptr).width - 72 {
                    return 2;
                }
                return 1;
            }
            WM_ACTIVATE if wp & 0xFFFF == 0 => {
                if !popup { clear_tip(ptr); }
                if (*ptr).menu_opening || (*ptr).dragging.is_some() {
                    return 0;
                }
                // WM_ACTIVATE is synchronous; GetForegroundWindow may still
                // report the old window. lParam identifies the new active one.
                let foreground = lp;
                if popup {
                    if foreground != (*ptr).hwnd {
                        close_menu(ptr);
                        if (*ptr).tray_present {
                            ShowWindow((*ptr).hwnd, SW_HIDE);
                        }
                    }
                } else if foreground != (*ptr).popup || IsWindowVisible((*ptr).popup) == 0 {
                    if (*ptr).tray_present {
                        ShowWindow(hwnd, SW_HIDE);
                    }
                }
                return 0;
            }
            WM_MOUSEMOVE => {
                let point = logical_point(ptr, lp, popup);
                let redraw = {
                    let state = &mut *ptr;
                    if popup {
                        if let Some(menu) = &mut state.menu {
                            let selected = menu.index_at(point);
                            let changed = selected != menu.selected;
                            menu.selected = selected;
                            changed
                        } else {
                            false
                        }
                    } else if state.dragging.is_some() {
                        state.volume_at(point.x);
                        true
                    } else {
                        let action = state.hit(point).map(|hit| hit.action.clone());
                        let changed = action != state.hovered;
                        state.hovered = action;
                        changed
                    }
                };
                if !popup { update_tray(ptr); hover_tip(ptr, point); }
                if redraw {
                    InvalidateRect(hwnd, null(), 0);
                }
                let mut track = TrackMouse {
                    size: size_of::<TrackMouse>() as u32,
                    flags: 2,
                    hwnd,
                    hover_time: 0,
                };
                TrackMouseEvent(&mut track);
                return 0;
            }
            WM_MOUSELEAVE => {
                if !popup { clear_tip(ptr); }
                if popup {
                    if let Some(menu) = &mut (*ptr).menu {
                        menu.selected = None;
                    }
                } else {
                    (*ptr).hovered = None;
                }
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            WM_LBUTTONDOWN => {
                let point = logical_point(ptr, lp, popup);
                if popup {
                    let index = (*ptr).menu.as_ref().and_then(|m| m.index_at(point));
                    if let Some(index) = index {
                        PostMessageW((*ptr).hwnd, WM_DO_ACTION, index, 0);
                    }
                } else {
                    let result = {
                        let state = &mut *ptr;
                        let action = state.hit(point).map(|hit| hit.action.clone());
                        if let Some(action) = action {
                            let result = state.activate(action);
                            if state.dragging.is_some() {
                                state.volume_at(point.x);
                            }
                            result
                        } else {
                            Effect::None
                        }
                    };
                    effect(ptr, result);
                }
                return 0;
            }
            WM_LBUTTONUP if !popup => {
                if (*ptr).dragging.take().is_some() {
                    effect(ptr, Effect::Release);
                }
                return 0;
            }
            WM_CAPTURECHANGED if !popup => {
                (&mut *ptr).flush_volume();
                KillTimer(hwnd,8);
                (*ptr).dragging = None;
                apply_pending(ptr);
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            WM_RBUTTONUP if !popup => {
                let point = logical_point(ptr,lp,false);
                let target = (&*ptr).hit(point).and_then(|hit| match hit.action {
                    Action::Slide(d,Some(a)) | Action::Mute(d,Some(a)) => Some((d,a)),
                    _ => None,
                });
                if let Some((d,a)) = target { app_menu(ptr,d,a); }
                return 0;
            }
            WM_MOUSEWHEEL => {
                let mut point = mouse_point(lp);
                ScreenToClient(hwnd, &mut point);
                let point = Point { x: (point.x as f64 / (*ptr).scale).round() as i32,
                    y: (point.y as f64 / (*ptr).scale).round() as i32 };
                {
                    let state = &mut *ptr;
                    let delta = ((wp >> 16) as u16 as i16) as i32;
                    if popup {
                        if let Some(menu) = &mut state.menu {
                            menu.scroll =
                                (menu.scroll - delta * 34 / 120).clamp(0, menu.max_scroll());
                            menu.selected = None;
                        }
                    } else if !state.wheel_volume(point, delta) {
                        state.scroll =
                            (state.scroll - delta * ui::ROW / 120).clamp(0, state.max_scroll());
                        state.hovered = None;
                        state.relayout();
                    }
                }
                update_tray(ptr);
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            WM_DO_ACTION if !popup => {
                let action = (*ptr)
                    .menu
                    .as_ref()
                    .and_then(|m| m.items.get(wp))
                    .and_then(|i| i.action.clone());
                if let Some(action) = action {
                    let result = (&mut *ptr).activate(action);
                    close_menu(ptr);
                    effect(ptr, result);
                } else {
                    close_menu(ptr);
                }
                return 0;
            }
            WM_KEYDOWN => {
                if popup {
                    if wp == 0x1B {
                        close_menu(ptr);
                        SetForegroundWindow((*ptr).hwnd);
                        SetFocus((*ptr).hwnd);
                    } else if wp == 0x0D {
                        if let Some(index) = (*ptr).menu.as_ref().and_then(|m| m.selected) {
                            PostMessageW((*ptr).hwnd, WM_DO_ACTION, index, 0);
                        }
                    } else if wp == 0x26 || wp == 0x28 {
                        if let Some(menu) = &mut (*ptr).menu {
                            let indices: Vec<usize> = menu
                                .items
                                .iter()
                                .enumerate()
                                .filter_map(|(i, item)| item.action.as_ref().map(|_| i))
                                .collect();
                            if !indices.is_empty() {
                                let current =
                                    indices.iter().position(|i| Some(*i) == menu.selected);
                                let next = if wp == 0x26 {
                                    current
                                        .map(|i| (i + indices.len() - 1) % indices.len())
                                        .unwrap_or(indices.len() - 1)
                                } else {
                                    current.map(|i| (i + 1) % indices.len()).unwrap_or(0)
                                };
                                menu.selected = Some(indices[next]);
                                menu.reveal_selected();
                            }
                        }
                        InvalidateRect(hwnd, null(), 0);
                    }
                } else {
                    let shift = GetKeyState(0x10) < 0;
                    let result = {
                        let state = &mut *ptr;
                        match wp {
                            0x1B => Effect::Hide,
                            0x74 => state.activate(Action::Refresh),
                            0x09 => {
                                state.next_focus(shift);
                                Effect::None
                            }
                            0x25 | 0x28 => {
                                state.adjust_focused(if shift { -10 } else { -1 }, None);
                                Effect::None
                            }
                            0x26 | 0x27 => {
                                state.adjust_focused(if shift { 10 } else { 1 }, None);
                                Effect::None
                            }
                            0x24 => {
                                state.adjust_focused(0, Some(0));
                                Effect::None
                            }
                            0x23 => {
                                state.adjust_focused(0, Some(100));
                                Effect::None
                            }
                            0x20 => {
                                if let Some(Action::Slide(d, a)) = state.focused {
                                    state.activate(Action::Mute(d, a))
                                } else {
                                    Effect::None
                                }
                            }
                            _ => Effect::None,
                        }
                    };
                    effect(ptr, result);
                }
                return 0;
            }
            WM_TRAY if !popup => {
                match lp as u32 {
                    WM_LBUTTONUP => {
                        close_menu(ptr);
                        (*ptr).view = ui::View::Panel;
                        (&*ptr).send(audio::Command::Refresh);
                        show_panel(ptr, true);
                    }
                    WM_RBUTTONUP => tray_menu(ptr),
                    _ => {}
                }
                return 0;
            }
            WM_SHOW_PANEL if !popup => {
                show_panel(ptr, true);
                return 0;
            }
            WM_CLOSE => {
                effect(
                    ptr,
                    if popup {
                        Effect::CloseMenu
                    } else {
                        Effect::Hide
                    },
                );
                return 0;
            }
            WM_DESTROY if !popup => {
                let data = tray_data(ptr);
                Shell_NotifyIconW(NIM_DELETE, &data);
                (*ptr).tray_present = false;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                PostQuitMessage(0);
                return 0;
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}
