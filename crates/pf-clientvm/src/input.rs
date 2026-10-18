//! Input for client script: the button and axis names, button/stick
//! callbacks, `InputIsButtonDown`/`InputGetAxis`, and console-command
//! callbacks. Ported from client.dll's input group (registered by
//! 0x1802c0eb0; docs/port/scripts.md, "Input").

use std::cell::RefCell;
use std::rc::Rc;

use pf_squirrel::{NativeError, Value, Vm, throw};

use crate::ClientVm;

/// The buttons scripts can name: (the engine's button code, name). A
/// script's button ID is the index in this list (client.dll 0x180b0c300,
/// 137 entries of {code, name[32]}).
pub const BUTTONS: [(i32, &str); 137] = [
    (1, "KEY_0"),
    (2, "KEY_1"),
    (3, "KEY_2"),
    (4, "KEY_3"),
    (5, "KEY_4"),
    (6, "KEY_5"),
    (7, "KEY_6"),
    (8, "KEY_7"),
    (9, "KEY_8"),
    (10, "KEY_9"),
    (11, "KEY_A"),
    (12, "KEY_B"),
    (13, "KEY_C"),
    (14, "KEY_D"),
    (15, "KEY_E"),
    (16, "KEY_F"),
    (17, "KEY_G"),
    (18, "KEY_H"),
    (19, "KEY_I"),
    (20, "KEY_J"),
    (21, "KEY_K"),
    (22, "KEY_L"),
    (23, "KEY_M"),
    (24, "KEY_N"),
    (25, "KEY_O"),
    (26, "KEY_P"),
    (27, "KEY_Q"),
    (28, "KEY_R"),
    (29, "KEY_S"),
    (30, "KEY_T"),
    (31, "KEY_U"),
    (32, "KEY_V"),
    (33, "KEY_W"),
    (34, "KEY_X"),
    (35, "KEY_Y"),
    (36, "KEY_Z"),
    (37, "KEY_PAD_0"),
    (38, "KEY_PAD_1"),
    (39, "KEY_PAD_2"),
    (40, "KEY_PAD_3"),
    (41, "KEY_PAD_4"),
    (42, "KEY_PAD_5"),
    (43, "KEY_PAD_6"),
    (44, "KEY_PAD_7"),
    (45, "KEY_PAD_8"),
    (46, "KEY_PAD_9"),
    (47, "KEY_PAD_DIVIDE"),
    (48, "KEY_PAD_MULTIPLY"),
    (49, "KEY_PAD_MINUS"),
    (50, "KEY_PAD_PLUS"),
    (51, "KEY_PAD_ENTER"),
    (52, "KEY_PAD_DECIMAL"),
    (53, "KEY_LBRACKET"),
    (54, "KEY_RBRACKET"),
    (55, "KEY_SEMICOLON"),
    (56, "KEY_APOSTROPHE"),
    (57, "KEY_BACKQUOTE"),
    (58, "KEY_COMMA"),
    (59, "KEY_PERIOD"),
    (60, "KEY_SLASH"),
    (61, "KEY_BACKSLASH"),
    (62, "KEY_MINUS"),
    (63, "KEY_EQUAL"),
    (64, "KEY_ENTER"),
    (65, "KEY_SPACE"),
    (66, "KEY_BACKSPACE"),
    (67, "KEY_TAB"),
    (68, "KEY_CAPSLOCK"),
    (69, "KEY_NUMLOCK"),
    (70, "KEY_ESCAPE"),
    (71, "KEY_SCROLLLOCK"),
    (72, "KEY_INSERT"),
    (73, "KEY_DELETE"),
    (74, "KEY_HOME"),
    (75, "KEY_END"),
    (76, "KEY_PAGEUP"),
    (77, "KEY_PAGEDOWN"),
    (78, "KEY_BREAK"),
    (79, "KEY_LSHIFT"),
    (80, "KEY_RSHIFT"),
    (81, "KEY_LALT"),
    (82, "KEY_RALT"),
    (83, "KEY_LCONTROL"),
    (84, "KEY_RCONTROL"),
    (85, "KEY_LWIN"),
    (86, "KEY_RWIN"),
    (87, "KEY_APP"),
    (88, "KEY_UP"),
    (89, "KEY_LEFT"),
    (90, "KEY_DOWN"),
    (91, "KEY_RIGHT"),
    (92, "KEY_F1"),
    (93, "KEY_F2"),
    (94, "KEY_F3"),
    (95, "KEY_F4"),
    (96, "KEY_F5"),
    (97, "KEY_F6"),
    (98, "KEY_F7"),
    (99, "KEY_F8"),
    (100, "KEY_F9"),
    (101, "KEY_F10"),
    (102, "KEY_F11"),
    (103, "KEY_F12"),
    (104, "KEY_CAPSLOCKTOGGLE"),
    (105, "KEY_NUMLOCKTOGGLE"),
    (106, "KEY_SCROLLLOCKTOGGLE"),
    (107, "MOUSE_LEFT"),
    (108, "MOUSE_RIGHT"),
    (109, "MOUSE_MIDDLE"),
    (110, "MOUSE_4"),
    (111, "MOUSE_5"),
    (112, "MOUSE_WHEEL_UP"),
    (113, "MOUSE_WHEEL_DOWN"),
    (242, "BUTTON_DPAD_UP"),
    (243, "BUTTON_DPAD_RIGHT"),
    (244, "BUTTON_DPAD_DOWN"),
    (245, "BUTTON_DPAD_LEFT"),
    (114, "BUTTON_A"),
    (115, "BUTTON_B"),
    (116, "BUTTON_X"),
    (117, "BUTTON_Y"),
    (118, "BUTTON_SHOULDER_LEFT"),
    (119, "BUTTON_SHOULDER_RIGHT"),
    (120, "BUTTON_BACK"),
    (121, "BUTTON_START"),
    (122, "BUTTON_STICK_LEFT"),
    (123, "BUTTON_STICK_RIGHT"),
    (258, "STICK1_RIGHT"),
    (259, "STICK1_LEFT"),
    (260, "STICK1_DOWN"),
    (261, "STICK1_UP"),
    (262, "BUTTON_TRIGGER_LEFT"),
    (263, "BUTTON_TRIGGER_RIGHT"),
    (264, "STICK2_RIGHT"),
    (265, "STICK2_LEFT"),
    (266, "STICK2_DOWN"),
    (267, "STICK2_UP"),
];

