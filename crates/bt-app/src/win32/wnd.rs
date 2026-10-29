//! Top-level windows and the message loop. Window messages become safe
//! `Event`s delivered to one handler on the UI thread. Events raised while
//! the handler is running (Windows sends some messages synchronously from
//! inside calls like `ShowWindow`) are queued and delivered right after.
use std::{
    cell::RefCell,
    collections::VecDeque,
    ffi::c_void,
    sync::atomic::{AtomicU32, Ordering},
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::{
            Dwm::{
                DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
                DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmExtendFrameIntoClientArea,
                DwmSetWindowAttribute,
            },
            Gdi::{
                BeginPaint, ClientToScreen, EndPaint, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST,
                MONITORINFO, MonitorFromPoint, PAINTSTRUCT,
            },
        },
        System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
        UI::{
            Controls::MARGINS,
            HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI},
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, GetKeyState, HOT_KEY_MODIFIERS, RegisterHotKey, ReleaseCapture,
                SetCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
                UnregisterHotKey, VK_CONTROL, VK_F4, VK_LBUTTON, VK_LWIN, VK_MBUTTON, VK_MENU,
                VK_RBUTTON, VK_RWIN, VK_SHIFT,
            },
            WindowsAndMessaging::{
                CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, GWLP_USERDATA, GetClientRect, GetCursorPos, GetForegroundWindow,
                GetMessageW, GetSystemMetrics, GetWindowLongPtrW, GetWindowPlacement, HICON,
                HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP,
                HTTOPLEFT, HTTOPRIGHT, HWND_TOPMOST, ICON_BIG, ICON_SMALL, IDC_ARROW, IMAGE_ICON,
                IsIconic, IsWindowVisible, KillTimer, LR_DEFAULTCOLOR, LR_SHARED, LoadCursorW,
                LoadImageW, MA_NOACTIVATE, MINMAXINFO, MSG, PBT_APMPOWERSTATUSCHANGE,
                PBT_POWERSETTINGCHANGE, PostMessageW, PostQuitMessage, RegisterClassExW,
                RegisterWindowMessageW, SM_CXICON, SM_CXSMICON, SW_HIDE, SW_RESTORE, SW_SHOW,
                SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
                SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetTimer, SetWindowLongPtrW,
                SetWindowPlacement, SetWindowPos, ShowWindow, TranslateMessage, WINDOWPLACEMENT,
                WINDOWPLACEMENT_FLAGS, WM_ACTIVATE, WM_APP, WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE,
                WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_HOTKEY,
                WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
                WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
                WM_MOUSEWHEEL, WM_MOVE, WM_NCCALCSIZE, WM_NCHITTEST, WM_NCLBUTTONDOWN,
                WM_NCMBUTTONDOWN, WM_NCRBUTTONDOWN, WM_PAINT, WM_POWERBROADCAST, WM_RBUTTONDBLCLK,
                WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETICON, WM_SETTINGCHANGE, WM_SIZE, WM_SYSCHAR,
                WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WNDCLASSEXW, WS_EX_APPWINDOW, WS_EX_LAYERED,
                WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
                WS_MINIMIZEBOX, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
            },
        },
    },
    core::{BOOL, HSTRING, PCWSTR, w},
};

/// A window of this process. Calls on a destroyed window simply fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Window(isize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Either Windows key.
    pub win: bool,
}

/// Notification-area icon messages (NOTIFYICON_VERSION_4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    LeftUp,
    MiddleUp,
    /// Right click or the menu key; screen position.
    Context(i32, i32),
    Move,
    /// Enter / Space with the icon focused.
    Select,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Pointer position in DIPs from the window's top-left.
    MouseMove(f32, f32),
    MouseLeave,
    MouseDown(Button, f32, f32, Modifiers),
    MouseUp(Button, f32, f32, Modifiers),
    DoubleClick(f32, f32),
    /// Wheel delta (120 per notch, positive away from the user).
    Wheel(i16, f32, f32),
    Key(u16, Modifiers),
    Activate(bool),
    CaptureLost,
    Timer(usize),
    /// The window's DPI changed (it moved to another monitor).
    DpiChanged,
    /// A system setting (theme, colours) or the displays changed.
    SettingChange,
    /// The taskbar restarted: notification icons must be added again.
    TaskbarCreated,
    Hotkey(i32),
    Tray(TrayEvent),
    /// `Poster::post` / `Window::post` with this code.
    App(usize),
    /// An app window (`create_app`) needs repainting.
    Paint,
    /// The user asked to close an app window (close button, Alt+F4).
    Close,
    /// An app window's client area changed size.
    Resized,
    /// An app window moved.
    Moved,
    /// A typed character (WM_CHAR UTF-16 unit, control characters
    /// included for text editing).
    Char(u16),
    /// The display turned on (`true`, dimmed included) or off, for windows
    /// registered with `presence::watch_display`.
    Display(bool),
    /// The PC switched between battery and mains (`presence::on_battery`
    /// is already up to date).
    PowerSource,
}

const WM_MOUSELEAVE: u32 = 0x02a3;

/// Messages this module posts.
const WM_APP_POST: u32 = WM_APP + 1;
pub const WM_TRAY: u32 = WM_APP + 2;
const WM_APP_TASK: u32 = WM_APP + 3;

type Handler = Box<dyn FnMut(Window, Event)>;
type Task = Box<dyn FnOnce()>;

thread_local! {
    static HANDLER: RefCell<Option<Handler>> = RefCell::new(None);
    static QUEUE: RefCell<VecDeque<(Window, Event)>> = const { RefCell::new(VecDeque::new()) };
    static TASKS: RefCell<VecDeque<Task>> = const { RefCell::new(VecDeque::new()) };
}
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// Sets the handler that receives every window's events.
pub fn set_handler(handler: impl FnMut(Window, Event) + 'static) {
    HANDLER.with(|h| *h.borrow_mut() = Some(Box::new(handler)));
}

