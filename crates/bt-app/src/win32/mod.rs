//! Win32 shim: windows, drawing, the tray icon, menus and settings access.
//! Audio and media COM stay in bt-audio and bt-media.
pub mod clock;
pub mod gfx;
pub mod icons;
pub mod instance;
pub mod menu;
pub mod package;
pub mod registry;
pub mod sound;
pub mod startup;
pub mod theme;
pub mod tray;
pub mod wheel;
pub mod wnd;
