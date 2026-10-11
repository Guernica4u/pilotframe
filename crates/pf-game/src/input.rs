//! Named input actions. Gameplay code only asks "is Jump pressed?", never "is
//! Space pressed?", so keyboard/mouse and controller are equal from the start and
//! rebinding later only touches `default_bindings`.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use leafwing_input_manager::prelude::*;

use crate::shell::playing;

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        // The mouse is captured only while playing; Esc opens the pause menu
        // (menu.rs), which releases it.
        app.add_plugins(InputManagerPlugin::<Action>::default())
            .add_systems(Update, recapture_on_click.run_if(playing));
    }
}

// Rust note: `derive` generates trait implementations for us. `Actionlike` (from
// leafwing) is what lets this enum be used as a set of bindable actions.
#[derive(Actionlike, PartialEq, Eq, Hash, Clone, Copy, Debug, Reflect)]
pub enum Action {
    /// Left stick or WASD. x = right, y = forwards.
    #[actionlike(DualAxis)]
    Move,
    /// Mouse movement, in pixels this frame.
    #[actionlike(DualAxis)]
    Look,
    /// Right stick, -1..1. Kept apart from `Look` because it's a rate, not a distance.
    #[actionlike(DualAxis)]
    LookStick,
    Jump,
    /// Held (keyboard).
    Sprint,
    /// Click once to sprint until you stop moving forwards (controller, L3).
    SprintToggle,
    /// Held. Ducks.
    Crouch,
    /// Held. Aim down sights; on a wall, this hangs on it.
    Zoom,
    /// Held. Fire the weapon.
    Fire,
    Reload,
    /// Back to the spawn point.
    Reset,
}

pub fn default_bindings() -> InputMap<Action> {
    InputMap::default()
        .with_dual_axis(Action::Move, VirtualDPad::wasd())
        .with_dual_axis(Action::Move, GamepadStick::LEFT)
        .with_dual_axis(Action::Look, MouseMove::default())
        .with_dual_axis(Action::LookStick, GamepadStick::RIGHT)
        .with(Action::Jump, KeyCode::Space)
        .with(Action::Jump, GamepadButton::South)
        .with(Action::Sprint, KeyCode::ShiftLeft)
        .with(Action::SprintToggle, GamepadButton::LeftThumb)
        .with(Action::Crouch, KeyCode::ControlLeft)
        .with(Action::Crouch, KeyCode::KeyC)
        .with(Action::Crouch, GamepadButton::East)
        .with(Action::Zoom, MouseButton::Right)
        .with(Action::Zoom, GamepadButton::LeftTrigger2)
        .with(Action::Fire, MouseButton::Left)
        .with(Action::Fire, GamepadButton::RightTrigger2)
        .with(Action::Reload, KeyCode::KeyR)
        .with(Action::Reload, GamepadButton::West)
        .with(Action::Reset, KeyCode::Backspace)
        .with(Action::Reset, GamepadButton::Select)
}

/// Look feel. Not movement tuning (it doesn't change the simulation), so it lives
/// here rather than in movement.ron. Will become player settings.
pub mod look {
    /// Radians of turn per pixel of mouse movement.
    pub const MOUSE_SENSITIVITY: f32 = 0.0022;
    /// Radians per second at full right-stick deflection.
    pub const STICK_YAW_SPEED: f32 = 3.2;
    pub const STICK_PITCH_SPEED: f32 = 2.2;
    /// Stick deflection is raised to this power: small tilts aim finely, full tilt
    /// still turns at full speed.
    pub const STICK_CURVE: f32 = 2.0;
    /// Below this deflection the stick counts as centred (worn sticks drift).
    pub const STICK_DEADZONE: f32 = 0.12;
}

/// If the mouse got released while playing (alt-tab), a click takes it back.
fn recapture_on_click(
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    if mouse.just_pressed(MouseButton::Left) {
        set_captured(&mut cursor, true);
    }
}

/// Lock and hide the mouse for look, or free it for menus.
pub fn set_captured(cursor: &mut CursorOptions, captured: bool) {
    cursor.grab_mode = if captured {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
    cursor.visible = !captured;
}

pub fn cursor_captured(cursor: &CursorOptions) -> bool {
    cursor.grab_mode != CursorGrabMode::None
}
