use crate::win32::*;
use std::ptr::null_mut;

pub const BG: u32 = 0x171717;
pub const DEVICE_BG: u32 = 0x734A13;
pub const BLUE: u32 = 0xD68100;
pub const WHITE: u32 = 0xFFFFFF;
pub const MUTED: u32 = 0xB6B6B6;
pub const MENU_BG: u32 = 0x2B2B2B;

pub struct Canvas {
    pub dc: Handle,
    pub scale: f64,
    fonts: Vec<(i32, Handle)>,
}

impl Canvas {
    pub fn new(dc: Handle, scale: f64) -> Self {
        unsafe {
            SetBkMode(dc, 1);
        }
        Self {
            dc,
            scale,
            fonts: vec![],
        }
    }

    pub fn px(&self, n: i32) -> i32 {
        (n as f64 * self.scale).round() as i32
    }

    pub fn fill(&self, r: Rect, color: u32) {
        unsafe {
            let brush = CreateSolidBrush(color);
            let p = Rect {
                left: self.px(r.left),
                top: self.px(r.top),
                right: self.px(r.right),
                bottom: self.px(r.bottom),
            };
            FillRect(self.dc, &p, brush);
            DeleteObject(brush);
        }
    }

    pub fn text(&mut self, r: Rect, text: &str, size: i32, color: u32, align: u32) {
        let font = if let Some((_, font)) = self.fonts.iter().find(|(s, _)| *s == size) {
            *font
        } else {
            let font = unsafe {
                CreateFontW(
                    -self.px(size),
                    0,
                    0,
                    0,
                    400,
                    0,
                    0,
                    0,
                    1,
                    0,
                    0,
                    5,
                    0,
                    wide("Microsoft YaHei UI").as_ptr(),
                )
            };
            self.fonts.push((size, font));
            font
        };
        unsafe {
            let old = SelectObject(self.dc, font);
            SetTextColor(self.dc, color);
            let mut p = Rect {
                left: self.px(r.left),
                top: self.px(r.top),
                right: self.px(r.right),
                bottom: self.px(r.bottom),
            };
            let text = wide(text);
            DrawTextW(
                self.dc,
                text.as_ptr(),
                (text.len() - 1) as i32,
                &mut p,
                if align & 0x80000000 != 0 { (align & !0x80000000) | DT_NOPREFIX } else { align | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX },
            );
            SelectObject(self.dc, old);
        }
    }

    pub fn line(&self, points: &[(i32, i32)], color: u32, width: i32) {
        if points.len() < 2 {
            return;
        }
        unsafe {
            let pen = CreatePen(0, self.px(width).max(1), color);
            let old = SelectObject(self.dc, pen);
            MoveToEx(
                self.dc,
                self.px(points[0].0),
                self.px(points[0].1),
                null_mut(),
            );
            for &(x, y) in &points[1..] {
                LineTo(self.dc, self.px(x), self.px(y));
            }
            SelectObject(self.dc, old);
            DeleteObject(pen);
        }
    }

    pub fn rounded(&self, r: Rect, color: u32, radius: i32) {
        unsafe {
            let brush = CreateSolidBrush(color);
            let old_brush = SelectObject(self.dc, brush);
            let old_pen = SelectObject(self.dc, GetStockObject(8));
            RoundRect(
                self.dc,
                self.px(r.left),
                self.px(r.top),
                self.px(r.right),
                self.px(r.bottom),
                self.px(radius * 2),
                self.px(radius * 2),
            );
            SelectObject(self.dc, old_brush);
            SelectObject(self.dc, old_pen);
            DeleteObject(brush);
        }
    }

    pub fn circle(&self, r: Rect, color: u32) {
        unsafe {
            let brush = CreateSolidBrush(color);
            let old_brush = SelectObject(self.dc, brush);
            let old_pen = SelectObject(self.dc, GetStockObject(8));
            Ellipse(
                self.dc,
                self.px(r.left),
                self.px(r.top),
                self.px(r.right),
                self.px(r.bottom),
            );
            SelectObject(self.dc, old_brush);
            SelectObject(self.dc, old_pen);
            DeleteObject(brush);
        }
    }

