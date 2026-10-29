Better Rust Trumpet {version}
==========================

BetterTrumpet (EarTrumpet) rewritten in Rust: per-app volume control from
the Windows notification area. Alpha: please report anything odd.

Start
  Run BetterTrumpet.exe. If Windows SmartScreen warns (the file is not
  signed), choose "More info", then "Run anyway". The icon appears in the
  notification area; left click opens the flyout, right click the menu.

  Quit the C# BetterTrumpet or EarTrumpet first if it is running.

  It reads your existing EarTrumpet/BetterTrumpet settings but never changes
  them; its own settings go to %LOCALAPPDATA%\BetterTrumpet-Rust. It sends
  nothing over the network.

bt.exe
  Optional command-line tool for the running app. Try: bt --help

Report a problem
  https://github.com/tokenbroker/BetterRustTrumpet/issues
  Include your Windows version (Win+R, winver) and, if you can, the newest log
  from %LOCALAPPDATA%\BetterTrumpet-Rust\logs or a report from
  Settings > About > Export diagnostic report.

Uninstall
  Turn off "Run at Windows startup" in Settings if you turned it on, quit from
  the tray menu, then delete this folder and %LOCALAPPDATA%\BetterTrumpet-Rust.

Licence: MIT with the upstream preamble, see LICENSE. Based on EarTrumpet by
David Golden, Rafael Rivera and Dave Amenta, and BetterTrumpet by xammen.
