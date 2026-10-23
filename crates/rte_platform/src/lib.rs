//! Win32 platform layer: a frameless top-level window that presents a CPU
//! framebuffer, plus input events, cross-thread wake-ups and startup timing.
//!
//! Everything runs on the thread that created the window. The application
//! is called through [`App`]; requests it makes during a call (close, redraw,
//! title) are applied after the call returns, so window procedures never
//! re-enter the application.

#![allow(unsafe_code)] // Win32 FFI.

mod clipboard;
pub mod files;

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{FILETIME, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_CLOAK, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmExtendFrameIntoClientArea,
    DwmFlush, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, EndPaint, HBITMAP, HDC, HGDIOBJ, InvalidateRect,
    PAINTSTRUCT, SRCCOPY, ScreenToClient, SelectObject, UpdateWindow,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
use windows_sys::Win32::UI::Controls::MARGINS;
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetCapture, GetDoubleClickTime, GetKeyState, ReleaseCapture, SetCapture, VK_CONTROL, VK_MENU,
    VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
    GetClientRect, GetCursorPos, GetMessageTime, GetMessageW, GetSystemMetrics, GetWindowLongPtrW,
    HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT,
    HTTOPRIGHT, IDC_ARROW, IDC_IBEAM, IMAGE_ICON, IsZoomed, LR_DEFAULTCOLOR, LoadCursorW,
    LoadImageW, MINMAXINFO, MSG, NCCALCSIZE_PARAMS, PostMessageW, PostQuitMessage,
    RegisterClassExW, SM_CXDOUBLECLK, SM_CXICON, SM_CXPADDEDBORDER, SM_CXSIZEFRAME, SM_CXSMICON,
    SM_CYDOUBLECLK, SM_CYICON, SM_CYSIZEFRAME, SM_CYSMICON, SW_SHOW, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetCursor, SetWindowLongPtrW,
    SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage, WM_APP, WM_CAPTURECHANGED, WM_CHAR,
    WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCACTIVATE,
    WM_NCCALCSIZE, WM_NCHITTEST, WM_PAINT, WM_SETCURSOR, WM_SIZE, WM_SYSKEYDOWN, WNDCLASSEXW,
    WS_OVERLAPPEDWINDOW,
};

const WM_WAKE: u32 = WM_APP + 1;
const CLASS_NAME: &str = "RoyiTextEditorWindow";
/// Icon group embedded in the executable by `rte_app`'s build script.
const APP_ICON: u16 = 1;
/// `wParam` flag of mouse messages: left button down.
const MK_LBUTTON: usize = 0x0001;

/// DPI of the primary monitor. Per-monitor v2 awareness comes from the
/// application manifest.
pub fn system_dpi() -> u32 {
    // SAFETY: plain Win32 call without pointers.
    unsafe { GetDpiForSystem() }
}

/// Milliseconds since the OS created this process.
pub fn ms_since_process_start() -> f64 {
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user, mut now) = (zero, zero, zero, zero, zero);
    // SAFETY: all pointers refer to live stack values.
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        );
        GetSystemTimePreciseAsFileTime(&mut now);
    }
    ticks(now).saturating_sub(ticks(created)) as f64 / 10_000.0
}

/// Blocks until DWM has composed the next frame.
pub fn wait_for_composition() {
    // SAFETY: plain Win32 call without pointers.
    unsafe {
        DwmFlush();
    }
}

