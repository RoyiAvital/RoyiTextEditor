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
    DWM_TIMING_INFO, DWMWA_CLOAK, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMWCP_ROUND,
    DwmExtendFrameIntoClientArea, DwmFlush, DwmGetCompositionTimingInfo, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, EndPaint, GetMonitorInfoW, HBITMAP, HDC, HGDIOBJ,
    InvalidateRect, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, PAINTSTRUCT, SRCCOPY,
    ScreenToClient, SelectObject, UpdateWindow,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
use windows_sys::Win32::UI::Controls::MARGINS;
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetCapture, GetDoubleClickTime, GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE,
    TRACKMOUSEEVENT, TrackMouseEvent, VK_CONTROL, VK_MENU, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWL_STYLE,
    GWLP_USERDATA, GetClientRect, GetCursorPos, GetMessageTime, GetMessageW, GetSystemMetrics,
    GetWindowLongPtrW, GetWindowPlacement, GetWindowRect, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT,
    HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, IDC_ARROW, IDC_IBEAM,
    IMAGE_ICON, IsIconic, IsZoomed, LR_DEFAULTCOLOR, LoadCursorW, LoadImageW, MINMAXINFO, MSG,
    NCCALCSIZE_PARAMS, PostMessageW, PostQuitMessage, RegisterClassExW, SM_CXDOUBLECLK, SM_CXICON,
    SM_CXPADDEDBORDER, SM_CXSIZEFRAME, SM_CXSMICON, SM_CYDOUBLECLK, SM_CYICON, SM_CYSIZEFRAME,
    SM_CYSMICON, SW_SHOW, SW_SHOWNORMAL, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SetCursor, SetForegroundWindow, SetWindowLongPtrW,
    SetWindowPlacement, SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage,
    WINDOWPLACEMENT, WM_APP, WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE, WM_DESTROY, WM_DPICHANGED,
    WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN,
    WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCACTIVATE, WM_NCCALCSIZE,
    WM_NCHITTEST, WM_PAINT, WM_SETCURSOR, WM_SIZE, WM_SYSCOMMAND, WM_SYSKEYDOWN,
    WM_WINDOWPOSCHANGED, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer, WM_TIMER};

const WM_WAKE: u32 = WM_APP + 1;
/// A scripted key (`Ctx::post_key`): `wParam` holds the key, see `encode_key`.
const WM_SCRIPTED_KEY: u32 = WM_APP + 2;
/// A scripted character (`Ctx::post_char`): `wParam` holds the code point.
const WM_SCRIPTED_CHAR: u32 = WM_APP + 3;
const CLASS_NAME: &str = "RoyiTextEditorWindow";
/// Icon group embedded in the executable by `rte_app`'s build script.
const APP_ICON: u16 = 1;
/// `wParam` flag of mouse messages: left button down.
const MK_LBUTTON: usize = 0x0001;
/// `wParam` flag of mouse messages: middle button down.
const MK_MBUTTON: usize = 0x0010;
/// Files dropped from Explorer; `wParam` is the `HDROP`.
const WM_DROPFILES: u32 = 0x0233;
/// Extended window style accepting `WM_DROPFILES` (what `DragAcceptFiles` sets).
const WS_EX_ACCEPTFILES: u32 = 0x0000_0010;
/// The one-shot timer of [`Ctx::set_timer`].
const APP_TIMER: usize = 1;
/// The pointer left the client area (after `TrackMouseEvent`).
const WM_MOUSELEAVE: u32 = 0x02A3;
/// The display resolution or arrangement changed.
const WM_DISPLAYCHANGE: u32 = 0x007E;
/// `WM_SYSCOMMAND` codes that move or size the window.
const SC_SIZE: usize = 0xF000;
const SC_MOVE: usize = 0xF010;
const SC_MAXIMIZE: usize = 0xF030;
const SC_RESTORE: usize = 0xF120;

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