/// The axes: (the engine's axis number, name); the script's axis ID is
/// the index (client.dll 0x180b0d650).
pub const AXES: [(i32, &str); 4] = [
    (0, "ANALOG_LEFT_X"),
    (1, "ANALOG_LEFT_Y"),
    (4, "ANALOG_RIGHT_X"),
    (3, "ANALOG_RIGHT_Y"),
];

/// Button codes from here up to `JOYSTICK_LAST` are controller buttons.
const JOYSTICK_FIRST: i32 = 0x72;
const JOYSTICK_LAST: i32 = 0x72 + 0xbf;

/// Each button, each stick, keeps 8 callbacks.
const SLOTS: usize = 8;
/// Console-command callbacks in all, and triggers kept per frame.
pub const MAX_CONCOMMAND_CALLBACKS: usize = 0x1e;
const MAX_TRIGGERS_PER_FRAME: usize = 0xf;

/// A script callback and the `this` it was registered with.
#[derive(Clone)]
pub struct Callback {
    pub func: Value,
    pub this: Value,
    /// Registered with a `RegisterMenu...` function.
    pub menu: bool,
}

type Slots = [Option<Callback>; SLOTS];

pub struct InputState {
    /// `SetInMenuForButtonCallbacks`: only callbacks with the same menu
    /// flag run.
    pub in_menu: bool,
    pressed: Vec<Slots>,
    released: Vec<Slots>,
    sticks: [Slots; 4],
    last_down: [bool; 137],
    last_axis: [f32; 4],
    /// When each button was last pressed (ms), so a tap shorter than a
    /// frame still counts.
    pressed_at: [u32; 137],
    last_frame_ms: u32,
    /// Console commands and their callbacks, newest first.
    concommands: Vec<(String, Vec<Callback>)>,
    /// Commands that ran since the last frame.
    triggered: Vec<String>,
}