/// Peak private (commit) bytes of this process so far (`PeakPagefileUsage`).
pub fn peak_private_bytes() -> u64 {
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    counters.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: `counters` is a live, correctly sized buffer.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    if ok == 0 {
        0
    } else {
        counters.PeakPagefileUsage as u64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    /// Windows virtual-key code.
    pub vk: u16,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The first frame is on screen and input is accepted.
    Shown,
    Resized {
        width: u32,
        height: u32,
    },
    DpiChanged(u32),
    Key(Key),
    Char(char),
    /// Wheel rotation in units of 120 per notch.
    Wheel {
        delta: i32,
        horizontal: bool,
        shift: bool,
        ctrl: bool,
    },
    /// Left button pressed in the client area; `clicks` counts 1-3 for
    /// single, double and triple clicks. The mouse is captured until
    /// [`Event::MouseUp`].
    MouseDown {
        x: i32,
        y: i32,
        clicks: u32,
        shift: bool,
        ctrl: bool,
        alt: bool,
    },
    /// Pointer movement while the left button is held (also outside the window).
    MouseMove {
        x: i32,
        y: i32,
    },
    /// The left button was released or the capture was lost.
    MouseUp,
    CloseRequested,
    /// Posted by a [`Waker`] from another thread.
    Wake,
}

/// Window state visible to the application, plus deferred requests.
pub struct Ctx {
    width: u32,
    height: u32,
    dpi: u32,
    redraw: bool,
    close: bool,
    title: Option<String>,
    hwnd: HWND,
    /// Clipboard of a headless context: text and private data.
    memory_clipboard: Option<(String, Vec<u8>)>,
}

impl Ctx {
    /// A context without a window, for driving an [`App`] in tests. Its
    /// clipboard is held in memory.
    pub fn headless(width: u32, height: u32, dpi: u32) -> Ctx {
        Ctx {
            width,
            height,
            dpi,
            redraw: false,
            close: false,
            title: None,
            hwnd: null_mut(),
            memory_clipboard: None,
        }
    }

    pub fn close_requested(&self) -> bool {
        self.close
    }

    pub fn redraw_requested(&self) -> bool {
        self.redraw
    }

    /// Title requested during this call, if any.
    pub fn requested_title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Clipboard text plus the data of the private `format`, if present.
    pub fn clipboard(&mut self, format: &str) -> Option<(String, Option<Vec<u8>>)> {
        if self.hwnd.is_null() {
            return self
                .memory_clipboard
                .clone()
                .map(|(text, data)| (text, Some(data)));
        }
        clipboard::read(self.hwnd, format)
    }

    /// Replaces the clipboard with `text` plus `data` in the private `format`.
    pub fn set_clipboard(&mut self, text: &str, format: &str, data: &[u8]) -> bool {
        if self.hwnd.is_null() {
            self.memory_clipboard = Some((text.to_owned(), data.to_vec()));
            return true;
        }
        clipboard::write(self.hwnd, text, format, data)
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn dpi(&self) -> u32 {
        self.dpi
    }

    pub fn redraw(&mut self) {
        self.redraw = true;
    }

    pub fn close(&mut self) {
        self.close = true;
    }

    pub fn set_title(&mut self, title: &str) {
        self.title = Some(title.to_owned());
    }
}

pub trait App {
    /// Handles an event; returns `false` to get the default system behavior
    /// (e.g. `Alt+F4` for an unhandled system key, closing for `CloseRequested`).
    fn event(&mut self, event: Event, ctx: &mut Ctx) -> bool;

    /// Draws the whole client area into `pixels` (`0xAARRGGBB`, top-down rows).
    fn paint(&mut self, pixels: &mut [u32], width: u32, height: u32);

    /// True when the client point belongs to the draggable title strip.
    fn is_caption(&self, x: i32, y: i32) -> bool;

    /// True when the pointer shows the text (I-beam) cursor at the client point.
    fn is_text(&self, _x: i32, _y: i32) -> bool {
        false
    }
}

/// Wakes the window thread from any thread; it receives [`Event::Wake`].
#[derive(Clone, Copy, Debug)]
pub struct Waker(isize);

impl Waker {
    pub fn wake(&self) {
        // SAFETY: posting to a possibly destroyed window fails harmlessly.
        unsafe {
            PostMessageW(self.0 as HWND, WM_WAKE, 0, 0);
        }
    }
}

#[derive(Debug)]
pub struct PlatformError(pub &'static str);

impl std::fmt::Display for PlatformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for PlatformError {}

/// Off-screen 32-bit DIB section selected into a memory DC.
struct Dib {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u32,
    width: u32,
    height: u32,
}

impl Dib {
    fn new(width: u32, height: u32) -> Option<Dib> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32), // Top-down rows.
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [Default::default()],
        };
        let mut bits: *mut c_void = null_mut();
        // SAFETY: `info` and `bits` outlive the calls; handles are checked.
        unsafe {
            let dc = CreateCompatibleDC(null_mut());
            if dc.is_null() {
                return None;
            }
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if bitmap.is_null() || bits.is_null() {
                DeleteDC(dc);
                return None;
            }
            let previous = SelectObject(dc, bitmap);
            Some(Dib {
                dc,
                bitmap,
                previous,
                bits: bits.cast(),
                width,
                height,
            })
        }
    }

    fn pixels(&mut self) -> &mut [u32] {
        // SAFETY: the DIB section holds exactly width * height 32-bit pixels
        // and lives as long as `self`.
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.width * self.height) as usize) }
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        // SAFETY: handles were created by `Dib::new` and are released once.
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