fn dispatch(window: Window, event: Event) {
    QUEUE.with(|q| q.borrow_mut().push_back((window, event)));
    HANDLER.with(|h| {
        // Busy: we are inside the handler; it drains the queue below.
        let Ok(mut guard) = h.try_borrow_mut() else {
            return;
        };
        let Some(handler) = guard.as_mut() else {
            return;
        };
        while let Some((window, event)) = QUEUE.with(|q| q.borrow_mut().pop_front()) {
            handler(window, event);
        }
    });
}

/// Runs `task` from the message loop, outside any event handler (for modal
/// work such as menus, which pump messages themselves).
pub fn post_task(window: Window, task: impl FnOnce() + 'static) {
    TASKS.with(|t| t.borrow_mut().push_back(Box::new(task)));
    window.post_raw(WM_APP_TASK, 0);
}

fn modifiers() -> Modifiers {
    // SAFETY: GetKeyState only reads this thread's keyboard state.
    let down = |vk: u16| unsafe { GetKeyState(i32::from(vk)) } < 0;
    Modifiers {
        ctrl: down(VK_CONTROL.0),
        alt: down(VK_MENU.0),
        shift: down(VK_SHIFT.0),
        win: down(VK_LWIN.0) || down(VK_RWIN.0),
    }
}

fn point(window: Window, lparam: LPARAM) -> (f32, f32) {
    let x = (lparam.0 & 0xffff) as u16 as i16;
    let y = ((lparam.0 >> 16) & 0xffff) as u16 as i16;
    let scale = window.scale();
    (window.mirror_x(f32::from(x)) / scale, f32::from(y) / scale)
}

/// Per-window flags kept in GWLP_USERDATA.
const NO_ACTIVATE: isize = 1;
const TRACKING: isize = 2;
/// A normal top-level window (`create_app`).
const APP: isize = 4;
/// Laid out right to left: pointer x counts from the right edge.
const MIRRORED: isize = 8;
/// Draws its own frame (`create_chrome_app`): the whole window is client
/// area, with a caption strip and resize borders hit-tested here.
const CHROME: isize = 16;

/// A self-drawn window frame (`create_chrome_app`), in DIPs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chrome {
    /// The caption strip's height: dragging there moves the window.
    pub caption: f32,
    /// The caption buttons' width at the strip's end (right, or left when
    /// mirrored), which stay client area.
    pub buttons: f32,
    /// Whether the edges resize the window.
    pub resizable: bool,
}

/// An app window's size limits in DIPs (the window size for a
/// `create_chrome_app` window, the client size otherwise).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Limits {
    min: (f32, f32),
    max: Option<(f32, f32)>,
}

/// A window's placement (GetWindowPlacement), in workspace pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Placement {
    pub flags: u32,
    pub show_cmd: u32,
    pub min_position: (i32, i32),
    pub max_position: (i32, i32),
    /// Left, top, right, bottom of the restored window.
    pub normal: (i32, i32, i32, i32),
}

thread_local! {
    /// App windows' size limits.
    static MIN_SIZE: RefCell<Vec<(isize, Limits)>> = const { RefCell::new(Vec::new()) };
    /// Self-drawn frames by window.
    static CHROMES: RefCell<Vec<(isize, Chrome)>> = const { RefCell::new(Vec::new()) };
}

fn is_chrome(window: Window) -> bool {
    // SAFETY: reads our own window's flags.
    unsafe { GetWindowLongPtrW(window.hwnd(), GWLP_USERDATA) & CHROME != 0 }
}

/// Hit-tests a self-drawn frame: resize edges, the caption strip, else
/// the client area.
fn chrome_hit(window: Window, lparam: LPARAM) -> Option<LRESULT> {
    let chrome = CHROMES.with(|c| {
        c.borrow()
            .iter()
            .find(|(w, _)| *w == window.0)
            .map(|(_, c)| *c)
    })?;
    let (sx, sy) = (
        i32::from((lparam.0 & 0xffff) as u16 as i16),
        i32::from(((lparam.0 >> 16) & 0xffff) as u16 as i16),
    );
    let (left, top, w, h) = window.bounds();
    let scale = window.scale();
    let (x, y) = ((sx - left) as f32, (sy - top) as f32);
    let (w, h) = (w as f32, h as f32);
    // WindowResizeBorderThickness: the sizing frame plus its padding.
    let border = 8.0 * scale;
    let edge = if chrome.resizable {
        let (l, r) = (x < border, x >= w - border);
        let (t, b) = (y < border, y >= h - border);
        match (l, r, t, b) {
            (true, _, true, _) => Some(HTTOPLEFT),
            (_, true, true, _) => Some(HTTOPRIGHT),
            (true, _, _, true) => Some(HTBOTTOMLEFT),
            (_, true, _, true) => Some(HTBOTTOMRIGHT),
            (true, ..) => Some(HTLEFT),
            (_, true, ..) => Some(HTRIGHT),
            (_, _, true, _) => Some(HTTOP),
            (.., true) => Some(HTBOTTOM),
            _ => None,
        }
    } else {
        None
    };
    let code = edge.unwrap_or_else(|| {
        let along = if window.is_mirrored() { w - x } else { x };
        if y < chrome.caption * scale && along < w - chrome.buttons * scale {
            HTCAPTION
        } else {
            HTCLIENT
        }
    });
    Some(LRESULT(code as isize))
}

