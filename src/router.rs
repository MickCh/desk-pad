//! What the pad's input means on the desktop: turns pad-core's
//! [`InputEvent`]s and axis readings into keys, mouse buttons, pointer
//! movement and scrolling. Plain Rust with time as a parameter, so it is
//! unit-tested with a mock [`Output`]; `main` feeds it from SDL and sends
//! its output through `SendInput`.

use crate::action::{Action, MouseButton};
use crate::keys::{self, Vk};
use crate::layout::{Layout, Stick, StickRole};
use pad_core::input::{Device, InputEvent, Log, hat_name};
use pad_core::logic::{Debouncer, Direction, Directions, HatRepeat, ModeDetector, Stepper};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Where the router's keys and mouse events go.
pub trait Output: Log {
    fn key(&mut self, vk: Vk, down: bool);
    fn mouse_button(&mut self, button: MouseButton, down: bool);
    /// Relative pointer movement in pixels (+x right, +y down).
    fn mouse_move(&mut self, dx: i32, dy: i32);
    /// Wheel movement in wheel units (120 = one notch); `vertical` > 0
    /// scrolls up, `horizontal` > 0 scrolls right.
    fn wheel(&mut self, vertical: i32, horizontal: i32);
}

/// Wheel units per notch (`WHEEL_DELTA`).
const WHEEL_DELTA: f32 = 120.0;

/// Raw axis values beyond this are reported in test mode.
const TEST_AXIS_THRESHOLD: u16 = 16000;

/// Keys and mouse buttons the router holds down, reference-counted: two
/// bindings sharing a key (e.g. `ctrl`) release it only when both are up.
#[derive(Debug, Default)]
struct Held {
    keys: HashMap<Vk, u32>,
    mouse: HashMap<MouseButton, u32>,
}

impl Held {
    fn key(&mut self, vk: Vk, down: bool, out: &mut dyn Output) {
        if let Some(edge) = count(&mut self.keys, vk, down) {
            out.key(vk, edge);
        }
    }

    fn mouse(&mut self, b: MouseButton, down: bool, out: &mut dyn Output) {
        if let Some(edge) = count(&mut self.mouse, b, down) {
            out.mouse_button(b, edge);
        }
    }

    /// Release everything: mouse buttons, then keys, modifiers last.
    fn release_all(&mut self, out: &mut dyn Output) {
        for (b, _) in self.mouse.drain() {
            out.mouse_button(b, false);
        }
        let mut keys: Vec<Vk> = self.keys.drain().map(|(k, _)| k).collect();
        keys.sort_by_key(|&k| keys::is_modifier(k));
        for k in keys {
            out.key(k, false);
        }
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.keys.is_empty() && self.mouse.is_empty()
    }
}

/// Count a press or release of `k`. Returns the edge to send: `Some(true)`
/// for the first press, `Some(false)` for the last release.
fn count<K: std::hash::Hash + Eq + Copy>(
    map: &mut HashMap<K, u32>,
    k: K,
    down: bool,
) -> Option<bool> {
    if down {
        let n = map.entry(k).or_insert(0);
        *n += 1;
        (*n == 1).then_some(true)
    } else {
        let n = map.get_mut(&k)?;
        *n -= 1;
        (*n == 0).then(|| {
            map.remove(&k);
            false
        })
    }
}

pub struct Router {
    layout: Layout,
    /// Only report button/axis numbers, send nothing.
    test_mode: bool,
    /// `pad:toggle`: everything but the toggle itself is ignored.
    paused: bool,
    device: Option<Device>,
    debouncer: Debouncer,
    hat: HatRepeat,
    mode: ModeDetector,
    /// Buttons whose press was acted on, with what to release.
    pressed: HashMap<u32, Action>,
    /// Released buttons, released for real at the given time unless they
    /// bounce back first (a worn contact must not end a drag).
    releasing: HashMap<u32, Instant>,
    held: Held,
    /// Fractional pixels and wheel units, by (stick, axis).
    steps: Stepper<(u8, u8)>,
    last_tick: Instant,
}