impl Default for InputState {
    fn default() -> Self {
        InputState {
            in_menu: false,
            pressed: vec![Default::default(); 137],
            released: vec![Default::default(); 137],
            sticks: Default::default(),
            last_down: [false; 137],
            last_axis: [0.0; 4],
            pressed_at: [0; 137],
            last_frame_ms: 0,
            concommands: Vec::new(),
            triggered: Vec::new(),
        }
    }
}

pub type Shared = Rc<RefCell<InputState>>;

/// Add a callback (client.dll 0x1802bf110): the same function twice is
/// refused, as is a ninth.
fn add(slots: &mut Slots, cb: Callback) -> Result<(), ()> {
    if slots.iter().flatten().any(|c| c.func.raw_eq(&cb.func)) {
        return Err(());
    }
    let free = slots.iter_mut().find(|s| s.is_none()).ok_or(())?;
    *free = Some(cb);
    Ok(())
}

/// Remove a callback (0x1802bf2e0).
fn remove(slots: &mut Slots, func: &Value) -> Result<(), ()> {
    let s = slots
        .iter_mut()
        .find(|s| s.as_ref().is_some_and(|c| c.func.raw_eq(func)))
        .ok_or(())?;
    *s = None;
    Ok(())
}

fn int(a: &[Value], i: usize) -> i32 {
    match a.get(i) {
        Some(Value::Int(n)) => *n,
        Some(Value::Float(x)) => *x as i32,
        _ => 0,
    }
}

/// A button ID from script, or the game's error.
fn button_id(a: &[Value]) -> Result<usize, NativeError> {
    let id = int(a, 1);
    if (0..137).contains(&id) {
        Ok(id as usize)
    } else {
        Err(throw(format!("Invalid button ID {id}")))
    }
}