/// Milliseconds between display refreshes as composed by DWM (60 Hz when
/// unknown).
pub fn refresh_period_ms() -> f64 {
    let mut info: DWM_TIMING_INFO = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<DWM_TIMING_INFO>() as u32;
    // SAFETY: `info` is a live, correctly sized buffer; a null window means the desktop.
    let ok = unsafe { DwmGetCompositionTimingInfo(null_mut(), &mut info) } >= 0;
    let rate = info.rateRefresh;
    if ok && rate.uiNumerator > 0 && rate.uiDenominator > 0 {
        1000.0 * f64::from(rate.uiDenominator) / f64::from(rate.uiNumerator)
    } else {
        1000.0 / 60.0
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

#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// Left (or, with `middle`, middle) button pressed in the client area;
    /// `clicks` counts 1-3 for single, double and triple clicks (always 1
    /// for the middle button). The mouse is captured until [`Event::MouseUp`].
    MouseDown {
        x: i32,
        y: i32,
        clicks: u32,
        shift: bool,
        ctrl: bool,
        alt: bool,
        middle: bool,
    },
    /// Pointer movement while the pressed button is held (also outside the window).
    MouseMove {
        x: i32,
        y: i32,
    },
    /// Pointer movement over the client area without a pressed button.
    Hover {
        x: i32,
        y: i32,
    },
    /// The pointer left the client area.
    MouseLeave,
    /// The time of [`Ctx::set_timer`] passed.
    Timer,
    /// The left button was released or the capture was lost.
    MouseUp,
    CloseRequested,
    /// Files dropped on the window (the drop is already released), or why
    /// the dropped file names could not be read.
    FilesDropped(Result<Dropped, String>),
    /// Posted by a [`Waker`] from another thread.
    Wake,
    /// Full screen turned on or off (also by the system, e.g. when the
    /// window is moved), or why a request failed (the state is unchanged).
    FullScreen(Result<bool, &'static str>),
}

/// The files of a drop: the first one and how many more came with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dropped {
    pub path: std::path::PathBuf,
    pub others: u32,
}

/// Window state visible to the application, plus deferred requests.
pub struct Ctx {
    width: u32,
    height: u32,
    dpi: u32,
    redraw: bool,
    close: bool,
    title: Option<String>,
    open_url: Option<String>,
    posted_key: Option<Key>,
    posted_char: Option<char>,
    full_screen: Option<bool>,
    timer: Option<u32>,
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
            open_url: None,
            posted_key: None,
            posted_char: None,
            full_screen: None,
            timer: None,
            hwnd: null_mut(),
            memory_clipboard: None,
        }
    }

    /// Key posted during this call, if any.
    pub fn posted_key(&self) -> Option<Key> {
        self.posted_key
    }

    /// Character posted during this call, if any.
    pub fn posted_char(&self) -> Option<char> {
        self.posted_char
    }

    pub fn close_requested(&self) -> bool {
        self.close
    }

    /// URL requested during this call, if any.
    pub fn requested_url(&self) -> Option<&str> {
        self.open_url.as_deref()
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

    /// Opens an `https` URL in the default browser after the call returns.
    pub fn open_url(&mut self, url: &str) {
        if url.starts_with("https://") {
            self.open_url = Some(url.to_owned());
        }
    }

    /// Queues `key` as if typed, with exactly its modifiers (benchmarks).
    pub fn post_key(&mut self, key: Key) {
        self.posted_key = Some(key);
    }

    /// Queues `ch` as if typed (benchmarks).
    pub fn post_char(&mut self, ch: char) {
        self.posted_char = Some(ch);
    }

    /// Turns full screen on or off after the call returns; the result
    /// arrives as [`Event::FullScreen`].
    pub fn set_full_screen(&mut self, on: bool) {
        self.full_screen = Some(on);
    }

    /// Full screen state requested during this call, if any.
    pub fn requested_full_screen(&self) -> Option<bool> {
        self.full_screen
    }

    /// Sends [`Event::Timer`] once, after `ms` milliseconds (replacing a
    /// pending timer).
    pub fn set_timer(&mut self, ms: u32) {
        self.timer = Some(ms);
    }

    /// Timer requested during this call, if any.
    pub fn requested_timer(&self) -> Option<u32> {
        self.timer
    }
}

fn encode_key(key: Key) -> WPARAM {
    usize::from(key.vk)
        | usize::from(key.ctrl) << 16
        | usize::from(key.shift) << 17
        | usize::from(key.alt) << 18
}

fn decode_key(wparam: WPARAM) -> Key {
    Key {
        vk: (wparam & 0xFFFF) as u16,
        ctrl: wparam & 1 << 16 != 0,
        shift: wparam & 1 << 17 != 0,
        alt: wparam & 1 << 18 != 0,
    }
}

