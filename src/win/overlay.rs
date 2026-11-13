//! 궤적·HUD 오버레이.
//!
//! 방식: 레이어드 창 + 32bpp DIB + Direct2D DC 렌더 타깃 + `UpdateLayeredWindowIndirect`.
//! - 레이어드 창(`WS_EX_LAYERED | WS_EX_TRANSPARENT`)은 픽셀별 알파와 클릭 통과가 보장된다.
//! - 프레임마다 바뀐 영역만 지우고 다시 그린 뒤, 그 영역만 화면에 반영한다.
//! - DIB는 제스처를 시작한 모니터 크기로 만들고, 제스처가 끝나면 해제한다(확장 `Trail.release`와 같은 목적).
//!
//! 좌표: 훅은 물리 픽셀 화면 좌표를 준다. 렌더 타깃 DPI를 모니터 DPI로 맞추고
//! 모니터 기준 DIP 좌표로 그린다. HUD 크기·색은 확장 `content.js`의 CSS 값과 같다.

use std::ptr::{null, null_mut};

use glide_core::actions::{Hud, Tone};
use windows::Win32::Foundation::RECT as WinRect;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_HOLLOW,
    D2D1_FIGURE_END_OPEN, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_SOLID,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_LINE_JOIN_ROUND, D2D1_QUADRATIC_BEZIER_SEGMENT, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
    D2D1_STROKE_STYLE_PROPERTIES, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory,
    ID2D1DCRenderTarget, ID2D1Factory, ID2D1RenderTarget, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_MEDIUM,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_METRICS,
    DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat,
    IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::HDC as WinHdc;
use windows::core::{PCWSTR, w};
use windows_numerics::Vector2;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetMonitorInfoW,
    HBITMAP, HDC, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, SelectObject,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, HTTRANSPARENT, HWND_TOPMOST, RegisterClassExW, SW_HIDE,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos, ShowWindow, ULW_ALPHA,
    UPDATELAYEREDWINDOWINFO, UpdateLayeredWindowIndirect, WM_NCHITTEST, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use super::log;

/// 궤적 두께(DIP)와 색(#6b5bf0). 확장과 같다.
const TRAIL_WIDTH: f32 = 4.0;
const TRAIL_COLOR: D2D1_COLOR_F = rgb(0x6b, 0x5b, 0xf0, 1.0);
/// 이보다 가까운 새 점은 궤적에 넣지 않는다(DIP)
const MIN_POINT_DISTANCE: f32 = 1.5;

const fn rgb(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a,
    }
}

/// HUD 텍스트 형식(DIP 크기)
struct Formats {
    title: IDWriteTextFormat,
    title_muted: IDWriteTextFormat,
    detail: IDWriteTextFormat,
    sequence: IDWriteTextFormat,
}

/// 제스처 하나 동안만 유지하는 그리기 표면.
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    /// 모니터 왼쪽 위(화면 좌표, 물리 픽셀)와 크기
    origin: POINT,
    size: SIZE,
    /// 작업 표시줄을 뺀 작업 영역(모니터 기준 물리 픽셀). HUD를 이 안에 놓는다.
    work: RECT,
    scale: f32,
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: 이 표면이 만든 DC·비트맵을 원래 상태로 돌리고 해제한다.
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

/// 화면에 그린 HUD.
struct HudView {
    arrows: String,
    hud: Hud,
}

pub struct Overlay {
    window: HWND,
    factory: ID2D1Factory,
    target: ID2D1DCRenderTarget,
    stroke: ID2D1StrokeStyle,
    dwrite: IDWriteFactory,
    formats: Formats,
    surface: Option<Surface>,
    shown: bool,
    /// 모니터 기준 DIP 좌표의 궤적 점
    points: Vec<Vector2>,
    /// 이전 프레임까지 그린 점 수
    drawn: usize,
    /// 지금까지 그린 궤적이 차지하는 영역(DIP)
    trail_bounds: Option<D2D_RECT_F>,
    hud: Option<HudView>,
    hud_rect: Option<D2D_RECT_F>,
    /// 다음 프레임에 다시 그릴 영역(DIP)
    dirty: Option<D2D_RECT_F>,
    /// 다음 프레임에 HUD 배치를 다시 계산해야 함
    hud_changed: bool,
}

