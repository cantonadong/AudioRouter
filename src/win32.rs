#![allow(non_snake_case, dead_code)]

use std::ffi::c_void;

pub type Handle = isize;
pub type WndProc = unsafe extern "system" fn(Handle, u32, usize, isize) -> isize;
pub type HookProc = unsafe extern "system" fn(i32, usize, isize) -> isize;
#[repr(C)]
pub struct MouseHookData {
    pub point: Point,
    pub mouse_data: u32,
    pub flags: u32,
    pub time: u32,
    pub extra: usize,
}
#[repr(C)]
pub struct NotifyIconIdentifier {
    pub size: u32,
    pub hwnd: Handle,
    pub id: u32,
    pub guid: [u8; 16],
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
#[repr(C)]
pub struct BitmapHeader {
    pub size: u32,
    pub width: i32,
    pub height: i32,
    pub planes: u16,
    pub bits: u16,
    pub compression: u32,
    pub image_size: u32,
    pub xppm: i32,
    pub yppm: i32,
    pub colors: u32,
    pub important: u32,
}
impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        }
    }
    pub fn contains(self, p: Point) -> bool {
        p.x >= self.left && p.x < self.right && p.y >= self.top && p.y < self.bottom
    }
}

#[repr(C)]
pub struct WndClass {
    pub size: u32,
    pub style: u32,
    pub proc: Option<WndProc>,
    pub class_extra: i32,
    pub window_extra: i32,
    pub instance: Handle,
    pub icon: Handle,
    pub cursor: Handle,
    pub background: Handle,
    pub menu_name: *const u16,
    pub class_name: *const u16,
    pub small_icon: Handle,
}
#[repr(C)]
pub struct CreateStruct {
    pub params: *mut c_void,
    pub instance: Handle,
    pub menu: Handle,
    pub parent: Handle,
    pub cy: i32,
    pub cx: i32,
    pub y: i32,
    pub x: i32,
    pub style: i32,
    pub name: *const u16,
    pub class: *const u16,
    pub ex_style: u32,
}
#[repr(C)]
#[derive(Default)]
pub struct Msg {
    pub hwnd: Handle,
    pub message: u32,
    pub wparam: usize,
    pub lparam: isize,
    pub time: u32,
    pub pt: Point,
    pub private: u32,
}
#[repr(C)]
pub struct PaintStruct {
    pub dc: Handle,
    pub erase: i32,
    pub rect: Rect,
    pub restore: i32,
    pub inc_update: i32,
    pub reserved: [u8; 32],
}
#[repr(C)]
pub struct MonitorInfo {
    pub size: u32,
    pub monitor: Rect,
    pub work: Rect,
    pub flags: u32,
}
#[repr(C)]
pub struct TrackMouse {
    pub size: u32,
    pub flags: u32,
    pub hwnd: Handle,
    pub hover_time: u32,
}
#[repr(C)]
pub struct NotifyIcon {
    pub size: u32,
    pub hwnd: Handle,
    pub id: u32,
    pub flags: u32,
    pub callback: u32,
    pub icon: Handle,
    pub tip: [u16; 128],
    pub state: u32,
    pub state_mask: u32,
    pub info: [u16; 256],
    pub timeout_version: u32,
    pub info_title: [u16; 64],
    pub info_flags: u32,
    pub guid: [u8; 16],
    pub balloon_icon: Handle,
}