pub trait App {
    /// Handles an event; returns `false` to get the default system behavior
    /// (e.g. `Alt+F4` for an unhandled system key, closing for `CloseRequested`).
    fn event(&mut self, event: Event, ctx: &mut Ctx) -> bool;

    /// Draws the whole client area into `pixels` (`0xAARRGGBB`, top-down rows).
    fn paint(&mut self, pixels: &mut [u32], width: u32, height: u32);

    /// Called after a painted frame was copied to the window (`BitBlt`).
    fn presented(&mut self, _ctx: &mut Ctx) {}

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
    /// Placement and style before full screen, while it is on.
    full_screen: Option<Saved>,
    /// The platform itself is moving the window (full screen changes).
    placing: bool,
    /// `WM_MOUSELEAVE` was requested for the pointer inside the window.
    tracking_leave: bool,
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
            full_screen: None,
            placing: false,
            tracking_leave: false,
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
                WS_EX_ACCEPTFILES,
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
    let mut ctx = window_ctx(hwnd, current);
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

fn window_ctx(hwnd: HWND, state: &State) -> Ctx {
    Ctx {
        width: state.width,
        height: state.height,
        dpi: state.dpi,
        hwnd,
        ..Ctx::headless(0, 0, 0)
    }
}

unsafe fn apply(hwnd: HWND, ctx: Ctx) {
    // SAFETY: window calls on a live window handle; the title is NUL-terminated.
    unsafe {
        if let Some(title) = ctx.title {
            SetWindowTextW(hwnd, wide(&title).as_ptr());
        }
        if let Some(url) = ctx.open_url {
            shell_open(hwnd, &url);
        }
        if let Some(key) = ctx.posted_key {
            PostMessageW(hwnd, WM_SCRIPTED_KEY, encode_key(key), 0);
        }
        if let Some(ch) = ctx.posted_char {
            PostMessageW(hwnd, WM_SCRIPTED_CHAR, ch as usize, 0);
        }
        if let Some(ms) = ctx.timer {
            SetTimer(hwnd, APP_TIMER, ms, None);
        }
        if let Some(on) = ctx.full_screen {
            if on {
                enter_full_screen(&mut Native(hwnd));
            } else {
                leave_full_screen(&mut Native(hwnd), true);
            }
        }
        if ctx.redraw {
            InvalidateRect(hwnd, null(), 0);
        }
        if ctx.close {
            DestroyWindow(hwnd);
        }
    }
}

fn is_full_screen(hwnd: HWND) -> bool {
    // SAFETY: called on the window thread with this window's handle.
    unsafe { state(hwnd) }.is_some_and(|s| s.full_screen.is_some())
}

/// Marks window moves made by the platform itself (full screen changes).
fn set_placing(hwnd: HWND, placing: bool) {
    // SAFETY: as above.
    if let Some(state) = unsafe { state(hwnd) } {
        state.placing = placing;
    }
}

/// Rectangle of the monitor nearest to the window.
fn monitor_rect(hwnd: HWND) -> Option<RECT> {
    // SAFETY: `info` is a live, correctly sized buffer.
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        (GetMonitorInfoW(monitor, &mut info) != 0).then_some(info.rcMonitor)
    }
}

fn set_corners(hwnd: HWND, preference: i32) {
    // SAFETY: the attribute value is a live i32.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&preference as *const i32).cast(),
            size_of::<i32>() as u32,
        );
    }
}

/// Saved placement and style of a full-screen window.
type Saved = (WINDOWPLACEMENT, isize);

const NO_MONITOR: &str = "Windows did not report the window's monitor";
const NO_FRAME: &str = "Windows did not change the window frame";
const NOT_COVERED: &str = "Windows did not size the window to its monitor";
const NOT_RESTORED: &str = "Windows did not restore the window's place";