/// Messages app windows handle themselves (painting, closing, sizing).
fn app_message(window: Window, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    let hwnd = window.hwnd();
    match msg {
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            // SAFETY: validates our window's update region; the event
            // handler redraws the whole client area.
            unsafe {
                BeginPaint(hwnd, &mut paint);
            }
            dispatch(window, Event::Paint);
            // SAFETY: pairs the BeginPaint above.
            unsafe {
                let _ = EndPaint(hwnd, &paint);
            }
            Some(LRESULT(0))
        }
        // The handler paints every pixel.
        WM_ERASEBKGND => Some(LRESULT(1)),
        WM_CLOSE => {
            dispatch(window, Event::Close);
            Some(LRESULT(0))
        }
        WM_SIZE => {
            dispatch(window, Event::Resized);
            Some(LRESULT(0))
        }
        WM_MOVE => {
            dispatch(window, Event::Moved);
            Some(LRESULT(0))
        }
        // A self-drawn frame: the client area is the whole window.
        WM_NCCALCSIZE if wparam.0 != 0 && is_chrome(window) => Some(LRESULT(0)),
        WM_NCHITTEST if is_chrome(window) => chrome_hit(window, lparam),
        WM_DPICHANGED => {
            // SAFETY: for WM_DPICHANGED, lParam points to the suggested
            // window rectangle for the new DPI.
            let r = unsafe { *(lparam.0 as *const RECT) };
            // SAFETY: resizes our own window.
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            dispatch(window, Event::DpiChanged);
            Some(LRESULT(0))
        }
        WM_GETMINMAXINFO => {
            let limits = MIN_SIZE.with(|m| {
                m.borrow()
                    .iter()
                    .find(|(w, _)| *w == window.0)
                    .map(|(_, limits)| *limits)
            })?;
            let scale = window.scale();
            // SAFETY: for WM_GETMINMAXINFO, lParam points to a MINMAXINFO
            // the system lets us fill in.
            unsafe {
                let info = &mut *(lparam.0 as *mut MINMAXINFO);
                info.ptMinTrackSize.x = (limits.min.0 * scale).round() as i32;
                info.ptMinTrackSize.y = (limits.min.1 * scale).round() as i32;
                if let Some(max) = limits.max {
                    info.ptMaxTrackSize.x = (max.0 * scale).round() as i32;
                    info.ptMaxTrackSize.y = (max.1 * scale).round() as i32;
                }
            }
            Some(LRESULT(0))
        }
        // Alt+F4 keeps its default meaning (WM_CLOSE).
        WM_SYSKEYDOWN if wparam.0 == usize::from(VK_F4.0) => {
            // SAFETY: default processing for our window.
            Some(unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) })
        }
        // Alt+letter arrives as a key; no menu beep.
        WM_SYSCHAR => Some(LRESULT(0)),
        _ => None,
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let window = Window(hwnd.0 as isize);
    // SAFETY: reads/writes our own window's user data.
    let flags = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    let set_flags = |f: isize| unsafe {
        // SAFETY: as above.
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, f)
    };
    if flags & APP != 0 {
        if let Some(result) = app_message(window, msg, wparam, lparam) {
            return result;
        }
    }
    let event = match msg {
        WM_CHAR => Some(Event::Char(wparam.0 as u16)),
        WM_MOUSEMOVE => {
            if flags & TRACKING == 0 {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                // SAFETY: `track` is a valid TRACKMOUSEEVENT for our window.
                unsafe {
                    let _ = TrackMouseEvent(&mut track);
                }
                set_flags(flags | TRACKING);
            }
            let (x, y) = point(window, lparam);
            Some(Event::MouseMove(x, y))
        }
        WM_MOUSELEAVE => {
            set_flags(flags & !TRACKING);
            Some(Event::MouseLeave)
        }
        WM_LBUTTONDOWN | WM_MBUTTONDOWN | WM_RBUTTONDOWN => {
            let (x, y) = point(window, lparam);
            let button = match msg {
                WM_LBUTTONDOWN => Button::Left,
                WM_MBUTTONDOWN => Button::Middle,
                _ => Button::Right,
            };
            Some(Event::MouseDown(button, x, y, modifiers()))
        }
        WM_LBUTTONUP | WM_MBUTTONUP | WM_RBUTTONUP => {
            let (x, y) = point(window, lparam);
            let button = match msg {
                WM_LBUTTONUP => Button::Left,
                WM_MBUTTONUP => Button::Middle,
                _ => Button::Right,
            };
            Some(Event::MouseUp(button, x, y, modifiers()))
        }
        WM_LBUTTONDBLCLK => {
            let (x, y) = point(window, lparam);
            Some(Event::DoubleClick(x, y))
        }
        WM_MOUSEWHEEL => {
            // Wheel positions are in screen coordinates.
            let (sx, sy) = (
                (lparam.0 & 0xffff) as u16 as i16,
                ((lparam.0 >> 16) & 0xffff) as u16 as i16,
            );
            let origin = window.screen_origin();
            let scale = window.scale();
            let delta = ((wparam.0 >> 16) & 0xffff) as u16 as i16;
            Some(Event::Wheel(
                delta,
                window.mirror_x(f32::from(sx) - origin.0 as f32) / scale,
                (f32::from(sy) - origin.1 as f32) / scale,
            ))
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => Some(Event::Key(wparam.0 as u16, modifiers())),
        WM_ACTIVATE => Some(Event::Activate((wparam.0 & 0xffff) != 0)),
        WM_MOUSEACTIVATE if flags & NO_ACTIVATE != 0 => {
            return LRESULT(MA_NOACTIVATE as isize);
        }
        WM_CAPTURECHANGED => Some(Event::CaptureLost),
        WM_TIMER => Some(Event::Timer(wparam.0)),
        WM_DPICHANGED => {
            dispatch(window, Event::DpiChanged);
            return LRESULT(0);
        }
        WM_SETTINGCHANGE | WM_DISPLAYCHANGE => Some(Event::SettingChange),
        WM_HOTKEY => Some(Event::Hotkey(wparam.0 as i32)),
        WM_TRAY => {
            let code = (lparam.0 & 0xffff) as u32;
            // NOTIFYICON_VERSION_4: the anchor point is in wParam.
            let (x, y) = (
                (wparam.0 & 0xffff) as u16 as i16,
                ((wparam.0 >> 16) & 0xffff) as u16 as i16,
            );
            const NIN_SELECT: u32 = 0x400;
            const NIN_KEYSELECT: u32 = 0x401;
            const WM_CONTEXTMENU: u32 = 0x7b;
            match code {
                WM_LBUTTONUP => Some(Event::Tray(TrayEvent::LeftUp)),
                WM_MBUTTONUP => Some(Event::Tray(TrayEvent::MiddleUp)),
                WM_CONTEXTMENU => Some(Event::Tray(TrayEvent::Context(i32::from(x), i32::from(y)))),
                WM_MOUSEMOVE => Some(Event::Tray(TrayEvent::Move)),
                NIN_KEYSELECT => Some(Event::Tray(TrayEvent::Select)),
                NIN_SELECT => None,
                _ => None,
            }
        }
        WM_APP_POST => Some(Event::App(wparam.0)),
        WM_POWERBROADCAST if wparam.0 == PBT_APMPOWERSTATUSCHANGE as usize => {
            let before = super::presence::on_battery();
            if super::presence::refresh_power() != before {
                dispatch(window, Event::PowerSource);
            }
            return LRESULT(1);
        }
        WM_POWERBROADCAST if wparam.0 == PBT_POWERSETTINGCHANGE as usize => {
            // SAFETY: for PBT_POWERSETTINGCHANGE, lParam points to a
            // POWERBROADCAST_SETTING whose data holds at least a DWORD for
            // the display state.
            let setting = unsafe {
                &*(lparam.0 as *const windows::Win32::System::Power::POWERBROADCAST_SETTING)
            };
            if setting.PowerSetting != super::presence::DISPLAY_STATE || setting.DataLength < 4 {
                return LRESULT(1);
            }
            // SAFETY: as above, DataLength covers the first DWORD.
            let state = unsafe { std::ptr::read_unaligned(setting.Data.as_ptr() as *const u32) };
            dispatch(window, Event::Display(state != 0));
            return LRESULT(1);
        }
        WM_APP_TASK => {
            while let Some(task) = TASKS.with(|t| t.borrow_mut().pop_front()) {
                task();
            }
            return LRESULT(0);
        }
        _ if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            Some(Event::TaskbarCreated)
        }
        _ => None,
    };
    if let Some(event) = event {
        dispatch(window, event);
        if msg != WM_ACTIVATE && msg != WM_SETTINGCHANGE && msg != WM_DISPLAYCHANGE {
            return LRESULT(0);
        }
    }
    // SAFETY: default processing for our window.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