impl Router {
    pub fn new(layout: Layout, now: Instant) -> Self {
        Router {
            debouncer: Debouncer::new(Duration::from_millis(layout.debounce_ms)),
            hat: HatRepeat::new(Duration::from_millis(layout.repeat_ms)),
            layout,
            test_mode: false,
            paused: false,
            device: None,
            mode: ModeDetector::default(),
            pressed: HashMap::new(),
            releasing: HashMap::new(),
            held: Held::default(),
            steps: Stepper::default(),
            last_tick: now,
        }
    }

    pub fn set_test_mode(&mut self, on: bool, out: &mut dyn Output) {
        if on {
            self.release_all(out);
        }
        self.test_mode = on;
    }

    pub fn connected(&mut self, device: Device) {
        self.device = Some(device);
    }

    /// The pad was unplugged: release everything and forget its state.
    pub fn disconnected(&mut self, now: Instant, out: &mut dyn Output) {
        self.device = None;
        self.hat.set((0, 0), now);
        self.mode.reset();
        self.release_all(out);
    }

    /// Release every key and mouse button held for the pad.
    pub fn release_all(&mut self, out: &mut dyn Output) {
        self.pressed.clear();
        self.releasing.clear();
        self.held.release_all(out);
    }

    pub fn event(&mut self, ev: InputEvent, now: Instant, out: &mut dyn Output) {
        if self.test_mode {
            test_report(ev, out);
            return;
        }
        match ev {
            InputEvent::Button(idx) => self.button_down(u32::from(idx) + 1, now, out),
            InputEvent::ButtonUp(idx) => self.button_up(u32::from(idx) + 1, now, out),
            InputEvent::Hat(xy) => {
                let dirs = self.hat.set(xy, now);
                self.dpad(dirs, out);
            }
            // Sticks are read in `tick`.
            InputEvent::Axis(..) => {}
        }
    }

    /// Everything that runs on time rather than on events: digital-mode
    /// detection, delayed releases, D-pad repeat and the sticks. Call once
    /// per poll, after the events; `read_axis` gives the current raw value
    /// of a hardware axis of the connected pad.
    pub fn tick(
        &mut self,
        now: Instant,
        read_axis: impl Fn(u32) -> Option<i16>,
        out: &mut dyn Output,
    ) {
        self.check_mode(now, &read_axis, out);
        let dt = now.duration_since(self.last_tick).as_secs_f32().min(0.1);
        self.last_tick = now;
        self.finish_releases(now, out);
        if self.test_mode || self.paused || self.device.is_none() {
            return;
        }
        let dirs = self.hat.tick(now);
        self.dpad(dirs, out);
        for (i, stick) in [(0, &self.layout.left), (1, &self.layout.right)] {
            let x = axis_value(&read_axis, stick.axis_x);
            let y = axis_value(&read_axis, stick.axis_y);
            match stick.role {
                Some(StickRole::Mouse) => mouse(&mut self.steps, i, stick, x, y, dt, out),
                Some(StickRole::Scroll) => scroll(&mut self.steps, i, stick, x, y, dt, out),
                None => {}
            }
        }
    }

    fn button_down(&mut self, num: u32, now: Instant, out: &mut dyn Output) {
        let accepted = self.debouncer.accept(num, now);
        if self.releasing.remove(&num).is_some() {
            return; // bounced back while held: still held
        }
        if !accepted {
            return;
        }
        if self.mode.digital() && (1..=4).contains(&num) {
            out.log(&format!(
                "[{num}] ignored — digital mode (press Analog, LED red)"
            ));
            return;
        }
        let Some(action) = self.layout.buttons.get(&num).cloned() else {
            out.log(&format!("[{num}] (not assigned)"));
            return;
        };
        if action == Action::PadToggle {
            self.set_paused(!self.paused, out);
            return;
        }
        if self.paused {
            return;
        }
        out.log(&format!("[{num}] {action}"));
        if let Some(old) = self.pressed.insert(num, action.clone()) {
            self.release(&old, out); // a lost release event
        }
        self.press(&action, out);
    }

