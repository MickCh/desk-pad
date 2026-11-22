# desk-pad

Use a cheap USB gamepad as keyboard and mouse on the Windows desktop: switch
tabs in Windows Terminal and the browser, move the pointer with one stick,
scroll with the other, switch windows, mute yourself in Teams.

A sibling of [reaper-pad](../reaper-pad), which turns the same pad into a
remote control for REAPER; both share the gamepad code in `pad-core`.

**Status:** early development (stages 1–5 of [the plan](docs/PLAN.md), with
per-application profiles in their first form). Layers and the window
switcher come next.

## Default layout

The layout below is the default configuration; see
[Configuration](#configuration) to change it.

Made for the **Art AG-16T** (a generic DragonRise twin-stick pad); button
numbers match its printed labels.

| Control | Function |
|---|---|
| **1** | Left mouse button (hold to drag) |
| **2** | Right mouse button |
| **3 / 4** | Esc / Enter |
| **5** | Paste an image in Claude CLI (Alt+V) |
| **6** | Paste an image in Devin CLI (Shift+Insert; elsewhere a normal paste) |
| **7 / 8** | Back / forward (Alt+← / Alt+→); previous / next tab in zellij |
| **9** | Held + D-pad: switch windows (Alt+Tab; release 9 to choose). Tapped: task view (Win+Tab) |
| **10** | Pause / resume the pad |
| **11** (left stick click) | Middle mouse button |
| **12** (right stick click) | Mute the microphone in calls (Win+Alt+K) |
| **D-pad ← →** | Previous / next tab (Ctrl+Shift+Tab / Ctrl+Tab) |
| **D-pad ↑ ↓** | Arrow up / down |
| **Left stick** | Mouse pointer (a slight push moves slowly) |
| **Right stick** | Scroll, vertical and horizontal |

In Microsoft Teams, **5** mutes / unmutes (Ctrl+Shift+M) and **6** turns
the camera on / off (Ctrl+Shift+O).

The D-pad repeats while held. Keys and mouse buttons stay down as long as
the pad's button. When the pad is paused or unplugged, or desk-pad stops,
everything it holds is released.

If buttons 1–4 fire repeatedly while held, the pad's **Turbo** is on for
them: press its Turbo button to switch it off. `desk-pad --test` shows how
long each button was held and how soon it was pressed again, which tells
Turbo (a steady series) from worn contacts (a few presses within ~100 ms).

If the right stick acts like buttons 1–4, the pad is in digital mode: press
its **Analog** button (LED red). desk-pad notices this on DragonRise pads and
ignores buttons 1–4 until then.

## Installation

From source (see [Building](#building)), in PowerShell:

```powershell
.\install.ps1      # builds and copies desk-pad.exe to %LOCALAPPDATA%\Programs\desk-pad
```

Start `desk-pad.exe`: an icon appears in the notification area (maybe
behind the ^ arrow). Its menu has **Start with Windows** to start it at
logon.

## Usage

desk-pad runs in the background. Its tray icon shows the state:

| Colour | State |
|---|---|
| green | active |
| grey | paused (button 10, or the menu) |
| red | waiting for a gamepad |
| amber | the pad is in digital mode: press its Analog button |
| blue | a passthrough profile is in front (REAPER): nothing is sent |

Click the icon for the menu: pause, reload or open the configuration, open
the log (`%APPDATA%\desk-pad\desk-pad.log`), start with Windows, quit.
Configuration errors and other things to act on show as Windows
notifications.

```
desk-pad            run in the tray
desk-pad --test     button test in a console: buttons (with timing), axes,
                    D-pad and the window in front; sends nothing
desk-pad --console  run in a console instead of the tray (stop with Ctrl+C)
desk-pad --seconds N
                    stop after N seconds
desk-pad --config PATH
                    use another configuration file
```

Only one desk-pad runs at a time. Quit it from the tray before a `--test`,
or it acts on the pad too.

In REAPER, desk-pad sends nothing (`[profiles.reaper]` with
`passthrough = true`): there [reaper-pad](../reaper-pad) has the pad.

## Configuration

On first start desk-pad writes the default configuration to
`%APPDATA%\desk-pad\desk-pad.toml`. Edit and save it: desk-pad reloads it
within about a second. If the file has an error, the console names the line
and the problem, and the previous settings stay active.

The [default `config.toml`](config.toml) documents every option. In short:

```toml
[pad]
debounce_ms = 120

[sticks.left]               # hardware: axes and dead zone
axis_x = 0
axis_y = 1
deadzone = 0.15

[mouse]                     # pixels per second at slight / full deflection
min_speed = 60
max_speed = 1600

[profiles.default.buttons]
1 = "mouse:left"
3 = "key:esc"
7 = "key:alt+v"             # e.g. paste an image in Claude CLI
10 = "pad:toggle"

[profiles.default.dpad]
right = "key:ctrl+tab"

[profiles.default.sticks]
left = "mouse"
right = "scroll"
```

**Actions:** `key:<keys>` (e.g. `key:ctrl+shift+tab`, `key:f5`; held as long
as the button), `mouse:left|right|middle|back|forward`, `pad:toggle`.
**Sticks:** `mouse`, `scroll`, `none`.

Windows does not let a normal program send input to windows of programs
running as administrator: run desk-pad as administrator too if you need that.

## Building

Requirements: Rust 1.88+, Visual Studio C++ build tools, CMake (SDL2 is
built from source and linked statically). [reaper-pad](../reaper-pad) must
be checked out next to this repository, on a branch with `crates/pad-core`:
desk-pad uses it by path for now.

```sh
cargo build --release      # target\release\desk-pad.exe
cargo test                 # unit tests (no gamepad needed)
cargo clippy --all-targets -- -D warnings
cargo fmt --all
```

## Development

```
src/main.rs        command line, console or tray
src/app.rs         the input loop, profiles following the window in front
src/logging.rs     log to the console and the log file
src/stop.rs        ending cleanly (keys released first)
src/router.rs      what the input means: keys, mouse, scroll, pause, test mode
src/config.rs      configuration file: parsing, validation, defaults
src/watch.rs       notices when the configuration file was saved
src/layout.rs      bindings and settings in effect
config.toml        documented default configuration (embedded into the exe)
src/action.rs      bindable actions (key:, mouse:, pad:toggle)
src/keys.rs        key names and chords → Windows virtual-key codes
src/win/           Win32: SendInput, the window in front, tray icon,
                   autostart, console, timer
```

`router.rs` is plain Rust with time as a parameter and is unit-tested with
a mock output; only `src/win/` calls the Win32 API, and only `pad-core`
calls SDL2.

## License

[MIT](LICENSE)