const CLASS: PCWSTR = w!("BetterTrumpetRust.Window");

fn register_class() -> bool {
    static REGISTERED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *REGISTERED.get_or_init(|| {
        // SAFETY: registers our window class once; the class name is a
        // static wide string and wndproc lives for the program.
        unsafe {
            let Ok(module) = GetModuleHandleW(None) else {
                return false;
            };
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW
                    | CS_VREDRAW
                    | windows::Win32::UI::WindowsAndMessaging::CS_DBLCLKS,
                lpfnWndProc: Some(wndproc),
                hInstance: module.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: CLASS,
                hIcon: HICON::default(),
                ..Default::default()
            };
            TASKBAR_CREATED.store(
                RegisterWindowMessageW(w!("TaskbarCreated")),
                Ordering::Relaxed,
            );
            RegisterClassExW(&class) != 0
        }
    })
}

/// How a window behaves.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// Takes keyboard focus when clicked or shown with `show(true)`.
    pub activate: bool,
    /// Ignores the pointer entirely (clicks go to what is below).
    pub click_through: bool,
}

/// Creates a hidden layered popup (always on top, not in the taskbar).
pub fn create(title: &str, options: Options) -> windows::core::Result<Window> {
    create_with(title, options, true)
}

/// A hidden window for the notification icon, hotkeys and broadcasts
/// (message-only windows get no broadcasts such as TaskbarCreated).
pub fn create_hidden(title: &str) -> windows::core::Result<Window> {
    create_with(
        title,
        Options {
            activate: true,
            click_through: false,
        },
        false,
    )
}

/// Creates a hidden normal top-level window (caption, resizable, a taskbar
/// button with the app icon), `size` DIPs centred on the monitor under the
/// pointer and at least `min` DIPs. It receives `Paint`, `Close` and
/// `Resized`; show it with `Window::present_app` and remove it with
/// `Window::destroy`.
pub fn create_app(title: &str, size: (f32, f32), min: (f32, f32)) -> windows::core::Result<Window> {
    if !register_class() {
        return Err(windows::core::Error::from_thread());
    }
    let (cx, cy) = cursor_pos();
    let monitor = monitor_at(cx, cy);
    let (wx, wy, ww, wh) = monitor.work;
    let w = ((size.0 * monitor.scale).round() as i32).min(ww);
    let h = ((size.1 * monitor.scale).round() as i32).min(wh);
    // SAFETY: creates a window of our registered class; the title is an
    // HSTRING that outlives the call. The icons are shared resources of
    // this module (LR_SHARED), never destroyed.
    unsafe {
        let module = GetModuleHandleW(None)?;
        let hwnd = CreateWindowExW(
            Default::default(),
            CLASS,
            &HSTRING::from(title),
            WS_OVERLAPPEDWINDOW,
            wx + (ww - w) / 2,
            wy + (wh - h) / 2,
            w,
            h,
            None,
            None,
            Some(module.into()),
            None,
        )?;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, APP);
        MIN_SIZE.with(|m| {
            m.borrow_mut()
                .push((hwnd.0 as isize, Limits { min, max: None }))
        });
        set_app_icons(hwnd, module);
        Ok(Window(hwnd.0 as isize))
    }
}

