//! Whether anyone can see the notification area, so the tray animation can
//! rest: the display's power state (GUID_CONSOLE_DISPLAY_STATE, delivered
//! to a window as `Event::Display`) and the shell's notification state
//! (locked session, screen saver, full-screen app). Also whether the PC
//! runs on battery, for AutoEcoMode.
use std::sync::atomic::{AtomicU8, Ordering};

use windows::{
    Win32::{
        Foundation::{HANDLE, HWND, POINT},
        Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromPoint, MonitorFromWindow},
        System::Power::{
            GetSystemPowerStatus, RegisterPowerSettingNotification, SYSTEM_POWER_STATUS,
        },
        UI::{
            Shell::{
                QUNS_BUSY, QUNS_NOT_PRESENT, QUNS_RUNNING_D3D_FULL_SCREEN,
                SHQueryUserNotificationState,
            },
            WindowsAndMessaging::{DEVICE_NOTIFY_WINDOW_HANDLE, GetForegroundWindow},
        },
    },
    core::GUID,
};

use super::wnd::Window;

/// GUID_CONSOLE_DISPLAY_STATE (Win32_System_SystemServices, not enabled).
pub const DISPLAY_STATE: GUID = GUID::from_u128(0x6fe69556_704a_47a0_8f24_c28d936fda47);

/// Sends `window` an `Event::Display` whenever the display turns off or on
/// (and once right away). The registration lasts for the app's lifetime.
pub fn watch_display(window: Window) {
    // SAFETY: registers our own top-level window; the handle is never
    // unregistered because the window lives until the process exits.
    unsafe {
        let _ = RegisterPowerSettingNotification(
            HANDLE(window.raw() as *mut _),
            &DISPLAY_STATE,
            DEVICE_NOTIFY_WINDOW_HANDLE,
        );
    }
}

/// The last power source seen: 0 unknown, 1 battery, 2 mains.
static BATTERY: AtomicU8 = AtomicU8::new(0);

/// Whether the PC runs on battery (SystemInformation.PowerStatus), from the
/// last WM_POWERBROADCAST or asked once.
pub fn on_battery() -> bool {
    match BATTERY.load(Ordering::Relaxed) {
        0 => refresh_power(),
        state => state == 1,
    }
}

/// Asks Windows for the power source again (PBT_APMPOWERSTATUSCHANGE).
pub fn refresh_power() -> bool {
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: `status` is a valid out-parameter.
    let ok = unsafe { GetSystemPowerStatus(&mut status) }.is_ok();
    // ACLineStatus: 0 offline (battery), 1 online, 255 unknown (desktops).
    let battery = ok && status.ACLineStatus == 0;
    BATTERY.store(if battery { 1 } else { 2 }, Ordering::Relaxed);
    battery
}

/// Whether the icon at `rect` (the tray's rectangle, physical pixels) is
/// hidden: the session is locked or not active, a screen saver runs, or a
/// full-screen app is in front on the taskbar's monitor.
pub fn tray_unseen(rect: Option<(i32, i32, i32, i32)>) -> bool {
    // SAFETY: plain shell and window-manager queries.
    unsafe {
        let Ok(state) = SHQueryUserNotificationState() else {
            return false;
        };
        if state == QUNS_NOT_PRESENT {
            return true;
        }
        if state != QUNS_BUSY && state != QUNS_RUNNING_D3D_FULL_SCREEN {
            return false;
        }
        // A full-screen app on another monitor leaves the taskbar visible.
        let Some((x, y, w, h)) = rect else {
            return true;
        };
        let foreground: HWND = GetForegroundWindow();
        if foreground.is_invalid() {
            return true;
        }
        let tray = MonitorFromPoint(
            POINT {
                x: x + w / 2,
                y: y + h / 2,
            },
            MONITOR_DEFAULTTONEAREST,
        );
        MonitorFromWindow(foreground, MONITOR_DEFAULTTONEAREST) == tray
    }
}