struct State {
    app: Option<Box<dyn App>>,
    dib: Option<Dib>,
    width: u32,
    height: u32,
    dpi: u32,
    high_surrogate: Option<u16>,
    /// Time, position and count of the last left click.
    last_click: (u32, i32, i32, u32),
}

pub struct WindowOptions<'a> {
    pub title: &'a str,
    /// Initial client size in pixels.
    pub width: u32,
    pub height: u32,
}

/// A created, still hidden window. [`Window::run`] shows it and runs the loop.
pub struct Window {
    hwnd: HWND,
    state: *mut State,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

impl Window {
    pub fn create(options: &WindowOptions<'_>) -> Result<Window, PlatformError> {
        let class = wide(CLASS_NAME);
        let title = wide(options.title);
        let state = Box::into_raw(Box::new(State {
            app: None,
            dib: None,
            width: options.width,
            height: options.height,
            dpi: system_dpi(),
            high_surrogate: None,
            last_click: (0, 0, 0, 0),
        }));
        // SAFETY: strings are NUL-terminated and outlive the calls; the state
        // pointer stays valid until `run` frees it after the loop.
        unsafe {
            let instance = GetModuleHandleW(null());
            // Taskbar and Alt+Tab use the class icons; null (no resource) keeps the default.
            let icon = |w, h| {
                let dpi = system_dpi();
                LoadImageW(
                    instance,
                    APP_ICON as usize as *const u16,
                    IMAGE_ICON,
                    GetSystemMetricsForDpi(w, dpi),
                    GetSystemMetricsForDpi(h, dpi),
                    LR_DEFAULTCOLOR,
                )
            };
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: Some(window_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: icon(SM_CXICON, SM_CYICON),
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                hbrBackground: null_mut(),
                lpszMenuName: null(),
                lpszClassName: class.as_ptr(),
                hIconSm: icon(SM_CXSMICON, SM_CYSMICON),
            };
            RegisterClassExW(&wc);
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                options.width as i32,
                options.height as i32,
                null_mut(),
                null_mut(),
                instance,
                null(),
            );
            if hwnd.is_null() {
                drop(Box::from_raw(state));
                return Err(PlatformError("CreateWindowExW failed"));
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
            (*state).dpi = GetDpiForWindow(hwnd);
            // A 1 px frame extension keeps the DWM shadow without a visible border.
            let margins = MARGINS {
                cxLeftWidth: 0,
                cxRightWidth: 0,
                cyTopHeight: 1,
                cyBottomHeight: 0,
            };
            DwmExtendFrameIntoClientArea(hwnd, &margins);
            let round = DWMWCP_ROUND;
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                (&round as *const i32).cast(),
                size_of::<i32>() as u32,
            );
            // Recompute the frame so WM_NCCALCSIZE removes the caption.
            SetWindowPos(
                hwnd,
                null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            GetClientRect(hwnd, &mut rect);
            (*state).width = (rect.right - rect.left).max(0) as u32;
            (*state).height = (rect.bottom - rect.top).max(0) as u32;
            Ok(Window { hwnd, state })
        }
    }