#[link(name = "user32")]
unsafe extern "system" {
    pub fn SetWindowsHookExW(kind: i32, callback: Option<HookProc>, module: Handle, thread: u32) -> Handle;
    pub fn UnhookWindowsHookEx(hook: Handle) -> i32;
    pub fn CallNextHookEx(hook: Handle, code: i32, wp: usize, lp: isize) -> isize;
    pub fn RegisterClassExW(class: *const WndClass) -> u16;
    pub fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: Handle,
        menu: Handle,
        instance: Handle,
        param: *mut c_void,
    ) -> Handle;
    pub fn DefWindowProcW(hwnd: Handle, msg: u32, wp: usize, lp: isize) -> isize;
    pub fn SetWindowLongPtrW(hwnd: Handle, index: i32, value: isize) -> isize;
    pub fn GetWindowLongPtrW(hwnd: Handle, index: i32) -> isize;
    pub fn GetMessageW(msg: *mut Msg, hwnd: Handle, min: u32, max: u32) -> i32;
    pub fn TranslateMessage(msg: *const Msg) -> i32;
    pub fn DispatchMessageW(msg: *const Msg) -> isize;
    pub fn PostQuitMessage(code: i32);
    pub fn PostMessageW(hwnd: Handle, msg: u32, wp: usize, lp: isize) -> i32;
    pub fn SendMessageW(hwnd: Handle, msg: u32, wp: usize, lp: isize) -> isize;
    pub fn EnumWindows(callback: Option<unsafe extern "system" fn(Handle, isize) -> i32>, param: isize) -> i32;
    pub fn GetWindowThreadProcessId(hwnd: Handle, process: *mut u32) -> u32;
    pub fn GetClassNameW(hwnd: Handle, class: *mut u16, count: i32) -> i32;
    pub fn ShowWindow(hwnd: Handle, cmd: i32) -> i32;
    pub fn DestroyWindow(hwnd: Handle) -> i32;
    pub fn IsWindowVisible(hwnd: Handle) -> i32;
    pub fn SetForegroundWindow(hwnd: Handle) -> i32;
    pub fn SetFocus(hwnd: Handle) -> Handle;
    pub fn GetForegroundWindow() -> Handle;
    pub fn SetWindowPos(
        hwnd: Handle,
        after: Handle,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        flags: u32,
    ) -> i32;
    pub fn GetDC(hwnd: Handle) -> Handle;
    pub fn ReleaseDC(hwnd: Handle, dc: Handle) -> i32;
    pub fn GetClientRect(hwnd: Handle, rect: *mut Rect) -> i32;
    pub fn GetWindowRect(hwnd: Handle, rect: *mut Rect) -> i32;
    pub fn InvalidateRect(hwnd: Handle, rect: *const Rect, erase: i32) -> i32;
    pub fn UpdateWindow(hwnd: Handle) -> i32;
    pub fn BeginPaint(hwnd: Handle, ps: *mut PaintStruct) -> Handle;
    pub fn EndPaint(hwnd: Handle, ps: *const PaintStruct) -> i32;
    pub fn FillRect(dc: Handle, rect: *const Rect, brush: Handle) -> i32;
    pub fn DrawTextW(dc: Handle, text: *const u16, len: i32, rect: *mut Rect, flags: u32) -> i32;
    pub fn DrawIconEx(
        dc: Handle,
        x: i32,
        y: i32,
        icon: Handle,
        w: i32,
        h: i32,
        step: u32,
        brush: Handle,
        flags: u32,
    ) -> i32;
    pub fn LoadCursorW(instance: Handle, name: *const u16) -> Handle;
    pub fn LoadImageW(
        instance: Handle,
        name: *const u16,
        kind: u32,
        w: i32,
        h: i32,
        flags: u32,
    ) -> Handle;
    pub fn CreatePopupMenu() -> Handle;
    pub fn AppendMenuW(menu: Handle, flags: u32, id: usize, label: *const u16) -> i32;
    pub fn DestroyMenu(menu: Handle) -> i32;
    pub fn TrackPopupMenuEx(menu: Handle, flags: u32, x: i32, y: i32, hwnd: Handle, params: *const c_void) -> i32;
    pub fn CreateIconFromResourceEx(data: *mut u8, len: u32, icon: i32, version: u32, w: i32, h: i32, flags: u32) -> Handle;
    pub fn CreateIcon(instance: Handle, w: i32, h: i32, planes: u8, bits: u8, and: *const u8, xor: *const u8) -> Handle;
    pub fn DestroyIcon(icon: Handle) -> i32;
    pub fn PrivateExtractIconsW(path: *const u16, index: i32, cx: i32, cy: i32, icons: *mut Handle, ids: *mut u32, count: u32, flags: u32) -> u32;
    pub fn SetTimer(hwnd: Handle, id: usize, milliseconds: u32, callback: *const c_void) -> usize;
    pub fn KillTimer(hwnd: Handle, id: usize) -> i32;
    pub fn GetCursorPos(point: *mut Point) -> i32;
    pub fn ScreenToClient(hwnd: Handle, point: *mut Point) -> i32;
    pub fn ClientToScreen(hwnd: Handle, point: *mut Point) -> i32;
    pub fn SetCapture(hwnd: Handle) -> Handle;
    pub fn ReleaseCapture() -> i32;
    pub fn TrackMouseEvent(track: *mut TrackMouse) -> i32;
    pub fn MonitorFromWindow(hwnd: Handle, flags: u32) -> Handle;
    pub fn MonitorFromPoint(point: Point, flags: u32) -> Handle;
    pub fn GetMonitorInfoW(monitor: Handle, info: *mut MonitorInfo) -> i32;
    pub fn GetDpiForWindow(hwnd: Handle) -> u32;
    pub fn FindWindowW(class: *const u16, title: *const u16) -> Handle;
    pub fn GetSystemMetricsForDpi(index: i32, dpi: u32) -> i32;
    pub fn SetProcessDpiAwarenessContext(context: Handle) -> i32;
    pub fn RegisterWindowMessageW(text: *const u16) -> u32;
    pub fn MessageBoxW(hwnd: Handle, text: *const u16, caption: *const u16, flags: u32) -> i32;
    pub fn GetKeyState(key: i32) -> i16;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    pub fn SetStretchBltMode(dc: Handle, mode: i32) -> i32;
    pub fn SetBrushOrgEx(dc: Handle, x: i32, y: i32, old: *mut Point) -> i32;
    pub fn StretchBlt(dest: Handle, x: i32, y: i32, w: i32, h: i32, src: Handle, sx: i32, sy: i32, sw: i32, sh: i32, rop: u32) -> i32;
    pub fn CreateSolidBrush(color: u32) -> Handle;
    pub fn CreatePen(style: i32, width: i32, color: u32) -> Handle;
    pub fn SelectObject(dc: Handle, object: Handle) -> Handle;
    pub fn DeleteObject(object: Handle) -> i32;
    pub fn SetTextColor(dc: Handle, color: u32) -> u32;
    pub fn SetBkMode(dc: Handle, mode: i32) -> i32;
    pub fn CreateFontW(
        height: i32,
        width: i32,
        escape: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strikeout: u32,
        charset: u32,
        output_precision: u32,
        clip_precision: u32,
        quality: u32,
        pitch: u32,
        face: *const u16,
    ) -> Handle;
    pub fn CreateCompatibleDC(dc: Handle) -> Handle;
    pub fn CreateCompatibleBitmap(dc: Handle, w: i32, h: i32) -> Handle;
    pub fn CreateDIBSection(dc: Handle, info: *const BitmapHeader, usage: u32, bits: *mut *mut c_void, section: Handle, offset: u32) -> Handle;
    pub fn GdiFlush() -> i32;
    pub fn DeleteDC(dc: Handle) -> i32;
    pub fn BitBlt(
        dest: Handle,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        source: Handle,
        sx: i32,
        sy: i32,
        rop: u32,
    ) -> i32;
    pub fn MoveToEx(dc: Handle, x: i32, y: i32, old: *mut Point) -> i32;
    pub fn LineTo(dc: Handle, x: i32, y: i32) -> i32;
    pub fn Ellipse(dc: Handle, left: i32, top: i32, right: i32, bottom: i32) -> i32;
    pub fn RoundRect(
        dc: Handle,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        ew: i32,
        eh: i32,
    ) -> i32;
    pub fn Arc(
        dc: Handle,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        sx: i32,
        sy: i32,
        ex: i32,
        ey: i32,
    ) -> i32;
    pub fn SaveDC(dc: Handle) -> i32;
    pub fn RestoreDC(dc: Handle, saved: i32) -> i32;
    pub fn IntersectClipRect(dc: Handle, left: i32, top: i32, right: i32, bottom: i32) -> i32;
    pub fn GetStockObject(index: i32) -> Handle;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    pub fn GetModuleHandleW(name: *const u16) -> Handle;
    pub fn CreateMutexW(attributes: *const c_void, owner: i32, name: *const u16) -> Handle;
    pub fn CloseHandle(handle: Handle) -> i32;
    pub fn GetLastError() -> u32;
}