/// Creates a hidden top-level layered window that draws its own frame
/// (the original's `WindowStyle="None"` + `AllowsTransparency` dialog
/// windows): a taskbar button with the app icon, the system menu and
/// minimising, not topmost. Present it with `Surface::present` and
/// `Window::present_app`; it receives `Close`, `Resized`, `Moved` and
/// `DpiChanged`, and `set_chrome` makes its caption and edges work.
pub fn create_chrome_app(title: &str) -> windows::core::Result<Window> {
    if !register_class() {
        return Err(windows::core::Error::from_thread());
    }
    // SAFETY: creates a window of our registered class; the title is an
    // HSTRING that outlives the call.
    unsafe {
        let module = GetModuleHandleW(None)?;
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_APPWINDOW,
            CLASS,
            &HSTRING::from(title),
            WS_POPUP | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(module.into()),
            None,
        )?;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, APP | CHROME);
        // WM_NCCALCSIZE again, now that the whole window is client area.
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
        set_app_icons(hwnd, module);
        Ok(Window(hwnd.0 as isize))
    }
}

/// The application icon on a window's caption and taskbar button.
///
/// # Safety
/// `hwnd` must be a window of this thread and `module` this executable.
unsafe fn set_app_icons(hwnd: HWND, module: windows::Win32::Foundation::HMODULE) {
    // SAFETY: the icons are shared resources of this module (LR_SHARED),
    // never destroyed; WM_SETICON goes to our own window.
    unsafe {
        for (kind, metric) in [(ICON_BIG, SM_CXICON), (ICON_SMALL, SM_CXSMICON)] {
            let px = GetSystemMetrics(metric);
            // MAKEINTRESOURCE(1): the application icon.
            if let Ok(icon) = LoadImageW(
                Some(module.into()),
                PCWSTR(std::ptr::without_provenance(1)),
                IMAGE_ICON,
                px,
                px,
                LR_DEFAULTCOLOR | LR_SHARED,
            ) {
                SendMessageW(
                    hwnd,
                    WM_SETICON,
                    Some(WPARAM(kind as usize)),
                    Some(LPARAM(icon.0 as isize)),
                );
            }
        }
    }
}

/// Creates a hidden borderless layered window that takes focus and has a
/// taskbar button with the app icon (a WPF `WindowStyle="None"`,
/// `AllowsTransparency` window): not topmost, drawn with
/// `Surface::present`. It receives `Close` (Alt+F4); show it with
/// `Window::present_app` and remove it with `Window::destroy`.
pub fn create_layered_app(title: &str) -> windows::core::Result<Window> {
    if !register_class() {
        return Err(windows::core::Error::from_thread());
    }
    // SAFETY: creates a window of our registered class; the title is an
    // HSTRING that outlives the call; the icons are set on that window.
    unsafe {
        let module = GetModuleHandleW(None)?;
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_APPWINDOW,
            CLASS,
            &HSTRING::from(title),
            WS_POPUP | WS_SYSMENU,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(module.into()),
            None,
        )?;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, APP);
        set_app_icons(hwnd, module);
        Ok(Window(hwnd.0 as isize))
    }
}

fn create_with(title: &str, options: Options, layered: bool) -> windows::core::Result<Window> {
    if !register_class() {
        return Err(windows::core::Error::from_thread());
    }
    let mut ex = WS_EX_TOOLWINDOW | WS_EX_TOPMOST;
    if layered {
        ex |= WS_EX_LAYERED;
    }
    if !options.activate {
        ex |= WS_EX_NOACTIVATE;
    }
    if options.click_through {
        ex |= WS_EX_TRANSPARENT;
    }
    // SAFETY: creates a window of our registered class; the title is an
    // HSTRING that outlives the call.
    unsafe {
        let module = GetModuleHandleW(None)?;
        let hwnd = CreateWindowExW(
            ex,
            CLASS,
            &HSTRING::from(title),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(module.into()),
            None,
        )?;
        if !options.activate {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, NO_ACTIVATE);
        }
        Ok(Window(hwnd.0 as isize))
    }
}

/// Prepares the UI thread: COM (single-threaded apartment) for WIC and
/// the shell.
pub fn init_thread() {
    // SAFETY: initialises COM for this thread once, before any COM use; it
    // stays initialised for the life of the process.
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }
}