unsafe extern "system" fn overlay_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCHITTEST {
        // 마우스 입력은 항상 아래 창으로 통과시킨다.
        return HTTRANSPARENT as LRESULT;
    }
    // SAFETY: 처리하지 않은 메시지를 기본 처리기로 넘긴다.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

impl Overlay {
    /// UI 스레드에서 한 번 만든다. 창은 숨긴 상태로 시작한다.
    pub fn new() -> windows::core::Result<Self> {
        let class_name: Vec<u16> = "GlideOverlay\0".encode_utf16().collect();
        // SAFETY: 정적 창 프로시저와 널 종료 클래스 이름으로 창 클래스를 등록하고 창을 만든다.
        let window = unsafe {
            let instance = GetModuleHandleW(null());
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(overlay_proc),
                hInstance: instance,
                lpszClassName: class_name.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassExW(&class);
            CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE,
                class_name.as_ptr(),
                class_name.as_ptr(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                null_mut(),
                null_mut(),
                instance,
                null(),
            )
        };
        if window.is_null() {
            return Err(windows::core::Error::from_thread());
        }
        // SAFETY: Direct2D·DirectWrite 팩터리와 렌더 타깃을 만든다. 모두 이 UI 스레드에서만 쓴다.
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let target = create_target(&factory)?;
            let stroke = factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_SOLID,
                    dashOffset: 0.0,
                },
                None,
            )?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let formats = Formats {
                title: text_format(&dwrite, DWRITE_FONT_WEIGHT_SEMI_BOLD, 15.0)?,
                title_muted: text_format(&dwrite, DWRITE_FONT_WEIGHT_MEDIUM, 15.0)?,
                detail: text_format(&dwrite, DWRITE_FONT_WEIGHT_NORMAL, 12.0)?,
                sequence: text_format(&dwrite, DWRITE_FONT_WEIGHT_BOLD, 18.0)?,
            };
            Ok(Self {
                window,
                factory,
                target,
                stroke,
                dwrite,
                formats,
                surface: None,
                shown: false,
                points: Vec::new(),
                drawn: 0,
                trail_bounds: None,
                hud: None,
                hud_rect: None,
                dirty: None,
                hud_changed: false,
            })
        }
    }

    /// 제스처를 그리는 중이거나 결과를 보여 주는 중인지.
    pub fn is_visible(&self) -> bool {
        self.surface.is_some()
    }

    /// 새 제스처를 시작한다. `x`, `y`는 오른쪽 버튼을 누른 화면 좌표(물리 픽셀)다.
    pub fn begin(&mut self, x: i32, y: i32) {
        self.hide();
        match self.create_surface(POINT { x, y }) {
            Ok(surface) => {
                let start = local(&surface, x, y);
                self.surface = Some(surface);
                self.points.push(start);
            }
            Err(error) => {
                log::warn(|| format!("[오버레이] 그리기 표면을 만들지 못했습니다: {error}"))
            }
        }
    }

    pub fn add_point(&mut self, x: i32, y: i32) {
        let Some(surface) = &self.surface else { return };
        let point = local(surface, x, y);
        // 1000Hz 마우스는 거의 같은 점을 많이 보낸다. 눈에 보이지 않는 차이는 버려 그리기 비용을 줄인다.
        let close = self.points.last().is_some_and(|last| {
            let (dx, dy) = (point.X - last.X, point.Y - last.Y);
            dx * dx + dy * dy < MIN_POINT_DISTANCE * MIN_POINT_DISTANCE
        });
        if !close {
            self.points.push(point);
        }
    }

    /// 손을 뗀 뒤: 궤적은 지우고 HUD만 남긴다(확장과 같음).
    pub fn clear_trail(&mut self) {
        if let Some(bounds) = self.trail_bounds.take() {
            self.mark_dirty(bounds);
        }
        self.points.clear();
        self.drawn = 0;
    }

    pub fn set_hud(&mut self, arrows: String, hud: Hud) {
        if self.surface.is_none() {
            return;
        }
        if self
            .hud
            .as_ref()
            .is_some_and(|current| current.arrows == arrows && current.hud == hud)
        {
            return;
        }
        self.hud = Some(HudView { arrows, hud });
        self.hud_changed = true;
    }

    /// 창을 숨기고 표면을 해제한다.
    pub fn hide(&mut self) {
        if self.shown {
            // SAFETY: 이 UI 스레드가 만든 창이다.
            unsafe { ShowWindow(self.window, SW_HIDE) };
            self.shown = false;
        }
        self.surface = None;
        self.points.clear();
        self.drawn = 0;
        self.trail_bounds = None;
        self.hud = None;
        self.hud_rect = None;
        self.dirty = None;
        self.hud_changed = false;
    }

    fn mark_dirty(&mut self, rect: D2D_RECT_F) {
        self.dirty = Some(match self.dirty {
            Some(current) => union(current, rect),
            None => rect,
        });
    }

    fn create_surface(&self, point: POINT) -> Result<Surface, String> {
        // SAFETY: 모니터 정보 조회와 DIB·메모리 DC 생성이다. 실패하면 만든 것을 정리한다.
        unsafe {
            let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..std::mem::zeroed()
            };
            if GetMonitorInfoW(monitor, &mut info) == 0 {
                return Err("모니터 정보를 읽지 못했습니다".into());
            }
            let (mut dpi_x, mut dpi_y) = (96, 96);
            if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) < 0 {
                dpi_x = 96;
            }
            let rect = info.rcMonitor;
            let size = SIZE {
                cx: rect.right - rect.left,
                cy: rect.bottom - rect.top,
            };
            let header = BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size.cx,
                // 음수 높이 = 위에서 아래로 쌓는 DIB
                biHeight: -size.cy,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..std::mem::zeroed()
            };
            let bitmap_info = BITMAPINFO {
                bmiHeader: header,
                ..std::mem::zeroed()
            };
            let dc = CreateCompatibleDC(null_mut());
            if dc.is_null() {
                return Err("메모리 DC를 만들지 못했습니다".into());
            }
            let mut bits = null_mut();
            let bitmap =
                CreateDIBSection(dc, &bitmap_info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if bitmap.is_null() {
                DeleteDC(dc);
                return Err("그리기 비트맵을 만들지 못했습니다".into());
            }
            let previous = SelectObject(dc, bitmap);
            let surface = Surface {
                dc,
                bitmap,
                previous,
                origin: POINT {
                    x: rect.left,
                    y: rect.top,
                },
                size,
                work: RECT {
                    left: info.rcWork.left - rect.left,
                    top: info.rcWork.top - rect.top,
                    right: info.rcWork.right - rect.left,
                    bottom: info.rcWork.bottom - rect.top,
                },
                scale: dpi_x as f32 / 96.0,
            };
            let bind = WinRect {
                left: 0,
                top: 0,
                right: size.cx,
                bottom: size.cy,
            };
            self.target
                .BindDC(WinHdc(dc), &bind)
                .map_err(|error| error.to_string())?;
            self.target.SetDpi(dpi_x as f32, dpi_x as f32);
            Ok(surface)
        }
    }

    /// 쌓인 변경을 한 번에 그린다. UI 스레드가 사건을 모두 처리한 뒤 부른다.
    pub fn render(&mut self) {
        if self.surface.is_none() {
            return;
        }
        // 새로 들어온 궤적 구간(직전 점부터)을 다시 그릴 영역에 넣는다.
        if self.points.len() > self.drawn {
            let from = self.drawn.saturating_sub(2);
            if let Some(bounds) = bounds_of(&self.points[from..], TRAIL_WIDTH) {
                self.mark_dirty(bounds);
                self.trail_bounds = Some(
                    self.trail_bounds
                        .map_or(bounds, |current| union(current, bounds)),
                );
            }
        }
        let mut layouts = None;
        if self.hud_changed {
            if let Some(old) = self.hud_rect.take() {
                self.mark_dirty(old);
            }
            layouts = self.layout_hud();
            if let Some((rect, ..)) = &layouts {
                self.hud_rect = Some(*rect);
                self.mark_dirty(*rect);
            }
            self.hud_changed = false;
        } else if let Some(rect) = self.hud_rect
            && self.dirty.is_some_and(|dirty| intersects(dirty, rect))
        {
            // 궤적이 HUD 아래를 지나가면 HUD를 다시 위에 그린다.
            layouts = self.layout_hud();
        }
        let Some(dirty) = self.dirty.take() else {
            return;
        };
        if let Err(error) = self.draw(dirty, layouts) {
            log::warn(|| format!("[오버레이] 그리기 실패: {error}"));
            // 렌더 타깃을 다시 만들고 이번 제스처 표시는 접는다.
            // SAFETY: 같은 팩터리로 새 DC 렌더 타깃을 만든다.
            if let Ok(target) = unsafe { create_target(&self.factory) } {
                self.target = target;
            }
            self.hide();
            return;
        }
        self.drawn = self.points.len();
        self.present(dirty);
    }

    fn draw(
        &self,
        dirty: D2D_RECT_F,
        layouts: Option<(D2D_RECT_F, HudLayouts)>,
    ) -> windows::core::Result<()> {
        let target: &ID2D1RenderTarget = &self.target;
        // SAFETY: BindDC로 연결한 표면에 그린다. BeginDraw와 EndDraw를 짝지어 호출한다.
        unsafe {
            target.BeginDraw();
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            target.PushAxisAlignedClip(&dirty, D2D1_ANTIALIAS_MODE_ALIASED);
            target.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }));
            let drawn = self
                .draw_trail(target, dirty)
                .and_then(|()| match (&self.hud, layouts) {
                    (Some(view), Some((rect, layouts))) => {
                        self.draw_hud(target, view, rect, &layouts)
                    }
                    _ => Ok(()),
                });
            target.PopAxisAlignedClip();
            let ended = target.EndDraw(None, None);
            drawn.and(ended)
        }
    }

    /// 확장 `Trail.draw`와 같은 곡선: 이전 점을 제어점으로, 두 점의 중점까지 2차 곡선으로 잇는다.
    /// 다시 그릴 영역(`dirty`)에 걸치는 구간만 경로에 넣어, 긴 제스처에서도 프레임 비용이 늘지 않게 한다.
    /// 궤적 색은 불투명이라 구간이 끊겨 겹쳐 그려져도 색이 진해지지 않는다.
    fn draw_trail(
        &self,
        target: &ID2D1RenderTarget,
        dirty: D2D_RECT_F,
    ) -> windows::core::Result<()> {
        if self.points.len() < 2 {
            return Ok(());
        }
        // 선 두께만큼 넓힌 영역에 구간의 세 점(시작·제어·끝) 상자가 걸치면 그린다.
        let area = inflate(dirty, TRAIL_WIDTH / 2.0 + 1.0);
        let touches = |a: Vector2, b: Vector2, c: Vector2| {
            intersects(
                area,
                D2D_RECT_F {
                    left: a.X.min(b.X).min(c.X),
                    top: a.Y.min(b.Y).min(c.Y),
                    right: a.X.max(b.X).max(c.X),
                    bottom: a.Y.max(b.Y).max(c.Y),
                },
            )
        };
        // SAFETY: 이 팩터리로 만든 경로 기하를 같은 스레드에서 그린다.
        unsafe {
            let path = self.factory.CreatePathGeometry()?;
            let sink = path.Open()?;
            let mut open = false;
            let mut start = self.points[0];
            for pair in self.points.windows(2) {
                let (last, point) = (pair[0], pair[1]);
                let middle = Vector2 {
                    X: (last.X + point.X) / 2.0,
                    Y: (last.Y + point.Y) / 2.0,
                };
                if touches(start, last, middle) {
                    if !open {
                        sink.BeginFigure(start, D2D1_FIGURE_BEGIN_HOLLOW);
                        open = true;
                    }
                    sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT {
                        point1: last,
                        point2: middle,
                    });
                } else if open {
                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    open = false;
                }
                start = middle;
            }
            // 마지막 중점에서 마지막 점까지 직선
            if let Some(last) = self.points.last().copied()
                && touches(start, last, last)
            {
                if !open {
                    sink.BeginFigure(start, D2D1_FIGURE_BEGIN_HOLLOW);
                    open = true;
                }
                sink.AddLine(last);
            }
            if open {
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            sink.Close()?;
            let brush = target.CreateSolidColorBrush(&TRAIL_COLOR, None)?;
            target.DrawGeometry(&path, &brush, TRAIL_WIDTH, &self.stroke);
        }
        Ok(())
    }

    /// HUD 배치를 계산한다. 반환값: (HUD 영역, 텍스트 배치)
    fn layout_hud(&self) -> Option<(D2D_RECT_F, HudLayouts)> {
        let view = self.hud.as_ref()?;
        let surface = self.surface.as_ref()?;
        // 작업 영역(작업 표시줄 제외) 기준으로 놓는다. 확장은 브라우저 화면 기준이다.
        let work_left = surface.work.left as f32 / surface.scale;
        let work_width = (surface.work.right - surface.work.left) as f32 / surface.scale;
        let work_bottom = surface.work.bottom as f32 / surface.scale;
        // CSS: max-width: min(85%, 560px); padding: 10px 20px 10px 12px; gap: 14px; bottom: 40px
        let max_width = (work_width * 0.85).min(560.0);
        let sequence = if view.arrows.is_empty() {
            None
        } else {
            self.text_layout(&view.arrows, &self.formats.sequence, 400.0)
        };
        let sequence_width = sequence.as_ref().map_or(0.0, |(_, m)| m.width + 20.0);
        let sequence_height = sequence.as_ref().map_or(0.0, |(_, m)| m.height + 8.0);
        let gap = if sequence.is_some() { 14.0 } else { 0.0 };
        let text_width = (max_width - 12.0 - 20.0 - sequence_width - gap).max(80.0);
        let title_format = if view.hud.tone == Tone::Muted {
            &self.formats.title_muted
        } else {
            &self.formats.title
        };
        let title = self.text_layout(&view.hud.title, title_format, text_width)?;
        let detail = if view.hud.detail.is_empty() {
            None
        } else {
            self.text_layout(&view.hud.detail, &self.formats.detail, text_width)
        };
        let text_block_width = title
            .1
            .width
            .max(detail.as_ref().map_or(0.0, |(_, m)| m.width));
        let text_block_height = title.1.height + detail.as_ref().map_or(0.0, |(_, m)| m.height);
        let inner_height = text_block_height.max(sequence_height);
        let box_width = 12.0 + sequence_width + gap + text_block_width + 20.0;
        let box_height = inner_height + 20.0;
        let left = (work_left + (work_width - box_width) / 2.0).floor();
        let top = (work_bottom - 40.0 - box_height).floor();
        let rect = D2D_RECT_F {
            left,
            top,
            right: left + box_width,
            bottom: top + box_height,
        };
        Some((
            rect,
            HudLayouts {
                sequence,
                title,
                detail,
                inner_height,
            },
        ))
    }

    fn text_layout(
        &self,
        text: &str,
        format: &IDWriteTextFormat,
        max_width: f32,
    ) -> Option<(IDWriteTextLayout, DWRITE_TEXT_METRICS)> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: UTF-16 문자열 슬라이스로 텍스트 배치를 만들고 크기를 잰다.
        unsafe {
            let layout = self
                .dwrite
                .CreateTextLayout(&wide, format, max_width, 200.0)
                .ok()?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics).ok()?;
            Some((layout, metrics))
        }
    }

    fn draw_hud(
        &self,
        target: &ID2D1RenderTarget,
        view: &HudView,
        rect: D2D_RECT_F,
        layouts: &HudLayouts,
    ) -> windows::core::Result<()> {
        let muted = view.hud.tone == Tone::Muted;
        // SAFETY: 같은 렌더 타깃에서 만든 브러시로 그린다.
        unsafe {
            // 상자: background #161520f0, border 1px #ffffff24, radius 16px
            let background =
                target.CreateSolidColorBrush(&rgb(0x16, 0x15, 0x20, 0xf0 as f32 / 255.0), None)?;
            let border =
                target.CreateSolidColorBrush(&rgb(0xff, 0xff, 0xff, 0x24 as f32 / 255.0), None)?;
            let shape = D2D1_ROUNDED_RECT {
                rect,
                radiusX: 16.0,
                radiusY: 16.0,
            };
            target.FillRoundedRectangle(&shape, &background);
            let inset = D2D1_ROUNDED_RECT {
                rect: inflate(rect, -0.5),
                radiusX: 15.5,
                radiusY: 15.5,
            };
            target.DrawRoundedRectangle(&inset, &border, 1.0, None::<&ID2D1StrokeStyle>);

            let center_y = rect.top + 10.0 + layouts.inner_height / 2.0;
            let mut x = rect.left + 12.0;
            if let Some((layout, metrics)) = &layouts.sequence {
                // 방향 알약: padding 4px 10px, radius 10px
                let pill_alpha = if muted { 0x0f } else { 0x1a } as f32 / 255.0;
                let pill =
                    target.CreateSolidColorBrush(&rgb(0xff, 0xff, 0xff, pill_alpha), None)?;
                let pill_height = metrics.height + 8.0;
                let pill_rect = D2D_RECT_F {
                    left: x,
                    top: center_y - pill_height / 2.0,
                    right: x + metrics.width + 20.0,
                    bottom: center_y + pill_height / 2.0,
                };
                target.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: pill_rect,
                        radiusX: 10.0,
                        radiusY: 10.0,
                    },
                    &pill,
                );
                let color = if muted {
                    rgb(0xa3, 0xa2, 0xb4, 1.0)
                } else {
                    rgb(0xc9, 0xc1, 0xff, 1.0)
                };
                let ink = target.CreateSolidColorBrush(&color, None)?;
                target.DrawTextLayout(
                    Vector2 {
                        X: x + 10.0,
                        Y: pill_rect.top + 4.0,
                    },
                    layout,
                    &ink,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
                x = pill_rect.right + 14.0;
            }
            let (title_layout, title_metrics) = &layouts.title;
            let detail_height = layouts.detail.as_ref().map_or(0.0, |(_, m)| m.height);
            let mut y = center_y - (title_metrics.height + detail_height) / 2.0;
            let title_color = match view.hud.tone {
                Tone::Ok => rgb(0xff, 0xff, 0xff, 1.0),
                Tone::Muted => rgb(0xd8, 0xd7, 0xe4, 1.0),
                Tone::Error => rgb(0xff, 0xb3, 0xc1, 1.0),
            };
            let title_ink = target.CreateSolidColorBrush(&title_color, None)?;
            target.DrawTextLayout(
                Vector2 { X: x, Y: y },
                title_layout,
                &title_ink,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
            y += title_metrics.height;
            if let Some((layout, _)) = &layouts.detail {
                let detail_ink = target.CreateSolidColorBrush(&rgb(0xb9, 0xb8, 0xc8, 1.0), None)?;
                target.DrawTextLayout(
                    Vector2 { X: x, Y: y },
                    layout,
                    &detail_ink,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
        }
        Ok(())
    }

    /// 그린 영역을 화면에 반영한다. 처음 보일 때는 전체를 반영하고 창을 띄운다.
    fn present(&mut self, dirty: D2D_RECT_F) {
        let Some(surface) = &self.surface else { return };
        let scale = surface.scale;
        let pixels = RECT {
            left: ((dirty.left * scale).floor() as i32 - 1).clamp(0, surface.size.cx),
            top: ((dirty.top * scale).floor() as i32 - 1).clamp(0, surface.size.cy),
            right: ((dirty.right * scale).ceil() as i32 + 1).clamp(0, surface.size.cx),
            bottom: ((dirty.bottom * scale).ceil() as i32 + 1).clamp(0, surface.size.cy),
        };
        let source = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let info = UPDATELAYEREDWINDOWINFO {
            cbSize: size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
            hdcDst: null_mut(),
            pptDst: &surface.origin,
            psize: &surface.size,
            hdcSrc: surface.dc,
            pptSrc: &source,
            crKey: 0,
            pblend: &blend,
            dwFlags: ULW_ALPHA,
            // 처음에는 창 전체(새 DIB는 투명)를, 이후에는 바뀐 영역만 반영한다.
            prcDirty: if self.shown { &pixels } else { null() },
        };
        // SAFETY: 이 스레드의 레이어드 창에 같은 크기의 DIB를 반영한다.
        unsafe {
            if UpdateLayeredWindowIndirect(self.window, &info) == 0 {
                log::warn(|| "[오버레이] 화면 반영에 실패했습니다".into());
                return;
            }
            if !self.shown {
                SetWindowPos(
                    self.window,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                );
                ShowWindow(self.window, SW_SHOWNOACTIVATE);
                self.shown = true;
            }
        }
    }
}

struct HudLayouts {
    sequence: Option<(IDWriteTextLayout, DWRITE_TEXT_METRICS)>,
    title: (IDWriteTextLayout, DWRITE_TEXT_METRICS),
    detail: Option<(IDWriteTextLayout, DWRITE_TEXT_METRICS)>,
    inner_height: f32,
}

/// 소프트웨어 DC 렌더 타깃. 바뀐 작은 영역만 자주 그리므로 GPU 왕복(읽어 오기)보다 싸다.
unsafe fn create_target(factory: &ID2D1Factory) -> windows::core::Result<ID2D1DCRenderTarget> {
    let properties = D2D1_RENDER_TARGET_PROPERTIES {
        r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        usage: D2D1_RENDER_TARGET_USAGE_NONE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    };
    // SAFETY: 유효한 속성 구조체를 넘긴다.
    unsafe { factory.CreateDCRenderTarget(&properties) }
}

/// "Segoe UI" 텍스트 형식. 한글은 DirectWrite 시스템 대체 글꼴(맑은 고딕 등)이 그린다.
unsafe fn text_format(
    dwrite: &IDWriteFactory,
    weight: DWRITE_FONT_WEIGHT,
    size: f32,
) -> windows::core::Result<IDWriteTextFormat> {
    // SAFETY: 정적 널 종료 문자열을 넘긴다.
    unsafe {
        dwrite.CreateTextFormat(
            w!("Segoe UI"),
            None::<&IDWriteFontCollection>,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            PCWSTR(w!("ko-kr").as_ptr()),
        )
    }
}

/// 화면 좌표(물리 픽셀) → 모니터 기준 DIP 좌표
fn local(surface: &Surface, x: i32, y: i32) -> Vector2 {
    Vector2 {
        X: (x - surface.origin.x) as f32 / surface.scale,
        Y: (y - surface.origin.y) as f32 / surface.scale,
    }
}

fn bounds_of(points: &[Vector2], width: f32) -> Option<D2D_RECT_F> {
    let first = points.first()?;
    let mut rect = D2D_RECT_F {
        left: first.X,
        top: first.Y,
        right: first.X,
        bottom: first.Y,
    };
    for point in points {
        rect.left = rect.left.min(point.X);
        rect.top = rect.top.min(point.Y);
        rect.right = rect.right.max(point.X);
        rect.bottom = rect.bottom.max(point.Y);
    }
    Some(inflate(rect, width / 2.0 + 2.0))
}

fn inflate(rect: D2D_RECT_F, by: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left: rect.left - by,
        top: rect.top - by,
        right: rect.right + by,
        bottom: rect.bottom + by,
    }
}

fn union(a: D2D_RECT_F, b: D2D_RECT_F) -> D2D_RECT_F {
    D2D_RECT_F {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    }
}

fn intersects(a: D2D_RECT_F, b: D2D_RECT_F) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}