#[link(name = "shell32")]
unsafe extern "system" {
    pub fn Shell_NotifyIconW(message: u32, data: *const NotifyIcon) -> i32;
    pub fn Shell_NotifyIconGetRect(identifier: *const NotifyIconIdentifier, rect: *mut Rect) -> i32;
    pub fn ExtractIconExW(
        path: *const u16,
        index: i32,
        large: *mut Handle,
        small: *mut Handle,
        count: u32,
    ) -> u32;
}

pub const WM_DESTROY: u32 = 0x0002;
pub const WM_SIZE: u32 = 0x0005;
pub const WM_ACTIVATE: u32 = 0x0006;
pub const WM_PAINT: u32 = 0x000F;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_NCCREATE: u32 = 0x0081;
pub const WM_NCHITTEST: u32 = 0x0084;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_MOUSEWHEEL: u32 = 0x020A;
pub const WM_CAPTURECHANGED: u32 = 0x0215;
pub const WM_MOUSELEAVE: u32 = 0x02A3;
pub const WM_DPICHANGED: u32 = 0x02E0;
pub const WM_TRAY: u32 = 0x8001;
pub const WM_TRAY_WHEEL: u32 = 0x8006;
pub const WM_SHOW_PANEL: u32 = 0x8002;
pub const WM_DO_ACTION: u32 = 0x8003;
pub const GWLP_USERDATA: i32 = -21;
pub const WS_POPUP: u32 = 0x80000000;
pub const WS_CLIPCHILDREN: u32 = 0x02000000;
pub const WS_EX_TOOLWINDOW: u32 = 0x80;
pub const WS_EX_TOPMOST: u32 = 0x08;
pub const SW_HIDE: i32 = 0;
pub const SW_SHOW: i32 = 5;
pub const DT_LEFT: u32 = 0;
pub const DT_CENTER: u32 = 1;
pub const DT_RIGHT: u32 = 2;
pub const DT_VCENTER: u32 = 4;
pub const DT_SINGLELINE: u32 = 0x20;
pub const DT_END_ELLIPSIS: u32 = 0x8000;
pub const DT_NOPREFIX: u32 = 0x800;
pub const NIM_ADD: u32 = 0;
pub const NIM_DELETE: u32 = 2;
pub const NIF_MESSAGE: u32 = 1;
pub const NIF_ICON: u32 = 2;
pub const NIF_TIP: u32 = 4;

pub fn mouse_point(lp: isize) -> Point {
    Point {
        x: (lp as u16 as i16) as i32,
        y: ((lp as usize >> 16) as u16 as i16) as i32,
    }
}

pub const WM_TIMER: u32 = 0x0113;

#[link(name = "msimg32")]
unsafe extern "system" {
    pub fn AlphaBlend(dest: Handle, x: i32, y: i32, w: i32, h: i32, src: Handle, sx: i32, sy: i32, sw: i32, sh: i32, blend: u32) -> i32;
}