pub fn install(cvm: &mut ClientVm) -> Shared {
    let state: Shared = Rc::default();
    let vm = &mut cvm.vm;

    // The names, as root-table integers: buttons, then axes (0x1802bedb0).
    {
        let mut root = vm.root.borrow_mut();
        for (i, (_, name)) in BUTTONS.iter().enumerate() {
            root.insert(Value::str(name), Value::Int(i as i32));
        }
        for (i, (_, name)) in AXES.iter().enumerate() {
            root.insert(Value::str(name), Value::Int(i as i32));
        }
    }

    // InputIsButtonDown( id ) (0x1802bff80): controller buttons count only
    // in controller mode; nothing is down while input doesn't go to the
    // game.
    let host = cvm.host.clone();
    vm.register_checked("InputIsButtonDown", 2, ".n", move |_, a| {
        let id = int(a, 1);
        if !(0..137).contains(&id) {
            return Err(throw(format!("Invalid button ID: {id}")));
        }
        Ok(Value::Bool(button_down(&*host, id as usize)))
    });

    // InputGetAxis( id ) (0x1802c0080).
    let host = cvm.host.clone();
    vm.register_checked("InputGetAxis", 2, ".n", move |_, a| {
        let id = int(a, 1);
        if !(0..4).contains(&id) {
            return Err(throw(format!("Invalid axis ID: {id}")));
        }
        Ok(Value::Float(axis(&*host, id as usize)))
    });

    // GetKeyBinding( id ) (0x1802bfe80): the command bound to the button.
    let host = cvm.host.clone();
    vm.register_checked("GetKeyBinding", 2, ".n", move |_, a| {
        let id = int(a, 1);
        if !(0..137).contains(&id) {
            return Err(throw(format!("Invalid button ID: {id}")));
        }
        Ok(Value::str(&host.key_binding(BUTTONS[id as usize].0)))
    });

    // SetInMenuForButtonCallbacks( inMenu ) (0x1802bf420).
    let st = state.clone();
    vm.register_checked("SetInMenuForButtonCallbacks", 2, ".b", move |_, a| {
        st.borrow_mut().in_menu = matches!(a[1], Value::Bool(true));
        Ok(Value::Null)
    });

    // Register/Deregister Button Pressed/Released callbacks, and the menu
    // versions (0x1802bf490 to 0x1802bfa20): ( buttonID, callback ), mask
    // ".ic"; `this` is kept and used for the call. A double registration
    // fails as "too many" (that error replaces the first).
    for (name, released, menu) in [
        ("RegisterButtonPressedCallback", false, false),
        ("RegisterButtonReleasedCallback", true, false),
        ("RegisterMenuButtonPressedCallback", false, true),
        ("RegisterMenuButtonReleasedCallback", true, true),
    ] {
        let st = state.clone();
        vm.register_checked(name, 3, ".ic", move |_, a| {
            let id = button_id(a)?;
            let mut s = st.borrow_mut();
            let list = if released {
                &mut s.released
            } else {
                &mut s.pressed
            };
            let cb = Callback {
                func: a[2].clone(),
                this: a[0].clone(),
                menu,
            };
            add(&mut list[id], cb)
                .map_err(|_| throw(format!("Too many button callbacks for button ID {id}")))?;
            Ok(Value::Null)
        });
    }
    for (name, released) in [
        ("DeregisterButtonPressedCallback", false),
        ("DeregisterButtonReleasedCallback", true),
    ] {
        let st = state.clone();
        vm.register_checked(name, 3, ".ic", move |_, a| {
            let id = button_id(a)?;
            let mut s = st.borrow_mut();
            let list = if released {
                &mut s.released
            } else {
                &mut s.pressed
            };
            remove(&mut list[id], &a[2]).map_err(|_| {
                throw(format!(
                    "Specified callback was not registered for button ID {id}"
                ))
            })?;
            Ok(Value::Null)
        });
    }

    // RegisterStickMovedCallback / DeregisterStickMovedCallback( axisID,
    // callback ) (0x1802bfb50, 0x1802bfc60).
    let st = state.clone();
    vm.register_checked("RegisterStickMovedCallback", 3, ".ic", move |_, a| {
        let id = int(a, 1);
        if !(0..4).contains(&id) {
            return Err(throw(format!("Invalid axis ID {id}")));
        }
        let cb = Callback {
            func: a[2].clone(),
            this: a[0].clone(),
            menu: true,
        };
        add(&mut st.borrow_mut().sticks[id as usize], cb)
            .map_err(|_| throw(format!("Too many axis callbacks for axis ID {id}")))?;
        Ok(Value::Null)
    });
    let st = state.clone();
    vm.register_checked("DeregisterStickMovedCallback", 3, ".ic", move |_, a| {
        let id = int(a, 1);
        if !(0..4).contains(&id) {
            return Err(throw(format!("Invalid axis ID {id}")));
        }
        // Only a script function: a native one is refused here.
        if !matches!(a[2], Value::Closure(_)) {
            return Err(throw("Second argument should be a function"));
        }
        remove(&mut st.borrow_mut().sticks[id as usize], &a[2]).map_err(|_| {
            throw(format!(
                "Specified callback was not registered for axis ID {id}"
            ))
        })?;
        Ok(Value::Null)
    });

    // RegisterConCommandTriggeredCallback( command, func ) (0x1802c0a50)
    // and Deregister... (0x1802c0ca0). At most 30 in all; a command's
    // callbacks run newest first.
    let (host, st) = (cvm.host.clone(), state.clone());
    vm.register_checked(
        "RegisterConCommandTriggeredCallback",
        3,
        ".sc",
        move |_, a| {
            let Value::Str(cmd) = &a[1] else {
                return Err(throw("Bad commandName parameter"));
            };
            let cmd = cmd.to_string_lossy();
            if !host.concommand_exists(&cmd) {
                return Err(throw(format!("No ConCommand with name '{cmd}' exists")));
            }
            let mut s = st.borrow_mut();
            let i = match s
                .concommands
                .iter()
                .position(|(c, _)| c.eq_ignore_ascii_case(&cmd))
            {
                Some(i) => i,
                None => {
                    s.concommands.push((cmd.clone(), Vec::new()));
                    s.concommands.len() - 1
                }
            };
            if s.concommands[i].1.iter().any(|c| c.func.raw_eq(&a[2])) {
                return Err(throw(format!(
                    "ConCommand '{cmd}' already has this callback registered"
                )));
            }
            let total: usize = s.concommands.iter().map(|(_, l)| l.len()).sum();
            if total >= MAX_CONCOMMAND_CALLBACKS {
                return Err(throw(format!(
                    "Can not register more than {MAX_CONCOMMAND_CALLBACKS} ConCommand callbacks"
                )));
            }
            let cb = Callback {
                func: a[2].clone(),
                this: a[0].clone(),
                menu: false,
            };
            s.concommands[i].1.insert(0, cb);
            Ok(Value::Null)
        },
    );
    let (host, st) = (cvm.host.clone(), state.clone());
    vm.register_checked(
        "DeregisterConCommandTriggeredCallback",
        3,
        ".sc",
        move |_, a| {
            let Value::Str(cmd) = &a[1] else {
                return Err(throw("Bad commandName parameter"));
            };
            let cmd = cmd.to_string_lossy();
            if !host.concommand_exists(&cmd) {
                return Err(throw(format!("No ConCommand with name '{cmd}' exists")));
            }
            let mut s = st.borrow_mut();
            let found = s
                .concommands
                .iter_mut()
                .find(|(c, _)| c.eq_ignore_ascii_case(&cmd))
                .and_then(|(_, l)| {
                    let at = l.iter().position(|c| c.func.raw_eq(&a[2]))?;
                    l.remove(at);
                    Some(())
                });
            if found.is_none() {
                return Err(throw(format!(
                    "ConCommand '{cmd}' does not has this callback registered"
                )));
            }
            // The game also forgets every command triggered this frame.
            s.triggered.clear();
            Ok(Value::Null)
        },
    );
    state
}