/// Runs the message loop until `quit`.
pub fn run() {
    let mut message = MSG::default();
    // SAFETY: the standard loop on this thread's queue.
    unsafe {
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// Input a nested loop (`run_modal`) sees before any window does.
/// Positions are screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalInput {
    MouseMove(Window, i32, i32),
    /// Client or non-client (title bar) presses.
    MouseDown(Window, Button, i32, i32),
    MouseUp(Window, Button, i32, i32),
    Wheel(Window),
    Key(u16, Modifiers),
    KeyUp(u16),
    Timer(Window, usize),
}

/// Runs a nested message loop on this thread, as a menu does, until
/// `done` returns true. Input messages for this thread's windows go to
/// `filter` first, which returns true to swallow them; everything else is
/// dispatched as usual, so other windows keep working. A quit request ends
/// the loop and is passed on to the outer one.
pub fn run_modal(mut filter: impl FnMut(ModalInput) -> bool, mut done: impl FnMut() -> bool) {
    let mut message = MSG::default();
    while !done() {
        // SAFETY: the standard loop on this thread's queue.
        let got = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if got.0 == 0 {
            // SAFETY: re-posts WM_QUIT for the outer loop.
            unsafe { PostQuitMessage(message.wParam.0 as i32) };
            return;
        }
        if got.0 == -1 {
            return;
        }
        let window = Window(message.hwnd.0 as isize);
        let (x, y) = (message.pt.x, message.pt.y);
        let input = match message.message {
            WM_MOUSEMOVE => Some(ModalInput::MouseMove(window, x, y)),
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_NCLBUTTONDOWN => {
                Some(ModalInput::MouseDown(window, Button::Left, x, y))
            }
            WM_RBUTTONDOWN | WM_RBUTTONDBLCLK | WM_NCRBUTTONDOWN => {
                Some(ModalInput::MouseDown(window, Button::Right, x, y))
            }
            WM_MBUTTONDOWN | WM_MBUTTONDBLCLK | WM_NCMBUTTONDOWN => {
                Some(ModalInput::MouseDown(window, Button::Middle, x, y))
            }
            WM_LBUTTONUP => Some(ModalInput::MouseUp(window, Button::Left, x, y)),
            WM_RBUTTONUP => Some(ModalInput::MouseUp(window, Button::Right, x, y)),
            WM_MBUTTONUP => Some(ModalInput::MouseUp(window, Button::Middle, x, y)),
            WM_MOUSEWHEEL => Some(ModalInput::Wheel(window)),
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                Some(ModalInput::Key(message.wParam.0 as u16, modifiers()))
            }
            WM_KEYUP | WM_SYSKEYUP => Some(ModalInput::KeyUp(message.wParam.0 as u16)),
            WM_TIMER => Some(ModalInput::Timer(window, message.wParam.0)),
            _ => None,
        };
        if input.is_some_and(&mut filter) {
            continue;
        }
        // SAFETY: as in `run`.
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// The window in the foreground (any process), as a comparable handle.
pub fn foreground() -> Window {
    // SAFETY: plain query.
    Window(unsafe { GetForegroundWindow() }.0 as isize)
}

/// Whether a mouse button is down, or was pressed since the last call
/// (the asynchronous state, so presses in other processes count too).
pub fn button_pressed() -> bool {
    // SAFETY: plain queries of the asynchronous key state.
    [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
        .into_iter()
        .any(|vk| unsafe { GetAsyncKeyState(i32::from(vk.0)) } != 0)
}

/// Whether a key is held down right now (the asynchronous state, as
/// WPF's `Keyboard.IsKeyDown` at startup; `vk` is a virtual-key code).
pub fn key_down(vk: u16) -> bool {
    // SAFETY: plain query of the asynchronous key state.
    let state = unsafe { GetAsyncKeyState(i32::from(vk)) };
    state < 0
}

/// Ends the message loop.
pub fn quit() {
    // SAFETY: posts WM_QUIT to this thread's queue.
    unsafe { PostQuitMessage(0) };
}

/// A thread-safe handle for posting `Event::App` codes to a window.
#[derive(Clone, Copy, Debug)]
pub struct Poster(isize);

impl Poster {
    pub fn post(&self, code: usize) {
        // SAFETY: PostMessage is thread-safe; a destroyed window only makes
        // the call fail.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.0 as *mut c_void)),
                WM_APP_POST,
                WPARAM(code),
                LPARAM(0),
            );
        }
    }
}

/// A monitor's bounds and work area in physical pixels, and its scale.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Monitor {
    pub bounds: (i32, i32, i32, i32),
    pub work: (i32, i32, i32, i32),
    pub scale: f32,
}

fn rect_tuple(r: RECT) -> (i32, i32, i32, i32) {
    (r.left, r.top, r.right - r.left, r.bottom - r.top)
}

/// The monitor nearest a screen point.
pub fn monitor_at(x: i32, y: i32) -> Monitor {
    // SAFETY: plain queries with valid out-parameters.
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        Monitor {
            bounds: rect_tuple(info.rcMonitor),
            work: rect_tuple(info.rcWork),
            scale: dx as f32 / 96.0,
        }
    }
}

pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT::default();
    // SAFETY: valid out-parameter.
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

/// Registers a system-wide hotkey on `window` (`modifiers` are MOD_* flags,
/// MOD_NOREPEAT added).
pub fn register_hotkey(window: Window, id: i32, modifiers: u32, vk: u32) -> bool {
    // SAFETY: registers for our window; fails if the combination is taken.
    unsafe {
        RegisterHotKey(
            Some(window.hwnd()),
            id,
            HOT_KEY_MODIFIERS(modifiers | 0x4000),
            vk,
        )
        .is_ok()
    }
}

pub fn unregister_hotkey(window: Window, id: i32) {
    // SAFETY: as above.
    unsafe {
        let _ = UnregisterHotKey(Some(window.hwnd()), id);
    }
}

