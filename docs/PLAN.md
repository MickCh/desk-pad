# desk-pad: implementation plan

Use a cheap USB gamepad (defaults made for the **Art AG-16T**, a 12-button
DragonRise twin-stick pad) as keyboard and mouse on the Windows desktop:
switch tabs in Windows Terminal and the browser, move the mouse with one
stick, scroll with the other, switch windows, control Teams. Windows only
for now.

Tick an item off (`[x]`) in the same commit that does it.

## Shared code: `pad-core`

The input code is shared with [reaper-pad](https://github.com/MickCh/reaper-pad) through the
`pad-core` crate, a workspace member of the reaper-pad repository:

| `pad-core` module | What it gives desk-pad |
|---|---|
| `gamepad` | SDL2 device: first pad, hot-plug, button down/up, axes, hats, rumble, DragonRise detection |
| `input` | SDL-free `InputEvent`, `Device`, `Log`, `hat_name` |
| `logic` | `Debouncer`, `HatRepeat` (D-pad auto-repeat), `AxisParams` (stick curve), `Stepper<K>` (fractional → whole steps: pixels, wheel units), `ModeDetector` (DragonRise digital mode) |

Not shared (REAPER-specific, or only the pattern is reused): reaper-pad's
router, commands, config, guard and REAPER glue.

**Dependency:** a git dependency pinned to a tag (`pad-core-v*` tags in
reaper-pad; reaper-pad's own releases are `v*`):

```toml
pad-core = { git = "https://github.com/MickCh/reaper-pad", tag = "pad-core-v0.1.0" }
```

## Architecture

```
desk-pad.exe
├─ input loop (~250 Hz): pad-core Gamepad → Router → Output
│    Router: plain Rust, time as a parameter, unit-tested with a mock Output
│    Output (trait): key down/up, mouse button, mouse move, wheel
│    win::SendInputOutput: the real Output, through SendInput
├─ foreground watcher: active exe + window title → profile        (stage 4)
├─ tray icon, Win32 message loop (main thread)
└─ config file watcher: reload on save
```

- **Timing.** REAPER's timer (~30/s) is too slow for a mouse. desk-pad runs
  its own loop every ~4 ms with `timeBeginPeriod(1)`.
- **Keys** are sent with virtual key *and* scan code; the extended-key flag
  comes from `MapVirtualKey(MAPVK_VK_TO_VSC_EX)`. Every injected event is
  tagged (`dwExtraInfo`) as desk-pad's own.
- **Held keys are reference-counted**, so two bindings sharing a modifier
  (`ctrl+tab` while `ctrl+shift+tab` is held) never release it early, and
  everything still held is released when the pad is paused or unplugged and
  on exit (Ctrl+C, closing the console): no stuck Alt or Ctrl.
- **Mouse:** radial dead zone (both axes together), the quadratic curve of
  `AxisParams::rate`, sub-pixel accumulation with `Stepper`.
- **Scroll:** wheel units smaller than one notch (`WHEEL_DELTA` = 120) for
  smooth scrolling in browsers and Windows Terminal; vertical and horizontal.

## Mapping model

```toml
[profiles.default]
buttons.1  = "mouse:left"            # held = drag
buttons.2  = "mouse:right"
buttons.3  = "key:esc"
buttons.4  = "key:enter"
buttons.5  = "key:alt+v"             # paste an image in Claude CLI
buttons.6  = "key:shift+insert"      # paste an image in Devin CLI
# layer:nav and the switcher (stage 4) need other buttons or combinations.
buttons.7  = "key:alt+left"          # back
buttons.8  = "key:alt+right"         # forward
buttons.9  = "key:win+tab"
buttons.10 = "pad:toggle"            # pause the whole pad
buttons.11 = "mouse:middle"
buttons.12 = "key:win+alt+k"         # mute the mic in calls (Windows 11, works with Teams)
dpad.left  = "key:ctrl+shift+tab"    # previous tab (browser, Windows Terminal)
dpad.right = "key:ctrl+tab"          # next tab
dpad.up    = "key:up"
dpad.down  = "key:down"
sticks.left  = "mouse"
sticks.right = "scroll"

[profiles.default.layers.nav]        # while L1 is held
dpad.left  = "key:win+ctrl+left"     # virtual desktops
dpad.right = "key:win+ctrl+right"

[profiles.terminal]
match = ["WindowsTerminal.exe"]
inherit = "default"
buttons.11 = "key:shift+tab"
# zellij runs inside Windows Terminal and switches its tabs with Alt+←/→
# (buttons 7/8 of the default profile already send these). Undecided:
# D-pad ←→ for zellij tabs here, Windows Terminal tabs on a layer.
# dpad.left  = "key:alt+left"
# dpad.right = "key:alt+right"
[profiles.terminal.layers.nav]
dpad.left  = "key:alt+left"          # move focus between panes
dpad.right = "key:alt+right"

[profiles.teams]
match = ["ms-teams.exe"]
inherit = "default"
buttons.12 = "key:ctrl+shift+m"      # mute
buttons.11 = "key:ctrl+shift+o"      # camera

[profiles.reaper]
match = ["reaper.exe"]
passthrough = true                   # reaper-pad has the pad: send nothing
```

- **Actions:**
  - `key:<chord>`: held as long as the button.
  - `mouse:left|right|middle|back|forward`: held as long as the button.
  - `text:<string>`: type text, e.g. `/clear⏎`.
  - `layer:<name>`: while held, the layer's bindings override the profile's.
  - `switcher`: Alt+Tab with the D-pad.
  - `focus:<exe>`: bring an application's window to the front.
  - `run:<command>`: start a program.
  - `profile:cycle`: switch the profile by hand.
  - `pad:toggle`: pause or resume the pad.
- **Pasting images into Claude CLI and Devin CLI (decided 2026-11-22):**
  Claude CLI pastes an image with **Alt+V**, Devin CLI with
  **Shift+Insert**. They get a button each (5 and 6 in the default
  profile), and the user chooses which to press: no detection. Telling the
  two apart automatically is not reliable: both run inside
  WindowsTerminal.exe, usually in zellij panes, and the window title showed
  neither (see the measurement below). Together with a `key:win+shift+s`
  binding (screen snip), taking a screenshot and pasting it into the CLI is
  two button presses.
- **Profile matching:** the most specific profile wins (exe + title, then
  exe, then `default`). The title is checked again when it changes
  (`EVENT_OBJECT_NAMECHANGE`), so switching tabs in Windows Terminal switches
  the profile too.
- **Measured (2026-11-22, `desk-pad --test`):** a Windows Terminal tab
  with zellij running Claude CLI was titled `circular-elephant | C:\Program
  Files\PowerShell\7\pwsh.EXE`, without "claude" in it. In a later session
  the same tab was `erudite-aardvark | …`: the first part changes from
  session to session (it is probably the random zellij session name; the
  second part is the focused pane's title), so it can't be matched either.
  Window titles still serve to tell applications apart (browser, Teams).
- **`passthrough` for REAPER:** both programs read the same pad at once (SDL
  background events), so desk-pad must stay quiet while REAPER is in front.

## Stages

### Stage 0: `pad-core` in reaper-pad
- [x] Workspace with `crates/pad-core` (gamepad, input, logic).
- [x] Generic `Stepper<K>`; `InputEvent::ButtonUp`.
- [x] reaper-pad behaves as before: 50 unit tests, clippy, release build.
- [x] Manual check in REAPER ([manual-tests.md](https://github.com/MickCh/reaper-pad/blob/main/docs/manual-tests.md)),
  2026-11-22 on Windows 11: buttons, sticks, replugging (digital-mode
  warning after reconnecting, as the pad falls back to digital mode).
- [x] Merged into reaper-pad `main`, tagged `pad-core-v0.1.0`, pushed;
  desk-pad uses the git dependency.

### Stage 1: skeleton
- [x] Input loop at ~250 Hz, `Output` trait, `SendInput` output.
- [x] Key chords and mouse buttons, held as long as the button.
- [x] Built-in default layout (the `default` profile above, without layers).
- [x] Buttons 5 / 6 paste an image in Claude CLI / Devin CLI (2026-11-22).
- [x] `--test`: prints buttons, axes, D-pad, and the active exe and window
  title (to write `match` / `title` patterns).
- [x] Safe exit: keys released on Ctrl+C / console close / pause / unplug.
- [x] Manual check: D-pad switches tabs in the browser and Windows Terminal
  (2026-11-22).

### Stage 2: mouse and scroll
- [x] Left stick: mouse (radial dead zone, curve, sub-pixel steps).
- [x] Right stick: smooth vertical and horizontal scroll.
- [x] Manual check: the default pointer and scroll speeds are fine on the
  real pad (2026-11-22).
- [x] Manual check: selecting text in Windows Terminal with button 1 held.

### Stage 3: configuration
- [x] `desk-pad.toml` in `%APPDATA%\desk-pad\` (or `--config PATH`), the
  documented default (`config.toml`) embedded in the exe and written on
  first start (as in reaper-pad).
- [x] Validation with line numbers (`deny_unknown_fields`, unknown actions
  and key names listed, button numbers, value ranges).
- [x] Reload on save; on an error, the previous settings stay. Done by
  polling the file's time and size every 250 ms and reading it once it
  stopped changing, instead of a `notify` watcher: no dependency, and
  editors that save by replacing the file are handled the same way.
- [x] Format ready for stage 4: hardware (`[sticks.*]`) and speeds
  (`[mouse]`, `[scroll]`) are global, bindings live in
  `[profiles.default]`; other profiles are refused with a clear message
  until then.

### Stage 4: profiles and layers
- [x] The program in front, checked every 100 ms (`GetForegroundWindow`;
  polling instead of `SetWinEventHook`, which would need a message loop on
  the input thread). Window titles are not used for profiles yet.
- [x] Profiles with `match` and `passthrough`; their bindings override
  `[profiles.default]` (implicit inheritance). A switch waits until no pad
  button is held, so a drag or a held key ends where it started.
- [x] `[profiles.reaper]` with `passthrough` in the default configuration.
- [x] `"none"` unbinds an inherited button, D-pad direction or layer
  binding.
- [x] Layers (`layer:<name>`): while the button is held, the layer's
  bindings apply; what it doesn't bind falls through to the profile.
  `hold` keys go down at the first use of the layer's own bindings and up
  with its button; `tap` is sent if the layer was not used. That makes the
  window switcher a layer: button 9, `hold = "alt"`, Tab / Shift+Tab on
  the D-pad, `tap = "key:win+tab"` (so no separate `switcher` action).
  App profiles inherit the default's layers and can extend them.
- [x] `text:` (Unicode input, `\n` as Enter, independent of the keyboard
  layout) and `run:` (`ShellExecute` on a thread of its own,
  `%VARIABLES%` expanded).
- [x] Default `teams` profile (5: mute, 6: camera). Terminal and browser
  need no profile: the defaults (D-pad Ctrl+Tab, 7/8 Alt+←/→, which
  switch zellij tabs too) work in both.
- [x] Manual check: the switcher on 9 (tap: task view; held + D-pad: Alt+Tab).
- [ ] Manual check: Teams buttons 5/6 in a call.
- [ ] Later, if needed: `focus:` (with the Alt trick for
  `SetForegroundWindow`), `profile:cycle`, `title` (regex; zellij makes
  terminal titles useless for telling CLIs apart), sticks in layers.

### Stage 5: tray and UX
- [x] No console window (`windows_subsystem = "windows"`); log to
  `%APPDATA%\desk-pad\desk-pad.log`. `--test` and `--console` open a
  console of their own (or write to a pipe the caller set up).
- [x] Tray icon showing the state by colour: active (green), paused (grey),
  no pad (red), digital mode (amber), passthrough profile (blue); the
  tooltip names the state and profile.
- [x] Tray menu: pause, reload, open configuration, open log, start with
  Windows (`HKCU\...\Run`), quit. The button test stays on the command
  line (`--test`): in-process it would need a console that closing would
  kill desk-pad with.
- [x] Notices (config errors, digital mode, pause) as Windows
  notifications.
- [x] One instance per session (named mutex); `install.ps1` installs to
  `%LOCALAPPDATA%\Programs\desk-pad`.
- [x] Held keys are also released by the `SendInput` layer when it is
  dropped (normal exit, panic) and on logoff (`WM_ENDSESSION`).
- [ ] Manual check: the icon, its menu, notifications, start with Windows.
- [ ] Later, maybe: a small on-screen overlay on layer and profile
  changes; the current layer in the tooltip (stage 4).

### Stage 6: release
- [x] CI (windows-latest): fmt, clippy, tests; MSRV check.
- [x] Release workflow: a `v*` tag builds `desk-pad.exe` and a zip with
  the README, the default configuration and the license.
- [ ] README with the default profiles and troubleshooting.

## Known limits

- **UIPI:** `SendInput` can't reach windows of elevated processes (e.g.
  Windows Terminal run as administrator) unless desk-pad is elevated too.
  `uiAccess` would lift this but needs a signed exe installed in Program
  Files: not planned; desk-pad logs it when input is blocked.
- **Secure desktop** (UAC prompts, lock screen): no injected input there.
- **Teams shortcuts** (Ctrl+Shift+M etc.) only work while Teams has focus;
  `win+alt+k` mutes globally. Otherwise `focus:ms-teams.exe` first.
- **Pointer precision:** relative mouse moves go through Windows' "Enhance
  pointer precision", which bends the stick curve a bit.
