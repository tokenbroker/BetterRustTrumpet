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
                GetKeyState, HOT_KEY_MODIFIERS, RegisterHotKey, ReleaseCapture, SetCapture,
                SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, UnregisterHotKey,
                VK_CONTROL, VK_F4, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
            },
            WindowsAndMessaging::{
                CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, GWLP_USERDATA, GetClientRect, GetCursorPos, GetForegroundWindow,
                GetMessageW, GetSystemMetrics, GetWindowLongPtrW, HICON, HWND_TOPMOST, ICON_BIG,
                ICON_SMALL, IDC_ARROW, IMAGE_ICON, IsIconic, IsWindowVisible, KillTimer,
                LR_DEFAULTCOLOR, LR_SHARED, LoadCursorW, LoadImageW, MA_NOACTIVATE, MINMAXINFO,
                MSG, PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW,
                SM_CXICON, SM_CXSMICON, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE,
                SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW,
                SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
                TranslateMessage, WM_ACTIVATE, WM_APP, WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE,
                WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_HOTKEY,
                WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN,
                WM_MBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT,
                WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETICON, WM_SETTINGCHANGE, WM_SIZE, WM_SYSCHAR,
                WM_SYSKEYDOWN, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
                WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_OVERLAPPEDWINDOW, WS_POPUP,
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
    /// A typed character (WM_CHAR UTF-16 unit, control characters
    /// included for text editing).
    Char(u16),
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
    (f32::from(x) / scale, f32::from(y) / scale)
}

/// Per-window flags kept in GWLP_USERDATA.
const NO_ACTIVATE: isize = 1;
const TRACKING: isize = 2;
/// A normal top-level window (`create_app`).
const APP: isize = 4;

thread_local! {
    /// App windows' minimum client size in DIPs.
    static MIN_SIZE: RefCell<Vec<(isize, (f32, f32))>> = const { RefCell::new(Vec::new()) };
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
            let min = MIN_SIZE.with(|m| {
                m.borrow()
                    .iter()
                    .find(|(w, _)| *w == window.0)
                    .map(|(_, size)| *size)
            })?;
            let scale = window.scale();
            // SAFETY: for WM_GETMINMAXINFO, lParam points to a MINMAXINFO
            // the system lets us fill in.
            unsafe {
                let info = &mut *(lparam.0 as *mut MINMAXINFO);
                info.ptMinTrackSize.x = (min.0 * scale).round() as i32;
                info.ptMinTrackSize.y = (min.1 * scale).round() as i32;
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
                (f32::from(sx) - origin.0 as f32) / scale,
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
        MIN_SIZE.with(|m| m.borrow_mut().push((hwnd.0 as isize, min)));
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
        // SAFETY: destroys our own window on its thread.
        unsafe {
            let _ = DestroyWindow(self.hwnd());
        }
    }
    /// A client point in DIPs as a screen position in pixels.
    pub fn client_to_screen(self, x: f32, y: f32) -> (i32, i32) {
        let scale = self.scale();
        let mut p = POINT {
            x: (x * scale).round() as i32,
            y: (y * scale).round() as i32,
        };
        // SAFETY: valid in/out parameter for our window.
        unsafe {
            let _ = ClientToScreen(self.hwnd(), &mut p);
        }
        (p.x, p.y)
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