    fn smooth<F: FnOnce(&mut Canvas)>(&self, rect: Rect, draw: F) {
        unsafe {
            let width = self.px(rect.right-rect.left);
            let height = self.px(rect.bottom-rect.top);
            if width <= 0 || height <= 0 { return; }
            let dc = CreateCompatibleDC(self.dc);
            let bitmap = CreateCompatibleBitmap(self.dc,width*3,height*3);
            if dc == 0 || bitmap == 0 {
                if bitmap != 0 { DeleteObject(bitmap); }
                if dc != 0 { DeleteDC(dc); }
                return;
            }
            let old = SelectObject(dc,bitmap);
            StretchBlt(dc,0,0,width*3,height*3,self.dc,self.px(rect.left),self.px(rect.top),width,height,0x00CC0020);
            { let mut canvas = Canvas::new(dc,self.scale*3.0); draw(&mut canvas); }
            let old_mode = SetStretchBltMode(self.dc,4);
            let mut origin = Point::default();
            SetBrushOrgEx(self.dc,0,0,&mut origin);
            StretchBlt(self.dc,self.px(rect.left),self.px(rect.top),width,height,dc,0,0,width*3,height*3,0x00CC0020);
            SetBrushOrgEx(self.dc,origin.x,origin.y,std::ptr::null_mut());
            SetStretchBltMode(self.dc,old_mode);
            SelectObject(dc,old); DeleteObject(bitmap); DeleteDC(dc);
        }
    }
    pub fn icon(&self, icon: usize, r: Rect) {
        if icon == 0 { return; }
        self.smooth(r, |c| unsafe {
            DrawIconEx(c.dc,0,0,icon as Handle,c.px(r.right-r.left),c.px(r.bottom-r.top),0,0,3);
        });
    }
    pub fn dim_icon(&self, icon: usize, r: Rect) {
        if icon == 0 { return; }
        self.smooth(r, |c| unsafe {
            let width = c.px(r.right-r.left);
            let height = c.px(r.bottom-r.top);
            let info = BitmapHeader { size: size_of::<BitmapHeader>() as u32,
                width, height: -height, planes: 1, bits: 32, compression: 0,
                image_size: 0, xppm: 0, yppm: 0, colors: 0, important: 0 };
            let black_dc = CreateCompatibleDC(c.dc);
            let white_dc = CreateCompatibleDC(c.dc);
            let mut black_bits = std::ptr::null_mut();
            let mut white_bits = std::ptr::null_mut();
            let black_bitmap = CreateDIBSection(c.dc,&info,0,&mut black_bits,0,0);
            let white_bitmap = CreateDIBSection(c.dc,&info,0,&mut white_bits,0,0);
            if black_dc == 0 || white_dc == 0 || black_bitmap == 0 || white_bitmap == 0 {
                if black_bitmap != 0 { DeleteObject(black_bitmap); }
                if white_bitmap != 0 { DeleteObject(white_bitmap); }
                if black_dc != 0 { DeleteDC(black_dc); }
                if white_dc != 0 { DeleteDC(white_dc); }
                DrawIconEx(c.dc,0,0,icon as Handle,width,height,0,0,3);
                return;
            }
            let old_black = SelectObject(black_dc,black_bitmap);
            let old_white = SelectObject(white_dc,white_bitmap);
            let length = (width*height*4) as usize;
            std::ptr::write_bytes(black_bits,0,length);
            std::ptr::write_bytes(white_bits,255,length);
            DrawIconEx(black_dc,0,0,icon as Handle,width,height,0,0,3);
            DrawIconEx(white_dc,0,0,icon as Handle,width,height,0,0,3);
            GdiFlush();
            let black = std::slice::from_raw_parts_mut(black_bits as *mut u8,length);
            let white = std::slice::from_raw_parts(white_bits as *const u8,length);
            // Two backgrounds recover coverage for both RGBA and legacy masked icons.
            // Darken only premultiplied foreground color; leave transparent pixels clear.
            for (b,w) in black.chunks_exact_mut(4).zip(white.chunks_exact(4)) {
                let difference = (0..3).map(|i| w[i].abs_diff(b[i])).max().unwrap_or(255);
                let alpha = 255-difference;
                for channel in &mut b[..3] { *channel = ((*channel as u32*115)/255).min(alpha as u32) as u8; }
                b[3] = alpha;
            }
            AlphaBlend(c.dc,0,0,width,height,black_dc,0,0,width,height,(255u32<<16)|(1u32<<24));
            SelectObject(black_dc,old_black); SelectObject(white_dc,old_white);
            DeleteObject(black_bitmap); DeleteObject(white_bitmap);
            DeleteDC(black_dc); DeleteDC(white_dc);
        });
    }
    pub fn volume_symbol(&self, x: i32, y: i32, icon: Handle) {
        unsafe {
            let size = self.px(26);
            let dc = CreateCompatibleDC(self.dc);
            let bitmap = CreateCompatibleBitmap(self.dc,size,size);
            if dc == 0 || bitmap == 0 { if bitmap != 0 { DeleteObject(bitmap); } if dc != 0 { DeleteDC(dc); } return; }
            let old = SelectObject(dc,bitmap);
            let canvas = Canvas::new(dc,self.scale);
            BitBlt(dc,0,0,size,size,self.dc,self.px(x),self.px(y),0x00CC0020);
            canvas.icon(icon as usize,Rect::new(0,0,26,26));
            AlphaBlend(self.dc,self.px(x),self.px(y),size,size,dc,0,0,size,size,210u32 << 16);
            SelectObject(dc,old); DeleteObject(bitmap); DeleteDC(dc);
        }
    }

    pub fn chevron(&self, x: i32, y: i32, collapsed: bool) {
        self.smooth(Rect::new(x-7,y-7,14,14), |c| {
            let points = if collapsed { [(4,2),(10,7),(4,12)] } else { [(2,4),(7,10),(12,4)] };
            c.line(&points,WHITE,1);
        });
    }