    fn button_up(&mut self, num: u32, now: Instant, out: &mut dyn Output) {
        if !self.pressed.contains_key(&num) {
            return;
        }
        let window = Duration::from_millis(self.layout.debounce_ms);
        if window.is_zero() {
            self.finish_release(num, out);
        } else {
            self.releasing.insert(num, now + window);
        }
    }

    fn finish_releases(&mut self, now: Instant, out: &mut dyn Output) {
        let due: Vec<u32> = self
            .releasing
            .iter()
            .filter(|&(_, &at)| at <= now)
            .map(|(&n, _)| n)
            .collect();
        for num in due {
            self.releasing.remove(&num);
            self.finish_release(num, out);
        }
    }

    fn finish_release(&mut self, num: u32, out: &mut dyn Output) {
        if let Some(action) = self.pressed.remove(&num) {
            self.release(&action, out);
        }
    }

    fn set_paused(&mut self, paused: bool, out: &mut dyn Output) {
        self.paused = paused;
        if paused {
            self.release_all(out);
            out.notice("Paused — press the pad:toggle button again to resume.");
        } else {
            out.notice("Resumed.");
        }
    }

    fn dpad(&mut self, dirs: Directions, out: &mut dyn Output) {
        if self.paused {
            return;
        }
        for dir in dirs {
            let d = &self.layout.dpad;
            let action = match dir {
                Direction::Up => &d.up,
                Direction::Down => &d.down,
                Direction::Left => &d.left,
                Direction::Right => &d.right,
            };
            match action.clone() {
                Some(Action::PadToggle) => self.set_paused(true, out),
                Some(a) => {
                    self.press(&a, out);
                    self.release(&a, out);
                }
                None => {}
            }
        }
    }

    fn press(&mut self, action: &Action, out: &mut dyn Output) {
        match action {
            Action::Key(chord) => {
                for k in chord.keys() {
                    self.held.key(k, true, out);
                }
            }
            Action::Mouse(b) => self.held.mouse(*b, true, out),
            Action::PadToggle => {}
        }
    }

    fn release(&mut self, action: &Action, out: &mut dyn Output) {
        match action {
            Action::Key(chord) => {
                for k in chord.keys().rev() {
                    self.held.key(k, false, out);
                }
            }
            Action::Mouse(b) => self.held.mouse(*b, false, out),
            Action::PadToggle => {}
        }
    }

    /// Watch for the digital mode of DragonRise pads (see `ModeDetector`).
    fn check_mode(
        &mut self,
        now: Instant,
        read_axis: &impl Fn(u32) -> Option<i16>,
        log: &mut dyn Output,
    ) {
        if !self.layout.detect_digital_mode || !self.device.is_some_and(|d| d.dragonrise) {
            return;
        }
        let x = read_axis(self.layout.right.axis_x).unwrap_or(0);
        let y = read_axis(self.layout.right.axis_y).unwrap_or(0);
        match self.mode.update(x, y, now) {
            Some(true) => log.notice(
                "Gamepad is in DIGITAL mode (LED green): the right stick would press \
                 buttons 1–4, so they are ignored. Press the Analog button (LED red).",
            ),
            Some(false) => log.notice("Gamepad is in analog mode — buttons 1–4 work again."),
            None => {}
        }
    }
}

fn test_report(ev: InputEvent, out: &mut dyn Output) {
    match ev {
        InputEvent::Button(idx) => out.log(&format!("button {}", u32::from(idx) + 1)),
        InputEvent::ButtonUp(idx) => out.log(&format!("button {} released", u32::from(idx) + 1)),
        InputEvent::Axis(idx, value) => {
            if value.unsigned_abs() > TEST_AXIS_THRESHOLD {
                out.log(&format!("axis {idx}: {value}"));
            }
        }
        InputEvent::Hat(xy) => out.log(&format!("D-pad {}", hat_name(xy))),
    }
}

/// Normalized axis value, -1..1.
fn axis_value(read_axis: &impl Fn(u32) -> Option<i16>, idx: u32) -> f32 {
    read_axis(idx).map_or(0.0, |r| (f32::from(r) / 32767.0).clamp(-1.0, 1.0))
}

