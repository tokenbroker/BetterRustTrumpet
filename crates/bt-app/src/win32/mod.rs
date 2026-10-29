//! Win32 shim: windows, drawing, the tray icon, menus and settings access.
//! Audio and media COM stay in bt-audio and bt-media.
pub mod clock;
pub mod diag;
pub mod dialog;
pub mod foreground;
pub mod gfx;
pub mod icons;
pub mod instance;
pub mod locale;
pub mod mainwindow;
pub mod memory;
pub mod menu;
pub mod package;
pub mod paths;
pub mod pipe;
pub mod presence;
pub mod registry;
pub mod sound;
pub mod startup;
pub mod theme;
pub mod tray;
pub mod trayicon;
pub mod wheel;
pub mod wnd;