    pub fn slider(
        &mut self,
        y: i32,
        width: i32,
        value: u8,
        peak: f32,
        muted: bool,
        master: bool,
        focused: bool,
    ) {
        let start = 68;
        let end = width - 62;
        let knob = start + (end - start) * value as i32 / 100;
        self.smooth(Rect::new(60,y,width-116,44), |c| {
            c.fill(Rect::new(start-60,21,end-start,3),if master {0xAC8464} else {0x666666});
            c.fill(Rect::new(start-60,21,knob-start,3),if muted {0x808080} else {BLUE});
            if !muted && peak > 0.0 {
                let level = ((end-start) as f32*peak.clamp(0.0,value as f32/100.0)).round() as i32;
                c.fill(Rect::new(start-60,21,level,3),WHITE);
            }
            c.rounded(Rect::new(knob-64,10,8,24),if focused {WHITE} else if muted {0x999999} else {BLUE},4);
        });
        self.text(
            Rect::new(width - 57, y, 47, 44),
            &value.to_string(),
            19,
            if muted { MUTED } else { WHITE },
            DT_CENTER,
        );
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        for &(_, font) in &self.fonts {
            unsafe {
                DeleteObject(font);
            }
        }
    }
}

pub unsafe fn buffered<F: FnOnce(&mut Canvas, i32, i32)>(hwnd: Handle, scale: f64, draw: F) {
    unsafe {
        let mut ps: PaintStruct = std::mem::zeroed();
        let dc = BeginPaint(hwnd, &mut ps);
        let mut rect = Rect::default();
        GetClientRect(hwnd, &mut rect);
        if rect.right > 0 && rect.bottom > 0 {
            let buffer = CreateCompatibleDC(dc);
            let bitmap = CreateCompatibleBitmap(dc, rect.right, rect.bottom);
            if buffer != 0 && bitmap != 0 {
                let old = SelectObject(buffer, bitmap);
                {
                    let mut canvas = Canvas::new(buffer, scale);
                    draw(
                        &mut canvas,
                        (rect.right as f64 / scale).round() as i32,
                        (rect.bottom as f64 / scale).round() as i32,
                    );
                }
                BitBlt(dc, 0, 0, rect.right, rect.bottom, buffer, 0, 0, 0x00CC0020);
                SelectObject(buffer, old);
            } else {
                let mut canvas = Canvas::new(dc, scale);
                draw(
                    &mut canvas,
                    (rect.right as f64 / scale).round() as i32,
                    (rect.bottom as f64 / scale).round() as i32,
                );
            }
            if bitmap != 0 {
                DeleteObject(bitmap);
            }
            if buffer != 0 {
                DeleteDC(buffer);
            }
        }
        EndPaint(hwnd, &ps);
    }
}

pub fn volume_level(volume: u8, muted: bool) -> usize {
    if muted || volume == 0 { 0 } else if volume <= 33 { 1 } else if volume <= 66 { 2 } else { 3 }
}

pub fn volume_icon(_instance: Handle, level: usize) -> Handle {
    let data: &[u8] = match level {
        0 => include_bytes!("../resources/volume0.bin"),
        1 => include_bytes!("../resources/volume1.bin"),
        2 => include_bytes!("../resources/volume2.bin"),
        _ => include_bytes!("../resources/volume3.bin"),
    };
    unsafe { CreateIconFromResourceEx(data.as_ptr() as *mut u8,data.len() as u32,1,0x30000,128,128,0) }
}

pub fn tray_volume_icon(instance: Handle, level: usize, width: i32, height: i32) -> Handle {
    // Each group contains artwork sampled directly at the taskbar's pixel
    // size, rather than a single bitmap that Windows must rescale again.
    unsafe { LoadImageW(instance, (level.min(3) + 2) as *const u16, 1, width, height, 0) }
}

pub fn tooltip_size(hwnd: Handle, scale: f64, text: &str, max_width: i32) -> (i32,i32,String) {
    unsafe {
        let dc = GetDC(hwnd);
        let font = CreateFontW(-(11.0*scale).round() as i32,0,0,0,400,0,0,0,1,0,0,5,0,wide("Microsoft YaHei UI").as_ptr());
        let old = SelectObject(dc,font);
        let padding = (20.0*scale).round() as i32;
        let available = (max_width-padding).max(1);
        let mut display = String::new();
        let mut line = String::new();
        for ch in text.chars() {
            let candidate = format!("{line}{ch}");
            let value = wide(&candidate);
            let mut measure = Rect::default();
            DrawTextW(dc,value.as_ptr(),(value.len()-1) as i32,&mut measure,0x400 | DT_SINGLELINE | DT_NOPREFIX);
            if measure.right > available && !line.is_empty() {
                display.push_str(&line); display.push('\n'); line.clear();
            }
            line.push(ch);
        }
        display.push_str(&line);
        let mut rect = Rect::new(0,0,available,0);
        let value = wide(&display);
        DrawTextW(dc,value.as_ptr(),(value.len()-1) as i32,&mut rect,0x400 | 0x10 | DT_NOPREFIX);
        SelectObject(dc,old); DeleteObject(font); ReleaseDC(hwnd,dc);
        ((rect.right+padding).max((60.0*scale) as i32).min(max_width),rect.bottom+(12.0*scale).round() as i32,display)
    }
}