/// `InputIsButtonDown` for a script button ID.
fn button_down(host: &dyn crate::Host, id: usize) -> bool {
    if !host.input_to_game() {
        return false;
    }
    let code = BUTTONS[id].0;
    if !host.controller_mode() && (JOYSTICK_FIRST..=JOYSTICK_LAST).contains(&code) {
        return false;
    }
    host.button_down(code)
}

/// `InputGetAxis`: the raw value over 32767, capped at 1 (not below -1).
fn axis(host: &dyn crate::Host, id: usize) -> f32 {
    if !host.input_to_game() {
        return 0.0;
    }
    let v = host.axis_raw(AXES[id].0) as f32 * (1.0 / 32767.0);
    if v > 1.0 { 1.0 } else { v }
}

fn call_all(vm: &mut Vm, cbs: &[Callback], args: &[Value]) {
    for cb in cbs {
        if let Err(e) = vm.call(&cb.func, cb.this.clone(), args) {
            vm.report_error(&e);
        }
    }
}

impl ClientVm {
    /// A button went down (the engine's key event): remembered so a tap
    /// between two frames still runs its callbacks (0x1802bfed0).
    pub fn button_event(&mut self, code: i32, now_ms: u32) {
        let Some(input) = self.input.clone() else {
            return;
        };
        if !self.host.controller_mode() && (JOYSTICK_FIRST..=JOYSTICK_LAST).contains(&code) {
            return;
        }
        if let Some(i) = BUTTONS.iter().position(|b| b.0 == code) {
            input.borrow_mut().pressed_at[i] = now_ms;
        }
    }

    /// A console command ran: its callbacks run next frame (up to 15
    /// commands a frame).
    pub fn concommand_triggered(&mut self, cmd: &str) {
        let Some(input) = self.input.clone() else {
            return;
        };
        let mut s = input.borrow_mut();
        let known = s
            .concommands
            .iter()
            .any(|(c, l)| c.eq_ignore_ascii_case(cmd) && !l.is_empty());
        if known && s.triggered.len() < MAX_TRIGGERS_PER_FRAME {
            s.triggered.push(cmd.to_string());
        }
    }