/// The window calls of full screen; tests replace them to fail each one.
trait Screen {
    /// Placement and style to restore later, when the monitor is known.
    fn save(&mut self) -> Option<Saved>;
    fn saved(&self) -> Option<Saved>;
    fn set_saved(&mut self, saved: Option<Saved>);
    /// Sets the window style; false when its frame bits did not take.
    fn set_style(&mut self, style: isize) -> bool;
    fn set_corners(&mut self, round: bool);
    /// Sizes the window to its current monitor.
    fn cover(&mut self) -> bool;
    /// Puts back `placement` (when given) and recomputes the frame.
    fn restore(&mut self, placement: Option<&WINDOWPLACEMENT>) -> bool;
    fn notify(&mut self, result: Result<bool, &'static str>);
}

/// Enters full screen. The application learns the state before the window
/// is resized; a failed step undoes the earlier ones and says why.
fn enter_full_screen(s: &mut impl Screen) {
    if s.saved().is_some() {
        s.notify(Ok(true));
        return;
    }
    let Some((placement, style)) = s.save() else {
        s.notify(Err(NO_MONITOR));
        return;
    };
    // Set first: the frame calculation then skips the maximized insets.
    s.set_saved(Some((placement, style)));
    if !s.set_style(style & !(WS_OVERLAPPEDWINDOW as isize)) {
        s.set_saved(None);
        s.set_style(style);
        s.notify(Err(NO_FRAME));
        return;
    }
    s.notify(Ok(true));
    s.set_corners(false);
    if !s.cover() {
        leave_full_screen(s, true);
        s.notify(Err(NOT_COVERED));
    }
}

/// Leaves full screen; `restore` puts back the saved placement (a move by
/// the system keeps its new place). Full screen stays when the frame cannot
/// be restored.
fn leave_full_screen(s: &mut impl Screen, restore: bool) {
    let Some((placement, style)) = s.saved() else {
        s.notify(Ok(false));
        return;
    };
    if !s.set_style(style) {
        s.notify(Err(NO_FRAME));
        return;
    }
    s.set_saved(None);
    s.notify(Ok(false));
    s.set_corners(true);
    if !s.restore(restore.then_some(&placement)) {
        s.notify(Err(NOT_RESTORED));
    }
}

/// Covers the window's monitor again (DPI or display change); leaves full
/// screen when that fails.
fn recover_full_screen(s: &mut impl Screen) {
    if !s.cover() {
        leave_full_screen(s, true);
        s.notify(Err(NOT_COVERED));
    }
}

/// [`Screen`] of a real window.
struct Native(HWND);

impl Screen for Native {
    fn save(&mut self) -> Option<Saved> {
        // SAFETY: window calls on a live handle with a live, sized buffer.
        unsafe {
            let mut placement: WINDOWPLACEMENT = std::mem::zeroed();
            placement.length = size_of::<WINDOWPLACEMENT>() as u32;
            let placed = GetWindowPlacement(self.0, &mut placement) != 0;
            (placed && monitor_rect(self.0).is_some())
                .then(|| (placement, GetWindowLongPtrW(self.0, GWL_STYLE)))
        }
    }

    fn saved(&self) -> Option<Saved> {
        // SAFETY: called on the window thread with this window's handle.
        unsafe { state(self.0) }.and_then(|s| s.full_screen)
    }

    fn set_saved(&mut self, saved: Option<Saved>) {
        // SAFETY: as above.
        if let Some(state) = unsafe { state(self.0) } {
            state.full_screen = saved;
        }
    }

    fn set_style(&mut self, style: isize) -> bool {
        let frame = WS_OVERLAPPEDWINDOW as isize;
        // SAFETY: window calls on a live handle.
        unsafe {
            SetWindowLongPtrW(self.0, GWL_STYLE, style);
            GetWindowLongPtrW(self.0, GWL_STYLE) & frame == style & frame
        }
    }

    fn set_corners(&mut self, round: bool) {
        set_corners(
            self.0,
            if round {
                DWMWCP_ROUND
            } else {
                DWMWCP_DONOTROUND
            },
        );
    }

    fn cover(&mut self) -> bool {
        let Some(r) = monitor_rect(self.0) else {
            return false;
        };
        set_placing(self.0, true);
        // SAFETY: plain window call on a live handle.
        let placed = unsafe {
            SetWindowPos(
                self.0,
                null_mut(),
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
            )
        } != 0;
        set_placing(self.0, false);
        placed
    }