    pub fn dpi(&self) -> u32 {
        // SAFETY: the state is owned by this window until `run` ends.
        unsafe { (*self.state).dpi }
    }

    pub fn size(&self) -> (u32, u32) {
        // SAFETY: as above.
        unsafe { ((*self.state).width, (*self.state).height) }
    }

    pub fn waker(&self) -> Waker {
        Waker(self.hwnd as isize)
    }

    /// Shows the window with its first frame already painted, sends
    /// [`Event::Shown`], and runs the message loop. Returns the exit code.
    pub fn run(self, app: Box<dyn App>) -> i32 {
        let hwnd = self.hwnd;
        // SAFETY: the state pointer is valid until it is freed below, after
        // the window has been destroyed and the loop has ended.
        unsafe {
            (*self.state).app = Some(app);
            // Cloak while the first frame is painted so no blank frame shows.
            let cloak: i32 = 1;
            DwmSetWindowAttribute(hwnd, DWMWA_CLOAK as u32, (&cloak as *const i32).cast(), 4);
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);
            let uncloak: i32 = 0;
            DwmSetWindowAttribute(hwnd, DWMWA_CLOAK as u32, (&uncloak as *const i32).cast(), 4);
            wait_for_composition();
            dispatch(hwnd, Event::Shown);

            let mut msg: MSG = std::mem::zeroed();
            let code = loop {
                let result = GetMessageW(&mut msg, null_mut(), 0, 0);
                if result <= 0 {
                    break if result == 0 { msg.wParam as i32 } else { 1 };
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            };
            drop(Box::from_raw(self.state));
            code
        }
    }
}

unsafe fn state<'a>(hwnd: HWND) -> Option<&'a mut State> {
    // SAFETY: the pointer is either null or the live `State` of this window.
    unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State).as_mut() }
}

/// Calls the application with `event`; returns its `handled` result, or
/// `None` when the application is busy (re-entrant message) or absent.
unsafe fn dispatch(hwnd: HWND, event: Event) -> Option<bool> {
    // SAFETY: called on the window thread with this window's handle.
    let current = unsafe { state(hwnd) }?;
    let mut app = current.app.take()?;
    let mut ctx = Ctx {
        width: current.width,
        height: current.height,
        dpi: current.dpi,
        redraw: false,
        close: false,
        title: None,
        hwnd,
        memory_clipboard: None,
    };
    let handled = app.event(event, &mut ctx);
    // SAFETY: as above; the application is not re-entered while taken.
    unsafe {
        if let Some(state) = state(hwnd) {
            state.app = Some(app);
        }
        apply(hwnd, ctx);
    }
    Some(handled)
}

unsafe fn apply(hwnd: HWND, ctx: Ctx) {
    // SAFETY: window calls on a live window handle; the title is NUL-terminated.
    unsafe {
        if let Some(title) = ctx.title {
            SetWindowTextW(hwnd, wide(&title).as_ptr());
        }
        if ctx.redraw {
            InvalidateRect(hwnd, null(), 0);
        }
        if ctx.close {
            DestroyWindow(hwnd);
        }
    }
}

fn modifier(vk: u16) -> bool {
    // SAFETY: plain Win32 call.
    unsafe { GetKeyState(i32::from(vk)) < 0 }
}

fn point_of(lparam: LPARAM) -> (i32, i32) {
    (
        i32::from((lparam & 0xFFFF) as u16 as i16),
        i32::from(((lparam >> 16) & 0xFFFF) as u16 as i16),
    )
}