impl Window {
    pub(crate) fn from_raw(raw: isize) -> Self {
        Self(raw)
    }
    pub(crate) fn hwnd(self) -> HWND {
        HWND(self.0 as *mut c_void)
    }
    pub fn raw(self) -> isize {
        self.0
    }
    pub fn poster(self) -> Poster {
        Poster(self.0)
    }
    pub fn post(self, code: usize) {
        self.poster().post(code);
    }
    fn post_raw(self, msg: u32, code: usize) {
        // SAFETY: posts to our own window.
        unsafe {
            let _ = PostMessageW(Some(self.hwnd()), msg, WPARAM(code), LPARAM(0));
        }
    }
    /// Physical pixels per DIP for the monitor the window is on.
    pub fn scale(self) -> f32 {
        // SAFETY: plain query; 0 for a bad handle.
        let dpi = unsafe { GetDpiForWindow(self.hwnd()) };
        if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }
    }
    fn screen_origin(self) -> (i32, i32) {
        let mut r = RECT::default();
        // SAFETY: valid out-parameter.
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(self.hwnd(), &mut r);
        }
        (r.left, r.top)
    }
    pub fn show(self, activate: bool) {
        // SAFETY: shows our window and keeps it on top.
        unsafe {
            let _ = ShowWindow(
                self.hwnd(),
                if activate { SW_SHOW } else { SW_SHOWNOACTIVATE },
            );
            let _ = SetWindowPos(
                self.hwnd(),
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
    pub fn hide(self) {
        // SAFETY: hides our window.
        unsafe {
            let _ = ShowWindow(self.hwnd(), SW_HIDE);
        }
    }
    pub fn is_visible(self) -> bool {
        // SAFETY: plain query.
        unsafe { IsWindowVisible(self.hwnd()).as_bool() }
    }
    /// Brings the window to the foreground with keyboard focus.
    pub fn focus(self) {
        // SAFETY: as above; Windows may refuse the foreground change.
        unsafe {
            let _ = SetForegroundWindow(self.hwnd());
            let _ = SetFocus(Some(self.hwnd()));
        }
    }
    pub fn is_foreground(self) -> bool {
        // SAFETY: plain query.
        unsafe { GetForegroundWindow() == self.hwnd() }
    }
    pub fn set_timer(self, id: usize, ms: u32) {
        // SAFETY: a WM_TIMER timer on our window.
        unsafe {
            SetTimer(Some(self.hwnd()), id, ms.max(1), None);
        }
    }
    pub fn kill_timer(self, id: usize) {
        // SAFETY: as above.
        unsafe {
            let _ = KillTimer(Some(self.hwnd()), id);
        }
    }
    pub fn capture(self) {
        // SAFETY: captures the mouse for our window.
        unsafe {
            SetCapture(self.hwnd());
        }
    }
    pub fn release_capture(self) {
        // SAFETY: releases our capture, if any.
        unsafe {
            let _ = ReleaseCapture();
        }
    }
    /// The original's acrylic (SetWindowCompositionAttribute accent
    /// policy) with an ABGR tint, or `None` to turn it off.
    pub fn set_acrylic(self, tint: Option<u32>) {
        #[repr(C)]
        struct AccentPolicy {
            state: i32,
            flags: i32,
            gradient: u32,
            animation: i32,
        }
        #[repr(C)]
        struct Data {
            attribute: i32,
            data: *mut c_void,
            size: usize,
        }
        type SetWca = unsafe extern "system" fn(HWND, *mut Data) -> BOOL;
        const WCA_ACCENT_POLICY: i32 = 19;
        const ACCENT_ENABLE_ACRYLICBLURBEHIND: i32 = 4;
        const DRAW_ALL_BORDERS: i32 = 0x20 | 0x40 | 0x80 | 0x100;
        let mut policy = AccentPolicy {
            state: if tint.is_some() {
                ACCENT_ENABLE_ACRYLICBLURBEHIND
            } else {
                0
            },
            flags: DRAW_ALL_BORDERS,
            gradient: tint.unwrap_or(0),
            animation: 0,
        };
        let mut data = Data {
            attribute: WCA_ACCENT_POLICY,
            data: (&raw mut policy).cast(),
            size: size_of::<AccentPolicy>(),
        };
        // SAFETY: SetWindowCompositionAttribute is an undocumented but
        // stable user32 export (the original calls it); we look it up and
        // call it with the layout the original declares. `policy` and `data`
        // outlive the call.
        unsafe {
            let Ok(user32) = GetModuleHandleW(w!("user32.dll")) else {
                return;
            };
            let Some(function) =
                GetProcAddress(user32, windows::core::s!("SetWindowCompositionAttribute"))
            else {
                return;
            };
            let set: SetWca = std::mem::transmute(function);
            let _ = set(self.hwnd(), &mut data);
        }
    }
    /// Shows an app window (restoring it if minimised) and brings it to
    /// the foreground.
    pub fn present_app(self) {
        // SAFETY: shows and activates our window; Windows may refuse the
        // foreground change.
        unsafe {
            let command = if IsIconic(self.hwnd()).as_bool() {
                SW_RESTORE
            } else {
                SW_SHOW
            };
            let _ = ShowWindow(self.hwnd(), command);
            let _ = SetForegroundWindow(self.hwnd());
        }
    }
    /// Destroys the window (its events stop; the handle becomes stale).
    pub fn destroy(self) {
        MIN_SIZE.with(|m| m.borrow_mut().retain(|(w, _)| *w != self.0));
        CHROMES.with(|c| c.borrow_mut().retain(|(w, _)| *w != self.0));
        // SAFETY: destroys our own window on its thread.
        unsafe {
            let _ = DestroyWindow(self.hwnd());
        }
    }
    /// A client point in DIPs as a screen position in pixels (x from the
    /// right edge on a mirrored window).
    pub fn client_to_screen(self, x: f32, y: f32) -> (i32, i32) {
        let scale = self.scale();
        let mut p = POINT {
            x: self.mirror_x(x * scale).round() as i32,
            y: (y * scale).round() as i32,
        };
        // SAFETY: valid in/out parameter for our window.
        unsafe {
            let _ = ClientToScreen(self.hwnd(), &mut p);
        }
        (p.x, p.y)
    }
    /// Lays the window out right to left (`FlowDirection="RightToLeft"`):
    /// pointer events and `client_to_screen` count x from the right edge,
    /// to match a surface drawn with `Surface::set_mirrored`.
    pub fn set_mirrored(self, mirrored: bool) {
        // SAFETY: reads and writes our own window's flags.
        unsafe {
            let flags = GetWindowLongPtrW(self.hwnd(), GWLP_USERDATA);
            let flags = if mirrored {
                flags | MIRRORED
            } else {
                flags & !MIRRORED
            };
            SetWindowLongPtrW(self.hwnd(), GWLP_USERDATA, flags);
        }
    }
    pub fn is_mirrored(self) -> bool {
        // SAFETY: reads our own window's flags.
        unsafe { GetWindowLongPtrW(self.hwnd(), GWLP_USERDATA) & MIRRORED != 0 }
    }
    /// A client x in pixels, counted from the right on a mirrored window.
    fn mirror_x(self, x: f32) -> f32 {
        if self.is_mirrored() {
            self.client_size().0 as f32 - x
        } else {
            x
        }
    }
    /// Client area size in physical pixels.
    pub fn client_size(self) -> (i32, i32) {
        let mut r = RECT::default();
        // SAFETY: valid out-parameter.
        unsafe {
            let _ = GetClientRect(self.hwnd(), &mut r);
        }
        (r.right - r.left, r.bottom - r.top)
    }
    pub fn is_minimized(self) -> bool {
        // SAFETY: plain query.
        unsafe { IsIconic(self.hwnd()).as_bool() }
    }
    /// An app window's frame: a dark or light caption, and the Mica
    /// backdrop behind the whole window (Windows 11 22H2+). Returns whether
    /// Mica is on, in which case transparent client pixels show it.
    pub fn set_app_frame(self, dark: bool) -> bool {
        let dark = BOOL::from(dark);
        let backdrop = DWMSBT_MAINWINDOW;
        // SAFETY: each attribute gets a value of the documented size; the
        // margins extend the frame over the whole client area.
        unsafe {
            let _ = DwmSetWindowAttribute(
                self.hwnd(),
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&raw const dark).cast(),
                size_of_val(&dark) as u32,
            );
            let mica = DwmSetWindowAttribute(
                self.hwnd(),
                DWMWA_SYSTEMBACKDROP_TYPE,
                (&raw const backdrop).cast(),
                size_of_val(&backdrop) as u32,
            )
            .is_ok();
            if !mica {
                return false;
            }
            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            DwmExtendFrameIntoClientArea(self.hwnd(), &margins).is_ok()
        }
    }
    /// A `create_chrome_app` window's caption strip and resize edges.
    pub fn set_chrome(self, chrome: Chrome) {
        CHROMES.with(|c| {
            let mut c = c.borrow_mut();
            c.retain(|(w, _)| *w != self.0);
            c.push((self.0, chrome));
        });
    }
    /// An app window's smallest and (optionally) largest size in DIPs.
    pub fn set_limits(self, min: (f32, f32), max: Option<(f32, f32)>) {
        MIN_SIZE.with(|m| {
            let mut m = m.borrow_mut();
            m.retain(|(w, _)| *w != self.0);
            m.push((self.0, Limits { min, max }));
        });
    }
    /// The window's screen rectangle in pixels: x, y, width, height.
    pub fn bounds(self) -> (i32, i32, i32, i32) {
        let mut r = RECT::default();
        // SAFETY: valid out-parameter.
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(self.hwnd(), &mut r);
        }
        (r.left, r.top, r.right - r.left, r.bottom - r.top)
    }
    /// Moves and sizes the window (screen pixels) without activating it.
    pub fn set_bounds(self, x: i32, y: i32, width: i32, height: i32) {
        // SAFETY: moves our own window.
        unsafe {
            let _ = SetWindowPos(
                self.hwnd(),
                None,
                x,
                y,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
    /// GetWindowPlacement.
    pub fn placement(self) -> Option<Placement> {
        let mut p = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        // SAFETY: `p` is a WINDOWPLACEMENT with its length set.
        unsafe { GetWindowPlacement(self.hwnd(), &mut p) }.ok()?;
        let r = p.rcNormalPosition;
        Some(Placement {
            flags: p.flags.0,
            show_cmd: p.showCmd,
            min_position: (p.ptMinPosition.x, p.ptMinPosition.y),
            max_position: (p.ptMaxPosition.x, p.ptMaxPosition.y),
            normal: (r.left, r.top, r.right, r.bottom),
        })
    }
    /// SetWindowPlacement, keeping the window hidden (it is shown later).
    pub fn set_placement(self, placement: &Placement) {
        let (left, top, right, bottom) = placement.normal;
        let p = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            flags: WINDOWPLACEMENT_FLAGS(placement.flags),
            showCmd: SW_HIDE.0 as u32,
            ptMinPosition: POINT {
                x: placement.min_position.0,
                y: placement.min_position.1,
            },
            ptMaxPosition: POINT {
                x: placement.max_position.0,
                y: placement.max_position.1,
            },
            rcNormalPosition: RECT {
                left,
                top,
                right,
                bottom,
            },
        };
        // SAFETY: `p` is a complete WINDOWPLACEMENT for our window.
        unsafe {
            let _ = SetWindowPlacement(self.hwnd(), &p);
        }
    }
    /// DWM rounded corners (Windows 11; ignored before).
    pub fn round_corners(self) {
        let preference = DWMWCP_ROUND;
        // SAFETY: passes a DWORD-sized value with its size.
        unsafe {
            let _ = DwmSetWindowAttribute(
                self.hwnd(),
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&raw const preference).cast(),
                size_of_val(&preference) as u32,
            );
        }
    }
}
