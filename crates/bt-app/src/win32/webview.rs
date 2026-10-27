//! Idle economy for hidden windows. Tauri's hide only hides the frame, so a
//! hidden WebView2 page would keep running scripts and timers. When hidden it
//! is marked invisible, asked to trim memory and suspended; showing it makes
//! it visible again, which resumes it.
use tauri::webview::PlatformWebview;
use webview2_com::{
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        ICoreWebView2_3, ICoreWebView2_19,
    },
    TrySuspendCompletedHandler,
};
use wv_core::Interface;

/// Marks the page hidden (`false`) or shown (`true`); call from a
/// `with_webview` closure. Best effort: older WebView2 runtimes without these
/// interfaces just keep running.
pub fn set_active(webview: PlatformWebview, active: bool) {
    let controller = webview.controller();
    // SAFETY: called on the webview's UI thread by `with_webview`; the
    // controller and the interfaces cast from it are live COM objects
    // owned by wry for the window's lifetime.
    unsafe {
        let _ = controller.SetIsVisible(active);
        let Ok(core) = controller.CoreWebView2() else {
            return;
        };
        if let Ok(memory) = core.cast::<ICoreWebView2_19>() {
            let _ = memory.SetMemoryUsageTargetLevel(if active {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
            } else {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
            });
        }
        if !active {
            if let Ok(suspend) = core.cast::<ICoreWebView2_3>() {
                let done = TrySuspendCompletedHandler::create(Box::new(|_, _| Ok(())));
                let _ = suspend.TrySuspend(&done);
            }
        }
    }
}