/// Click count of a left click at `(x, y)`: 1-3, cycling, when it follows the
/// previous click within the double-click time and rectangle.
fn count_click(state: &mut State, x: i32, y: i32) -> u32 {
    // SAFETY: plain Win32 calls.
    let (time, limit, cx, cy) = unsafe {
        (
            GetMessageTime() as u32,
            GetDoubleClickTime(),
            GetSystemMetrics(SM_CXDOUBLECLK) / 2,
            GetSystemMetrics(SM_CYDOUBLECLK) / 2,
        )
    };
    let (last_time, lx, ly, count) = state.last_click;
    let near = (x - lx).abs() <= cx && (y - ly).abs() <= cy;
    let clicks = if count > 0 && near && time.wrapping_sub(last_time) <= limit {
        count % 3 + 1
    } else {
        1
    };
    state.last_click = (time, x, y, clicks);
    clicks
}

fn resize_border(dpi: u32) -> (i32, i32) {
    // SAFETY: plain Win32 calls.
    unsafe {
        let pad = GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
        (
            GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi) + pad,
            GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) + pad,
        )
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: Win32 message handling on the window thread; pointers in
    // `lparam` follow each message's documented contract.
    unsafe {
        match msg {
            WM_NCCALCSIZE if wparam != 0 => {
                // The whole window is client area; when maximized, keep content on screen.
                if IsZoomed(hwnd) != 0 {
                    let params = &mut *(lparam as *mut NCCALCSIZE_PARAMS);
                    let (bx, by) = resize_border(GetDpiForWindow(hwnd));
                    let r = &mut params.rgrc[0];
                    r.left += bx;
                    r.right -= bx;
                    r.top += by;
                    r.bottom -= by;
                }
                0
            }
            WM_NCACTIVATE => DefWindowProcW(hwnd, msg, wparam, -1),
            WM_NCHITTEST => hit_test(hwnd, lparam),
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                paint(hwnd);
                0
            }
            WM_SIZE => {
                let (width, height) = ((lparam & 0xFFFF) as u32, ((lparam >> 16) & 0xFFFF) as u32);
                if let Some(state) = state(hwnd) {
                    state.width = width;
                    state.height = height;
                }
                dispatch(hwnd, Event::Resized { width, height });
                InvalidateRect(hwnd, null(), 0);
                0
            }
            WM_DPICHANGED => {
                let dpi = (wparam & 0xFFFF) as u32;
                if let Some(state) = state(hwnd) {
                    state.dpi = dpi;
                }
                dispatch(hwnd, Event::DpiChanged(dpi));
                let r = &*(lparam as *const RECT);
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                0
            }
            WM_GETMINMAXINFO => {
                let info = &mut *(lparam as *mut MINMAXINFO);
                let dpi = GetDpiForWindow(hwnd).max(96) as i32;
                info.ptMinTrackSize = POINT {
                    x: 400 * dpi / 96,
                    y: 240 * dpi / 96,
                };
                0
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let key = Key {
                    vk: wparam as u16,
                    ctrl: modifier(VK_CONTROL),
                    shift: modifier(VK_SHIFT),
                    alt: modifier(VK_MENU),
                };
                match dispatch(hwnd, Event::Key(key)) {
                    Some(true) => 0,
                    _ => DefWindowProcW(hwnd, msg, wparam, lparam),
                }
            }
            WM_CHAR => {
                let unit = wparam as u16;
                let ch = match state(hwnd) {
                    Some(state) if (0xD800..0xDC00).contains(&unit) => {
                        state.high_surrogate = Some(unit);
                        None
                    }
                    Some(state) => match state.high_surrogate.take() {
                        Some(high) => char::decode_utf16([high, unit]).next().and_then(Result::ok),
                        None => char::from_u32(u32::from(unit)),
                    },
                    None => None,
                };
                if let Some(ch) = ch {
                    dispatch(hwnd, Event::Char(ch));
                }
                0
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let delta = i32::from((wparam >> 16) as u16 as i16);
                let event = Event::Wheel {
                    delta,
                    horizontal: msg == WM_MOUSEHWHEEL,
                    shift: modifier(VK_SHIFT),
                    ctrl: modifier(VK_CONTROL),
                };
                dispatch(hwnd, event);
                0
            }
            WM_WAKE => {
                dispatch(hwnd, Event::Wake);
                0
            }
            WM_LBUTTONDOWN => {
                let (x, y) = point_of(lparam);
                let Some(state) = state(hwnd) else {
                    return 0;
                };
                let clicks = count_click(state, x, y);
                SetCapture(hwnd);
                let event = Event::MouseDown {
                    x,
                    y,
                    clicks,
                    shift: modifier(VK_SHIFT),
                    ctrl: modifier(VK_CONTROL),
                    alt: modifier(VK_MENU),
                };
                dispatch(hwnd, event);
                0
            }
            WM_MOUSEMOVE => {
                if wparam & MK_LBUTTON != 0 && GetCapture() == hwnd {
                    let (x, y) = point_of(lparam);
                    dispatch(hwnd, Event::MouseMove { x, y });
                }
                0
            }
            WM_LBUTTONUP => {
                // Releasing the capture sends WM_CAPTURECHANGED, which reports MouseUp.
                ReleaseCapture();
                0
            }
            WM_CAPTURECHANGED => {
                dispatch(hwnd, Event::MouseUp);
                0
            }
            WM_SETCURSOR if (lparam & 0xFFFF) as u32 == HTCLIENT => {
                let mut point = POINT { x: 0, y: 0 };
                GetCursorPos(&mut point);
                ScreenToClient(hwnd, &mut point);
                let text = state(hwnd)
                    .and_then(|s| s.app.as_ref())
                    .is_some_and(|app| app.is_text(point.x, point.y));
                SetCursor(LoadCursorW(
                    null_mut(),
                    if text { IDC_IBEAM } else { IDC_ARROW },
                ));
                1
            }
            WM_CLOSE => {
                if dispatch(hwnd, Event::CloseRequested) != Some(true) {
                    DestroyWindow(hwnd);
                }
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

unsafe fn hit_test(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let (x, y) = point_of(lparam);
    let mut point = POINT { x, y };
    // SAFETY: valid window and stack pointers.
    unsafe {
        ScreenToClient(hwnd, &mut point);
        let Some(state) = state(hwnd) else {
            return HTCLIENT as LRESULT;
        };
        let (w, h) = (state.width as i32, state.height as i32);
        if IsZoomed(hwnd) == 0 {
            let (bx, by) = resize_border(state.dpi);
            let left = point.x < bx;
            let right = point.x >= w - bx;
            let top = point.y < by;
            let bottom = point.y >= h - by;
            let hit = match (left, right, top, bottom) {
                (true, _, true, _) => Some(HTTOPLEFT),
                (_, true, true, _) => Some(HTTOPRIGHT),
                (true, _, _, true) => Some(HTBOTTOMLEFT),
                (_, true, _, true) => Some(HTBOTTOMRIGHT),
                (true, ..) => Some(HTLEFT),
                (_, true, ..) => Some(HTRIGHT),
                (_, _, true, _) => Some(HTTOP),
                (.., true) => Some(HTBOTTOM),
                _ => None,
            };
            if let Some(hit) = hit {
                return hit as LRESULT;
            }
        }
        let caption = state
            .app
            .as_ref()
            .is_some_and(|app| app.is_caption(point.x, point.y));
        (if caption { HTCAPTION } else { HTCLIENT }) as LRESULT
    }
}

unsafe fn paint(hwnd: HWND) {
    // SAFETY: BeginPaint/EndPaint pair on the window thread; the DIB and the
    // application are only used inside this call.
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);
        if let Some(state) = state(hwnd)
            && state.width > 0
            && state.height > 0
        {
            let (w, h) = (state.width, state.height);
            if state
                .dib
                .as_ref()
                .is_none_or(|d| d.width != w || d.height != h)
            {
                state.dib = None;
                state.dib = Dib::new(w, h);
            }
            if let (Some(dib), Some(mut app)) = (state.dib.as_mut(), state.app.take()) {
                app.paint(dib.pixels(), w, h);
                BitBlt(hdc, 0, 0, w as i32, h as i32, dib.dc, 0, 0, SRCCOPY);
                state.app = Some(app);
            }
        }
        EndPaint(hwnd, &ps);
    }
}