    fn restore(&mut self, placement: Option<&WINDOWPLACEMENT>) -> bool {
        set_placing(self.0, true);
        // SAFETY: window calls on a live handle; the placement is a live value.
        let restored = unsafe {
            let placed = placement.is_none_or(|p| SetWindowPlacement(self.0, p) != 0);
            let framed = SetWindowPos(
                self.0,
                null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED
                    | SWP_NOMOVE
                    | SWP_NOSIZE
                    | SWP_NOZORDER
                    | SWP_NOOWNERZORDER
                    | SWP_NOACTIVATE,
            ) != 0;
            placed && framed
        };
        set_placing(self.0, false);
        restored
    }

    fn notify(&mut self, result: Result<bool, &'static str>) {
        // SAFETY: called on the window thread; the application is not running.
        unsafe { dispatch(self.0, Event::FullScreen(result)) };
    }
}
fn modifier(vk: u16) -> bool {
    // SAFETY: plain Win32 call.
    unsafe { GetKeyState(i32::from(vk)) < 0 }
}

/// Address of `name` in `library`, which is loaded on first use and stays loaded.
fn library_proc(
    library: &str,
    name: &std::ffi::CStr,
) -> Option<unsafe extern "system" fn() -> isize> {
    // SAFETY: both names are NUL-terminated and outlive the calls.
    unsafe {
        let module = LoadLibraryW(wide(library).as_ptr());
        if module.is_null() {
            return None;
        }
        GetProcAddress(module, name.as_ptr().cast())
    }
}

/// Opens `url` with its default handler. shell32 and ole32 are not imported
/// at startup; they load on the first call.
fn shell_open(hwnd: HWND, url: &str) {
    type CoInitializeEx = unsafe extern "system" fn(*const c_void, u32) -> i32;
    type ShellExecuteW = unsafe extern "system" fn(
        HWND,
        *const u16,
        *const u16,
        *const u16,
        *const u16,
        i32,
    ) -> *mut c_void;
    const COINIT_APARTMENTTHREADED: u32 = 0x2;
    const COINIT_DISABLE_OLE1DDE: u32 = 0x4;
    // SAFETY: the pointers come from GetProcAddress for functions with these
    // documented signatures; the strings are NUL-terminated and outlive the call.
    unsafe {
        // ShellExecute may delegate to COM-based handlers.
        if let Some(init) = library_proc("ole32.dll", c"CoInitializeEx") {
            let init: CoInitializeEx = std::mem::transmute(init);
            init(null(), COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        }
        if let Some(execute) = library_proc("shell32.dll", c"ShellExecuteW") {
            let execute: ShellExecuteW = std::mem::transmute(execute);
            let (verb, url) = (wide("open"), wide(url));
            execute(
                hwnd,
                verb.as_ptr(),
                url.as_ptr(),
                null(),
                null(),
                SW_SHOWNORMAL,
            );
        }
    }
}

/// `DragQueryFileW` and `DragFinish`.
#[derive(Clone, Copy)]
struct DropApi {
    query: unsafe extern "system" fn(*mut c_void, u32, *mut u16, u32) -> u32,
    finish: unsafe extern "system" fn(*mut c_void),
}

/// The drop functions of shell32, which is loaded on the first drop (not at
/// startup); `None` when they cannot be found.
fn drop_api() -> Option<DropApi> {
    static API: std::sync::OnceLock<Option<DropApi>> = std::sync::OnceLock::new();
    *API.get_or_init(|| {
        let query = library_proc("shell32.dll", c"DragQueryFileW")?;
        let finish = library_proc("shell32.dll", c"DragFinish")?;
        // SAFETY: both come from GetProcAddress for functions with these
        // documented signatures.
        unsafe {
            Some(DropApi {
                query: std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    unsafe extern "system" fn(*mut c_void, u32, *mut u16, u32) -> u32,
                >(query),
                finish: std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    unsafe extern "system" fn(*mut c_void),
                >(finish),
            })
        }
    })
}

/// Reads the first file of the drop `hdrop` and releases the drop (exactly
/// once when the API is available; without it the drop cannot be released).
fn take_drop(hdrop: *mut c_void, api: Option<DropApi>) -> Result<Dropped, String> {
    let Some(api) = api else {
        return Err("Drag and drop is not available: shell32 could not be loaded".to_owned());
    };
    // SAFETY: `hdrop` is the drop handle of WM_DROPFILES, valid until
    // `finish`; the buffer holds `len + 1` units as passed.
    let first = unsafe {
        let count = (api.query)(hdrop, u32::MAX, null_mut(), 0);
        let first = if count == 0 {
            None
        } else {
            let len = (api.query)(hdrop, 0, null_mut(), 0);
            let mut buf = vec![0u16; len as usize + 1];
            let got = (api.query)(hdrop, 0, buf.as_mut_ptr(), len + 1);
            (got > 0).then(|| {
                buf.truncate(got as usize);
                (buf, count)
            })
        };
        (api.finish)(hdrop);
        first
    };
    let (name, count) =
        first.ok_or_else(|| "The dropped file name could not be read".to_owned())?;
    use std::os::windows::ffi::OsStringExt;
    Ok(Dropped {
        path: std::ffi::OsString::from_wide(&name).into(),
        others: count - 1,
    })
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
                // The whole window is client area; when maximized, keep content on
                // screen (full screen covers the whole monitor instead).
                if IsZoomed(hwnd) != 0 && !is_full_screen(hwnd) {
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
                if is_full_screen(hwnd) {
                    recover_full_screen(&mut Native(hwnd));
                } else {
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
                }
                0
            }
            WM_DISPLAYCHANGE => {
                if is_full_screen(hwnd) {
                    recover_full_screen(&mut Native(hwnd));
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_SYSCOMMAND => {
                // Moving, sizing or maximizing by the system ends full screen first.
                if is_full_screen(hwnd)
                    && matches!(
                        wparam & 0xFFF0,
                        SC_SIZE | SC_MOVE | SC_MAXIMIZE | SC_RESTORE
                    )
                {
                    leave_full_screen(&mut Native(hwnd), true);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_WINDOWPOSCHANGED => {
                // Snapping or other system moves leave the monitor: no longer full screen.
                let own = state(hwnd).is_none_or(|s| s.placing);
                if !own && is_full_screen(hwnd) && IsIconic(hwnd) == 0 {
                    let mut r = RECT {
                        left: 0,
                        top: 0,
                        right: 0,
                        bottom: 0,
                    };
                    GetWindowRect(hwnd, &mut r);
                    let m = monitor_rect(hwnd);
                    let covers = m.is_some_and(|m| {
                        (m.left, m.top, m.right, m.bottom) == (r.left, r.top, r.right, r.bottom)
                    });
                    if !covers {
                        leave_full_screen(&mut Native(hwnd), false);
                    }
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
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
            WM_SCRIPTED_KEY => {
                dispatch(hwnd, Event::Key(decode_key(wparam)));
                0
            }
            WM_SCRIPTED_CHAR => {
                if let Some(ch) = char::from_u32(wparam as u32) {
                    dispatch(hwnd, Event::Char(ch));
                }
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
                    middle: false,
                };
                dispatch(hwnd, event);
                0
            }
            WM_MBUTTONDOWN => {
                let (x, y) = point_of(lparam);
                SetCapture(hwnd);
                let event = Event::MouseDown {
                    x,
                    y,
                    clicks: 1,
                    shift: modifier(VK_SHIFT),
                    ctrl: modifier(VK_CONTROL),
                    alt: modifier(VK_MENU),
                    middle: true,
                };
                dispatch(hwnd, event);
                0
            }
            WM_MOUSEMOVE => {
                let (x, y) = point_of(lparam);
                if wparam & (MK_LBUTTON | MK_MBUTTON) != 0 && GetCapture() == hwnd {
                    dispatch(hwnd, Event::MouseMove { x, y });
                } else if GetCapture().is_null() {
                    if let Some(state) = state(hwnd)
                        && !state.tracking_leave
                    {
                        let mut track = TRACKMOUSEEVENT {
                            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        state.tracking_leave = TrackMouseEvent(&mut track) != 0;
                    }
                    dispatch(hwnd, Event::Hover { x, y });
                }
                0
            }
            WM_MOUSELEAVE => {
                if let Some(state) = state(hwnd) {
                    state.tracking_leave = false;
                }
                dispatch(hwnd, Event::MouseLeave);
                0
            }
            WM_TIMER if wparam == APP_TIMER => {
                KillTimer(hwnd, APP_TIMER);
                dispatch(hwnd, Event::Timer);
                0
            }
            WM_LBUTTONUP | WM_MBUTTONUP => {
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
            WM_DROPFILES => {
                // Released here, before the application sees the paths.
                let dropped = take_drop(wparam as *mut c_void, drop_api());
                SetForegroundWindow(hwnd);
                dispatch(hwnd, Event::FilesDropped(dropped));
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
        if IsZoomed(hwnd) == 0 && state.full_screen.is_none() {
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
        let mut requests = None;
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
                let mut ctx = window_ctx(hwnd, state);
                app.presented(&mut ctx);
                state.app = Some(app);
                requests = Some(ctx);
            }
        }
        EndPaint(hwnd, &ps);
        if let Some(ctx) = requests {
            apply(hwnd, ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_keys_keep_their_modifiers() {
        for (ctrl, shift, alt) in [
            (false, false, false),
            (true, true, false),
            (false, true, true),
        ] {
            let key = Key {
                vk: 0x50,
                ctrl,
                shift,
                alt,
            };
            assert_eq!(decode_key(encode_key(key)), key);
        }
        let mut ctx = Ctx::headless(10, 10, 96);
        ctx.post_key(Key {
            vk: 0x1B,
            ctrl: false,
            shift: false,
            alt: false,
        });
        assert_eq!(ctx.posted_key().map(|k| k.vk), Some(0x1B));
        ctx.post_char('é');
        assert_eq!(ctx.posted_char(), Some('é'));
    }

    /// A fake drop of two files, the first named `PATH`; counts releases.
    mod fake_drop {
        use super::*;
        use std::sync::atomic::{AtomicUsize, Ordering};

        pub static FINISHED: AtomicUsize = AtomicUsize::new(0);
        const NAME: &str = r"C:\docs\a.txt";

        pub unsafe extern "system" fn query(
            _: *mut c_void,
            i: u32,
            buf: *mut u16,
            cch: u32,
        ) -> u32 {
            let name: Vec<u16> = NAME.encode_utf16().collect();
            match (i, buf.is_null()) {
                (u32::MAX, _) => 2,
                (_, true) => name.len() as u32,
                _ => {
                    let n = name.len().min(cch as usize - 1);
                    // SAFETY: the caller passes a buffer of `cch` units.
                    unsafe {
                        std::ptr::copy_nonoverlapping(name.as_ptr(), buf, n);
                        *buf.add(n) = 0;
                    }
                    n as u32
                }
            }
        }

        pub unsafe extern "system" fn unreadable(
            _: *mut c_void,
            i: u32,
            _: *mut u16,
            _: u32,
        ) -> u32 {
            u32::from(i == u32::MAX)
        }

        pub unsafe extern "system" fn finish(_: *mut c_void) {
            FINISHED.fetch_add(1, Ordering::SeqCst);
        }

        pub fn finished() -> usize {
            FINISHED.load(Ordering::SeqCst)
        }

        pub const PATH: &str = NAME;
    }

    /// A window whose full-screen calls fail on request.
    #[derive(Default)]
    struct FakeScreen {
        style: isize,
        saved: Option<Saved>,
        round: bool,
        covered: bool,
        events: Vec<Result<bool, &'static str>>,
        fail_save: bool,
        /// Fails setting a style without (true) or with (false) the frame.
        fail_style: Option<bool>,
        fail_cover: bool,
        fail_restore: bool,
    }

    const FRAMED: isize = WS_OVERLAPPEDWINDOW as isize | 0x1000_0000;

    impl FakeScreen {
        fn new() -> FakeScreen {
            FakeScreen {
                style: FRAMED,
                round: true,
                ..FakeScreen::default()
            }
        }
    }

    impl Screen for FakeScreen {
        fn save(&mut self) -> Option<Saved> {
            // SAFETY: plain data; zero is a valid placement value.
            (!self.fail_save).then(|| (unsafe { std::mem::zeroed() }, self.style))
        }
        fn saved(&self) -> Option<Saved> {
            self.saved
        }
        fn set_saved(&mut self, saved: Option<Saved>) {
            self.saved = saved;
        }
        fn set_style(&mut self, style: isize) -> bool {
            let framed = style & WS_OVERLAPPEDWINDOW as isize != 0;
            if self.fail_style == Some(!framed) {
                return false;
            }
            self.style = style;
            true
        }
        fn set_corners(&mut self, round: bool) {
            self.round = round;
        }
        fn cover(&mut self) -> bool {
            self.covered = !self.fail_cover;
            self.covered
        }
        fn restore(&mut self, _: Option<&WINDOWPLACEMENT>) -> bool {
            self.covered = false;
            !self.fail_restore
        }
        fn notify(&mut self, result: Result<bool, &'static str>) {
            self.events.push(result);
        }
    }

    #[test]
    fn full_screen_reports_only_what_took_effect() {
        let mut s = FakeScreen::new();
        enter_full_screen(&mut s);
        assert_eq!(s.events, [Ok(true)]);
        assert!(s.saved.is_some() && s.covered && !s.round);
        assert_eq!(s.style, FRAMED & !(WS_OVERLAPPEDWINDOW as isize));
        leave_full_screen(&mut s, true);
        assert_eq!(s.events, [Ok(true), Ok(false)]);
        assert!(s.saved.is_none() && !s.covered && s.round);
        assert_eq!(s.style, FRAMED);

        // Entry failures leave the window as it was.
        let mut s = FakeScreen {
            fail_save: true,
            ..FakeScreen::new()
        };
        enter_full_screen(&mut s);
        assert_eq!(
            (s.events.as_slice(), s.saved.is_none()),
            (&[Err(NO_MONITOR)][..], true)
        );
        let mut s = FakeScreen {
            fail_style: Some(true),
            ..FakeScreen::new()
        };
        enter_full_screen(&mut s);
        assert_eq!(s.events, [Err(NO_FRAME)]);
        assert!(s.saved.is_none() && s.round && s.style == FRAMED);
        let mut s = FakeScreen {
            fail_cover: true,
            ..FakeScreen::new()
        };
        enter_full_screen(&mut s);
        assert_eq!(s.events, [Ok(true), Ok(false), Err(NOT_COVERED)]);
        assert!(s.saved.is_none() && s.round && s.style == FRAMED);

        // Exit: a frame that cannot be restored keeps full screen; a place
        // that cannot be restored is reported after the state changed.
        let mut s = FakeScreen {
            fail_style: Some(false),
            ..FakeScreen::new()
        };
        enter_full_screen(&mut s);
        leave_full_screen(&mut s, true);
        assert_eq!(s.events, [Ok(true), Err(NO_FRAME)]);
        assert!(s.saved.is_some() && !s.round);
        let mut s = FakeScreen {
            fail_restore: true,
            ..FakeScreen::new()
        };
        enter_full_screen(&mut s);
        leave_full_screen(&mut s, true);
        assert_eq!(s.events, [Ok(true), Ok(false), Err(NOT_RESTORED)]);
        assert!(s.saved.is_none() && s.style == FRAMED);

        // A monitor change that cannot be covered ends full screen.
        let mut s = FakeScreen::new();
        enter_full_screen(&mut s);
        s.fail_cover = true;
        recover_full_screen(&mut s);
        assert_eq!(s.events, [Ok(true), Ok(false), Err(NOT_COVERED)]);
        assert!(s.saved.is_none());
    }

    #[test]
    fn a_headless_context_records_full_screen_requests() {
        let mut ctx = Ctx::headless(800, 600, 96);
        assert_eq!(ctx.requested_timer(), None);
        ctx.set_timer(500);
        assert_eq!(ctx.requested_timer(), Some(500));
        assert_eq!(ctx.requested_full_screen(), None);
        ctx.set_full_screen(true);
        assert_eq!(ctx.requested_full_screen(), Some(true));
        ctx.set_full_screen(false);
        assert_eq!(ctx.requested_full_screen(), Some(false));
    }

    #[test]
    fn drops_name_their_first_file_and_are_released_once() {
        let before = fake_drop::finished();
        let api = DropApi {
            query: fake_drop::query,
            finish: fake_drop::finish,
        };
        let dropped = take_drop(null_mut(), Some(api)).unwrap();
        assert_eq!(dropped.path, std::path::PathBuf::from(fake_drop::PATH));
        assert_eq!(dropped.others, 1);
        assert_eq!(fake_drop::finished(), before + 1);

        let before = fake_drop::finished();
        let unreadable = DropApi {
            query: fake_drop::unreadable,
            finish: fake_drop::finish,
        };
        assert!(take_drop(null_mut(), Some(unreadable)).is_err());
        assert_eq!(
            fake_drop::finished(),
            before + 1,
            "released after a failure"
        );

        assert!(take_drop(null_mut(), None).unwrap_err().contains("shell32"));
    }
}
