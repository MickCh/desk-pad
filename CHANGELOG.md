# Changelog

All notable changes to this project are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- Built-in layout for the Art AG-16T: mouse buttons, keys and chords held
  as long as the pad's button, D-pad with auto-repeat, pause button.
- Left stick moves the pointer (radial dead zone, speed curve, sub-pixel
  steps); right stick scrolls smoothly, vertically and horizontally.
- `--test`: button test with timing (how long a button was held, how soon
  it was pressed again) that also shows the program and title of the window
  in front. `--seconds N` ends it, or a normal run, after N seconds.
- Keys held by desk-pad are released on pause, unplugging and exit
  (Ctrl+C, closing the console).
- Digital mode of DragonRise pads is detected; buttons 1–4 are ignored
  while it lasts.
- Configuration file `%APPDATA%\desk-pad\desk-pad.toml` (or
  `--config PATH`), created with the documented defaults on first start and
  reloaded when saved. Errors name the line; the previous settings stay
  active.
- Profiles for single programs (`match`), chosen by the program in front;
  their bindings replace the default ones. `passthrough = true` sends
  nothing: the default configuration does this for REAPER, where
  reaper-pad has the pad.
- Runs in the background with a tray icon: its colour shows the state, its
  menu pauses, reloads and opens the configuration, opens the log, starts
  desk-pad with Windows and quits. Notices show as Windows notifications;
  the log goes to `%APPDATA%\desk-pad\desk-pad.log`. `--console` runs in a
  console as before; `--test` opens a console of its own.
- Only one desk-pad runs at a time.
- `install.ps1` installs to `%LOCALAPPDATA%\Programs\desk-pad`.
