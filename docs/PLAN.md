# desk-pad: implementation plan

Use a cheap USB gamepad (defaults made for the **Art AG-16T**, a 12-button
DragonRise twin-stick pad) as keyboard and mouse on the Windows desktop:
switch tabs in Windows Terminal and the browser, move the mouse with one
stick, scroll with the other, switch windows, control Teams. Windows only
for now.

Tick an item off (`[x]`) in the same commit that does it.

## Shared code: `pad-core`

The input code is shared with [reaper-pad](../../reaper-pad) through the
`pad-core` crate, a workspace member of the reaper-pad repository:

| `pad-core` module | What it gives desk-pad |
|---|---|
| `gamepad` | SDL2 device: first pad, hot-plug, button down/up, axes, hats, rumble, DragonRise detection |
| `input` | SDL-free `InputEvent`, `Device`, `Log`, `hat_name` |
| `logic` | `Debouncer`, `HatRepeat` (D-pad auto-repeat), `AxisParams` (stick curve), `Stepper<K>` (fractional → whole steps: pixels, wheel units), `ModeDetector` (DragonRise digital mode) |

Not shared (REAPER-specific, or only the pattern is reused): reaper-pad's
router, commands, config, guard and REAPER glue.

**Dependency:** for now a local path, `../reaper-pad/crates/pad-core`, so
reaper-pad must be checked out next to desk-pad, on a branch that has
`crates/pad-core` (currently the local branch `pad-core`). Once that is
merged and pushed, switch to a git dependency pinned to a tag:

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
├─ tray icon, Win32 message loop                                  (stage 5)
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
buttons.5  = "layer:nav"             # held L1 switches the layer        (stage 4)
buttons.6  = "switcher"              # held: Alt down, D-pad = Tab/Shift+Tab (stage 4)
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
[profiles.terminal.layers.nav]
dpad.left  = "key:alt+left"          # move focus between panes
dpad.right = "key:alt+right"

# Claude CLI and Devin CLI both run inside WindowsTerminal.exe: they are told
# apart by the window title (the title of the active tab).
[profiles.claude]
match = ["WindowsTerminal.exe"]
title = "(?i)claude"
inherit = "terminal"
buttons.7 = "paste_image"            # Alt+V in Claude CLI

[profiles.devin]
match = ["WindowsTerminal.exe"]
title = "(?i)devin"
inherit = "terminal"
buttons.7 = "paste_image"            # Shift+Insert in Devin CLI

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
- **`paste_image`:** Claude CLI pastes an image with **Alt+V**, Devin CLI with
  **Shift+Insert**. The binding is per profile (`paste_image = "key:alt+v"`).
  Together with a `key:win+shift+s` binding (screen snip), taking a
  screenshot and pasting it into the CLI is two button presses.
- **Profile matching:** the most specific profile wins (exe + title, then
  exe, then `default`). The title is checked again when it changes
  (`EVENT_OBJECT_NAMECHANGE`), so switching tabs in Windows Terminal switches
  the profile too.
- **Measured (2026-11-22, `desk-pad --test`):** a Windows Terminal tab
  running Claude CLI was titled `circular-elephant | C:\Program
  Files\PowerShell\7\pwsh.EXE`, without "claude" in it. So `(?i)claude`
  alone is not enough: check the real titles with `--test` before choosing
  the patterns, and expect to need the fallback below. In a later session
  the same tab was `erudite-aardvark | …`: the first part changes from
  session to session, so it can't be matched either.
- **Fallback when a CLI doesn't set a useful title:** a fixed tab title in the
  Windows Terminal profile (`"tabTitle"` + `"suppressApplicationTitle": true`),
  or `profile:cycle` on a button.
- **`passthrough` for REAPER:** both programs read the same pad at once (SDL
  background events), so desk-pad must stay quiet while REAPER is in front.

## Stages

### Stage 0: `pad-core` in reaper-pad
- [x] Workspace with `crates/pad-core` (gamepad, input, logic).
- [x] Generic `Stepper<K>`; `InputEvent::ButtonUp`.
- [x] reaper-pad behaves as before: 50 unit tests, clippy, release build.
- [ ] Manual check in REAPER ([manual-tests.md](../../reaper-pad/docs/manual-tests.md)).
- [ ] Merge into reaper-pad `main`, tag `pad-core-v0.1.0`, push; switch
  desk-pad to the git dependency.

### Stage 1: skeleton
- [x] Input loop at ~250 Hz, `Output` trait, `SendInput` output.
- [x] Key chords and mouse buttons, held as long as the button.
- [x] Built-in default layout (the `default` profile above, without layers).
- [x] `--test`: prints buttons, axes, D-pad, and the active exe and window
  title (to write `match` / `title` patterns).
- [x] Safe exit: keys released on Ctrl+C / console close / pause / unplug.
- [ ] Manual check: D-pad switches tabs in Chrome/Edge and Windows Terminal.

### Stage 2: mouse and scroll
- [x] Left stick: mouse (radial dead zone, curve, sub-pixel steps).
- [x] Right stick: smooth vertical and horizontal scroll.
- [ ] Manual check: selecting text in Windows Terminal with button 1 held;
  tuning the default speeds on the real pad.

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
- [ ] Active exe and window title (`SetWinEventHook`: foreground and name
  changes).
- [ ] Profiles with `match`, `title` (regex), `inherit`, `passthrough`.
- [ ] Layers (`layer:`), `switcher`, `paste_image`, `profile:cycle`.
- [ ] `text:`, `focus:` (with the Alt trick for `SetForegroundWindow`), `run:`.
- [ ] Default profiles: terminal, claude, devin, browser, teams, reaper.

### Stage 5: tray and UX
- [ ] No console window (`windows_subsystem = "windows"`); log to a file.
- [ ] Tray icon showing the state: active / paused / no pad / digital mode,
  and the current profile and layer in the tooltip.
- [ ] Tray menu: pause, reload, button test, open config, start with
  Windows, quit.
- [ ] Feedback without rumble (DragonRise pads can't vibrate on Windows):
  the tray icon, later maybe a small on-screen overlay on layer and profile
  changes.

### Stage 6: release
- [ ] CI (windows-latest): fmt, clippy, tests.
- [ ] Release `desk-pad.exe` from a `v*` tag.
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