/// Pointer movement: the dead zone and curve apply to the deflection as a
/// whole (radial), so diagonal moves are as smooth as straight ones.
fn mouse(
    steps: &mut Stepper<(u8, u8)>,
    i: u8,
    stick: &Stick,
    x: f32,
    y: f32,
    dt: f32,
    out: &mut dyn Output,
) {
    let mag = x.hypot(y).min(1.0);
    let speed = stick.params().rate(mag);
    if speed == 0.0 {
        return;
    }
    let sy = if stick.invert_y { -1.0 } else { 1.0 };
    let dx = steps.steps((i, 0), x / mag * speed * dt);
    let dy = steps.steps((i, 1), sy * y / mag * speed * dt);
    if dx != 0 || dy != 0 {
        out.mouse_move(dx, dy);
    }
}

/// Scrolling, each axis on its own; pushing the stick up scrolls up.
fn scroll(
    steps: &mut Stepper<(u8, u8)>,
    i: u8,
    stick: &Stick,
    x: f32,
    y: f32,
    dt: f32,
    out: &mut dyn Output,
) {
    let p = stick.params();
    let sy = if stick.invert_y { 1.0 } else { -1.0 };
    let v = steps.steps((i, 1), sy * p.rate(y) * WHEEL_DELTA * dt);
    let h = steps.steps((i, 0), p.rate(x) * WHEEL_DELTA * dt);
    if v != 0 || h != 0 {
        out.wheel(v, h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    enum Out {
        Key(&'static str, bool),
        Mouse(MouseButton, bool),
        Move(i32, i32),
        Wheel(i32, i32),
        Log(String),
    }

    #[derive(Default)]
    struct TestOut(Vec<Out>);

    impl TestOut {
        /// Everything but log messages.
        fn sent(&self) -> Vec<Out> {
            self.0
                .iter()
                .filter(|o| !matches!(o, Out::Log(_)))
                .cloned()
                .collect()
        }

        fn logs(&self) -> Vec<&str> {
            self.0
                .iter()
                .filter_map(|o| match o {
                    Out::Log(m) => Some(m.as_str()),
                    _ => None,
                })
                .collect()
        }

        fn moved(&self) -> (i32, i32) {
            self.0.iter().fold((0, 0), |(x, y), o| match o {
                Out::Move(dx, dy) => (x + dx, y + dy),
                _ => (x, y),
            })
        }

        fn scrolled(&self) -> (i32, i32) {
            self.0.iter().fold((0, 0), |(v, h), o| match o {
                Out::Wheel(dv, dh) => (v + dv, h + dh),
                _ => (v, h),
            })
        }
    }

    /// Key names leaked as `'static` keep the expected values short.
    fn k(name: &str) -> &'static str {
        Box::leak(name.to_string().into_boxed_str())
    }

    impl Log for TestOut {
        fn log(&mut self, msg: &str) {
            self.0.push(Out::Log(msg.into()));
        }
    }

    impl Output for TestOut {
        fn key(&mut self, vk: Vk, down: bool) {
            self.0.push(Out::Key(k(&keys::name(vk)), down));
        }
        fn mouse_button(&mut self, button: MouseButton, down: bool) {
            self.0.push(Out::Mouse(button, down));
        }
        fn mouse_move(&mut self, dx: i32, dy: i32) {
            self.0.push(Out::Move(dx, dy));
        }
        fn wheel(&mut self, vertical: i32, horizontal: i32) {
            self.0.push(Out::Wheel(vertical, horizontal));
        }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn layout() -> Layout {
        let mut l = Layout::default();
        l.buttons.insert(5, "key:ctrl+shift+tab".parse().unwrap());
        l.buttons.insert(6, "key:ctrl+tab".parse().unwrap());
        l
    }

    fn router(t0: Instant) -> Router {
        let mut r = Router::new(layout(), t0);
        r.connected(Device { dragonrise: false });
        r
    }

    fn no_axes(_: u32) -> Option<i16> {
        None
    }

    use InputEvent::{Button, ButtonUp, Hat};

    #[test]
    fn chord_is_held_with_the_button() {
        let t0 = Instant::now();
        let mut r = router(t0);
        let mut out = TestOut::default();
        r.event(Button(4), t0, &mut out); // button 5
        assert_eq!(
            out.sent(),
            vec![
                Out::Key("ctrl", true),
                Out::Key("shift", true),
                Out::Key("tab", true)
            ]
        );
        r.event(ButtonUp(4), t0 + ms(500), &mut out);
        r.tick(t0 + ms(500), no_axes, &mut out);
        assert_eq!(out.sent().len(), 3, "release waits for the debounce window");
        r.tick(t0 + ms(620), no_axes, &mut out);
        assert_eq!(
            out.sent()[3..],
            [
                Out::Key("tab", false),
                Out::Key("shift", false),
                Out::Key("ctrl", false)
            ]
        );
        assert!(r.held.is_empty());
    }

    #[test]
    fn shared_modifier_is_released_last() {
        let t0 = Instant::now();
        let mut r = router(t0);
        let mut out = TestOut::default();
        r.event(Button(4), t0, &mut out); // ctrl+shift+tab
        r.event(Button(5), t0, &mut out); // ctrl+tab: ctrl already down
        r.event(ButtonUp(4), t0 + ms(10), &mut out);
        r.tick(t0 + ms(200), no_axes, &mut out);
        let sent = out.sent();
        assert_eq!(
            sent.iter()
                .filter(|o| **o == Out::Key("ctrl", true))
                .count(),
            1
        );
        assert!(
            !sent.contains(&Out::Key("ctrl", false)),
            "ctrl+tab still held"
        );
        r.event(ButtonUp(5), t0 + ms(300), &mut out);
        r.tick(t0 + ms(500), no_axes, &mut out);
        assert_eq!(out.sent().last(), Some(&Out::Key("ctrl", false)));
        assert!(r.held.is_empty());
    }

    #[test]
    fn contact_bounce_neither_repeats_nor_ends_a_drag() {
        let t0 = Instant::now();
        let mut r = router(t0);
        let mut out = TestOut::default();
        r.event(Button(0), t0, &mut out); // mouse:left
        r.event(ButtonUp(0), t0 + ms(20), &mut out); // bounce
        r.event(Button(0), t0 + ms(40), &mut out);
        r.tick(t0 + ms(400), no_axes, &mut out);
        assert_eq!(
            out.sent(),
            vec![Out::Mouse(MouseButton::Left, true)],
            "still dragging"
        );
        r.event(ButtonUp(0), t0 + ms(900), &mut out);
        r.tick(t0 + ms(1100), no_axes, &mut out);
        assert_eq!(
            out.sent(),
            vec![
                Out::Mouse(MouseButton::Left, true),
                Out::Mouse(MouseButton::Left, false)
            ]
        );
    }

    #[test]
    fn dpad_taps_and_repeats() {
        let t0 = Instant::now();
        let mut r = router(t0);
        let mut out = TestOut::default();
        r.event(Hat((1, 0)), t0, &mut out);
        let tap = vec![
            Out::Key("ctrl", true),
            Out::Key("tab", true),
            Out::Key("tab", false),
            Out::Key("ctrl", false),
        ];
        assert_eq!(out.sent(), tap);
        r.tick(t0 + ms(300), no_axes, &mut out);
        assert_eq!(out.sent().len(), 8, "repeats while held");
        r.event(Hat((0, 0)), t0 + ms(310), &mut out);
        r.tick(t0 + ms(1000), no_axes, &mut out);
        assert_eq!(out.sent().len(), 8);
    }

    #[test]
    fn pause_releases_everything_and_ignores_input() {
        let t0 = Instant::now();
        let mut r = router(t0);
        let mut out = TestOut::default();
        r.event(Button(0), t0, &mut out); // mouse:left held
        r.event(Button(9), t0 + ms(10), &mut out); // pad:toggle
        assert_eq!(
            out.sent().last(),
            Some(&Out::Mouse(MouseButton::Left, false))
        );
        let n = out.sent().len();
        r.event(Button(2), t0 + ms(200), &mut out);
        r.event(Hat((1, 0)), t0 + ms(200), &mut out);
        r.tick(t0 + ms(300), |_| Some(32767), &mut out);
        assert_eq!(out.sent().len(), n, "paused");
        r.event(Button(9), t0 + ms(400), &mut out);
        r.event(Button(2), t0 + ms(600), &mut out);
        assert_eq!(out.sent().last(), Some(&Out::Key("esc", true)));
    }

    #[test]
    fn unplugging_releases_held_keys() {
        let t0 = Instant::now();
        let mut r = router(t0);
        let mut out = TestOut::default();
        r.event(Button(4), t0, &mut out);
        r.disconnected(t0 + ms(10), &mut out);
        assert!(out.sent().contains(&Out::Key("ctrl", false)));
        assert!(r.held.is_empty());
        r.event(ButtonUp(4), t0 + ms(20), &mut out);
        r.tick(t0 + ms(500), no_axes, &mut out);
        assert_eq!(out.sent().len(), 6, "nothing released twice");
    }

    #[test]
    fn test_mode_only_reports() {
        let t0 = Instant::now();
        let mut r = router(t0);
        let mut out = TestOut::default();
        r.set_test_mode(true, &mut out);
        r.event(Button(0), t0, &mut out);
        r.event(ButtonUp(0), t0, &mut out);
        r.event(Hat((-1, 1)), t0, &mut out);
        r.event(InputEvent::Axis(3, -30000), t0, &mut out);
        r.tick(t0 + ms(100), |_| Some(32767), &mut out);
        assert!(out.sent().is_empty());
        assert_eq!(
            out.logs(),
            [
                "button 1",
                "button 1 released",
                "D-pad left-up",
                "axis 3: -30000"
            ]
        );
    }

    #[test]
    fn digital_mode_ignores_buttons_1_to_4() {
        let t0 = Instant::now();
        let mut r = Router::new(layout(), t0);
        r.connected(Device { dragonrise: true });
        let mut out = TestOut::default();
        let frozen = |_| Some(-129);
        r.tick(t0, frozen, &mut out);
        r.tick(t0 + ms(600), frozen, &mut out);
        r.event(Button(0), t0 + ms(700), &mut out);
        r.event(Button(4), t0 + ms(700), &mut out);
        assert_eq!(
            out.sent()[0],
            Out::Key("ctrl", true),
            "button 1 ignored, 5 works"
        );
    }

    #[test]
    fn stick_moves_the_mouse() {
        let t0 = Instant::now();
        let mut r = router(t0);
        let mut out = TestOut::default();
        // Left stick (axes 0, 1) fully right for one second, in 4 ms ticks.
        let right = |a| (a == 0).then_some(32767);
        for i in 1..=250 {
            r.tick(t0 + ms(i * 4), right, &mut out);
        }
        let (x, y) = out.moved();
        assert!((1590..=1600).contains(&x), "max speed 1600 px/s, got {x}");
        assert_eq!(y, 0);
    }

    #[test]
    fn small_deflection_moves_nothing() {
        let t0 = Instant::now();
        let mut r = router(t0);
        let mut out = TestOut::default();
        let drift = |a| match a {
            0 | 1 => Some(3000), // ~9 % on both axes: radially ~13 %, below 15 %
            _ => None,
        };
        for i in 1..=100 {
            r.tick(t0 + ms(i * 4), drift, &mut out);
        }
        assert!(out.sent().is_empty());
    }

    #[test]
    fn stick_up_scrolls_up() {
        let t0 = Instant::now();
        let mut r = router(t0);
        let mut out = TestOut::default();
        // Right stick (axes 3, 4) fully up for one second.
        let up = |a| (a == 4).then_some(-32767);
        for i in 1..=250 {
            r.tick(t0 + ms(i * 4), up, &mut out);
        }
        let (v, h) = out.scrolled();
        assert!(
            (2990..=3000).contains(&v),
            "25 notches/s = 3000 units/s, got {v}"
        );
        assert_eq!(h, 0);
    }

    #[test]
    fn no_movement_without_a_pad() {
        let t0 = Instant::now();
        let mut r = Router::new(layout(), t0);
        let mut out = TestOut::default();
        r.tick(t0 + ms(100), |_| Some(32767), &mut out);
        assert!(out.sent().is_empty());
    }
}