    /// The per-frame input pass (client.dll 0x1802c0360): button and stick
    /// callbacks (skipped while the game's menus have the input), then the
    /// console-command callbacks. Each gets the local player.
    pub fn input_frame(&mut self, now_ms: u32) {
        let Some(input) = self.input.clone() else {
            return;
        };
        let player = match (self.host.local_player(), self.entities.clone()) {
            (Some(id), Some(ents)) => {
                crate::entities::Entities::instance(&ents, &mut self.vm, &*self.host, id)
            }
            _ => Value::Null,
        };
        let host = self.host.clone();
        if !host.game_ui_active() {
            let since = {
                let mut s = input.borrow_mut();
                let since = s.last_frame_ms.max(now_ms.wrapping_sub(100));
                s.last_frame_ms = now_ms;
                since
            };
            for i in 0..BUTTONS.len() {
                let down = button_down(&*host, i);
                let (calls, menu) = {
                    let mut s = input.borrow_mut();
                    let menu = s.in_menu;
                    if down == s.last_down[i] {
                        // A press since the last frame that's already
                        // released: pressed, then released.
                        let at = s.pressed_at[i];
                        let recent = at.wrapping_sub(since) as i32 >= 0
                            && at.wrapping_sub(now_ms) as i32 <= 0;
                        if recent && !down {
                            s.pressed_at[i] = since.wrapping_sub(1);
                            let mut c: Vec<Callback> =
                                s.pressed[i].iter().flatten().cloned().collect();
                            c.extend(s.released[i].iter().flatten().cloned());
                            (c, menu)
                        } else {
                            continue;
                        }
                    } else {
                        s.last_down[i] = down;
                        let list = if down { &s.pressed[i] } else { &s.released[i] };
                        (list.iter().flatten().cloned().collect(), menu)
                    }
                };
                let calls: Vec<Callback> = calls.into_iter().filter(|c| c.menu == menu).collect();
                call_all(&mut self.vm, &calls, std::slice::from_ref(&player));
            }
            for i in 0..AXES.len() {
                let v = axis(&*host, i);
                let calls: Vec<Callback> = {
                    let mut s = input.borrow_mut();
                    if v == s.last_axis[i] {
                        continue;
                    }
                    s.last_axis[i] = v;
                    s.sticks[i].iter().flatten().cloned().collect()
                };
                call_all(&mut self.vm, &calls, &[player.clone(), Value::Float(v)]);
            }
        }
        let calls: Vec<Callback> = {
            let mut s = input.borrow_mut();
            let triggered = std::mem::take(&mut s.triggered);
            triggered
                .iter()
                .flat_map(|cmd| {
                    s.concommands
                        .iter()
                        .filter(|(c, _)| c.eq_ignore_ascii_case(cmd))
                        .flat_map(|(_, l)| l.iter().cloned())
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        call_all(&mut self.vm, &calls, std::slice::from_ref(&player));
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::Host;

    /// A host with one button the test can hold down.
    struct Pad(Rc<Cell<bool>>);

    impl Host for Pad {
        fn read_script(&self, _: &str) -> Option<String> {
            None
        }
        fn level_name(&self) -> String {
            String::new()
        }
        fn time(&self) -> f32 {
            0.0
        }
        fn frame_time(&self) -> f32 {
            0.0
        }
        fn button_down(&self, code: i32) -> bool {
            code == 11 && self.0.get() // KEY_A
        }
    }

    #[test]
    fn button_callbacks_follow_state_changes_and_menu_flag() {
        let held = Rc::new(Cell::new(false));
        let mut cvm = ClientVm::new(Rc::new(Pad(held.clone())));
        cvm.input = Some(install(&mut cvm));
        let log = Rc::new(RefCell::new(String::new()));
        let sink = log.clone();
        cvm.vm.out = Box::new(move |s| sink.borrow_mut().push_str(s));
        cvm.vm
            .run_script(
                r#"
                function Down( player ) { print( "down " ) }
                function Up( player ) { print( "up " ) }
                function MenuDown( player ) { print( "menu " ) }
                RegisterButtonPressedCallback( KEY_A, Down )
                RegisterButtonReleasedCallback( KEY_A, Up )
                RegisterMenuButtonPressedCallback( KEY_A, MenuDown )
                try { RegisterButtonPressedCallback( KEY_A, Down ) }
                catch ( e ) { print( e + "|" ) }
                try { RegisterButtonPressedCallback( 137, Down ) }
                catch ( e ) { print( e + "|" ) }
                "#,
                "test",
            )
            .unwrap();
        cvm.input_frame(1000);
        held.set(true);
        cvm.input_frame(1016);
        cvm.input_frame(1033);
        held.set(false);
        cvm.input_frame(1050);
        assert_eq!(
            *log.borrow(),
            "Too many button callbacks for button ID 10|Invalid button ID 137|down up "
        );
    }
}
