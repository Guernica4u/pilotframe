//! Menus: main menu, map select, options, and the in-game pause menu.
//!
//! Layout follows the original's (see `docs/reference-ui.md`): big light-blue
//! title top left over a thin bar, a left column of white caps items, a soft
//! blue bar behind the selected item. Plain colours and Bevy's built-in font for
//! now; the original's art and fonts come later, from the player's own files.
//!
//! Mouse (hover + click), keyboard (W/S or arrows, Enter, A/D or arrows to
//! change a setting, Esc to go back) and controller (D-pad, A, B) all work.

use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};

use crate::Screen;
use crate::input::set_captured;
use crate::level::{ChosenMap, colony_path};
use crate::settings::{FOV_RANGE, SENSITIVITY_RANGE, Settings, step_setting};

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Paused(false))
            .init_resource::<Selected>()
            .init_resource::<Pending>()
            .add_systems(
                OnEnter(Screen::MainMenu),
                (menu_camera, free_cursor, spawn_main),
            )
            .add_systems(OnEnter(Screen::MapSelect), (menu_camera, spawn_map_select))
            .add_systems(OnEnter(Screen::Options), (menu_camera, spawn_options))
            .add_systems(OnExit(Screen::MainMenu), despawn_menu)
            .add_systems(OnExit(Screen::MapSelect), despawn_menu)
            .add_systems(OnExit(Screen::Options), despawn_menu)
            .add_systems(OnEnter(Screen::Loading), remove_menu_camera)
            .add_systems(OnEnter(Screen::InGame), capture_cursor)
            .add_systems(OnExit(Screen::InGame), (despawn_menu, unpause))
            .add_systems(Update, toggle_pause.run_if(in_state(Screen::InGame)))
            .add_systems(
                Update,
                (pointer, keys, apply, highlight, refresh_labels)
                    .chain()
                    .run_if(menu_open),
            );
    }
}

/// True while the pause menu is open in game.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paused(pub bool);

/// True while in game with no menu open: movement, aim and mouse capture run.
pub fn playing(screen: Res<State<Screen>>, paused: Res<Paused>) -> bool {
    *screen.get() == Screen::InGame && !paused.0
}

fn menu_open(screen: Res<State<Screen>>, paused: Res<Paused>) -> bool {
    match screen.get() {
        Screen::InGame => paused.0,
        Screen::Loading => false,
        _ => true,
    }
}

const TITLE_BLUE: Color = Color::srgb(0.72, 0.86, 1.0);
const ITEM_WHITE: Color = Color::srgb(0.95, 0.96, 0.98);
const ITEM_GREY: Color = Color::srgb(0.45, 0.47, 0.5);
const GLOW: Color = Color::srgba(0.55, 0.75, 1.0, 0.28);
const BACKGROUND: Color = Color::srgb(0.07, 0.09, 0.12);
const NOTICE: &str = "Unofficial fan project. Not affiliated with EA or Respawn. \
                      Uses game files from your own copy.";

#[derive(Component)]
struct MenuRoot;

#[derive(Component)]
struct MenuCamera;

/// What an item does.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
enum MenuAction {
    Play,
    Options,
    Quit,
    PickMap(ChosenMap),
    Back,
    Resume,
    ToMainMenu,
    Sensitivity,
    Fov,
}

/// Position in the page, for keyboard/controller navigation.
#[derive(Component)]
struct Item {
    index: usize,
    enabled: bool,
}

#[derive(Component)]
struct ItemText;

#[derive(Resource, Default)]
struct Selected(usize);

/// An item activated this frame: (action, direction). Direction is +1 for
/// click/Enter/right, -1 for left; only settings use it.
#[derive(Resource, Default)]
struct Pending(Option<(MenuAction, i32)>);

fn menu_camera(mut commands: Commands, existing: Query<(), With<MenuCamera>>) {
    if existing.is_empty() {
        commands.spawn((MenuCamera, Camera2d));
    }
}

fn remove_menu_camera(mut commands: Commands, cams: Query<Entity, With<MenuCamera>>) {
    for e in &cams {
        commands.entity(e).despawn();
    }
}

fn free_cursor(mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>) {
    set_captured(&mut cursor, false);
}

fn capture_cursor(mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>) {
    set_captured(&mut cursor, true);
}

fn unpause(mut paused: ResMut<Paused>) {
    paused.0 = false;
}

fn despawn_menu(mut commands: Commands, roots: Query<Entity, With<MenuRoot>>) {
    for e in &roots {
        commands.entity(e).despawn();
    }
}

/// One page: title, items (label, action, enabled), and whether it's drawn over
/// the game (pause) or on its own background.
fn spawn_page(
    commands: &mut Commands,
    selected: &mut Selected,
    title: &str,
    items: &[(String, MenuAction, bool)],
    overlay: bool,
) {
    selected.0 = items.iter().position(|i| i.2).unwrap_or(0);
    let background = if overlay {
        Color::srgba(0.0, 0.0, 0.0, 0.6)
    } else {
        BACKGROUND
    };
    commands
        .spawn((
            MenuRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::new(Val::Px(110.0), Val::Px(110.0), Val::Px(80.0), Val::Px(40.0)),
                ..default()
            },
            BackgroundColor(background),
            // Draw over the in-game HUD.
            GlobalZIndex(10),
        ))
        .with_children(|page| {
            page.spawn((
                Text::new(title),
                TextFont {
                    font_size: FontSize::Px(60.0),
                    ..default()
                },
                TextColor(TITLE_BLUE),
            ));
            page.spawn((
                Node {
                    width: Val::Percent(90.0),
                    height: Val::Px(4.0),
                    margin: UiRect::new(Val::Px(0.0), Val::Px(0.0), Val::Px(8.0), Val::Px(40.0)),
                    ..default()
                },
                BackgroundColor(TITLE_BLUE.with_alpha(0.55)),
            ));
            for (index, (label, action, enabled)) in items.iter().enumerate() {
                page.spawn((
                    Button,
                    *action,
                    Item {
                        index,
                        enabled: *enabled,
                    },
                    Node {
                        width: Val::Px(620.0),
                        padding: UiRect::axes(Val::Px(20.0), Val::Px(6.0)),
                        margin: UiRect::bottom(Val::Px(6.0)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|item| {
                    item.spawn((
                        ItemText,
                        Text::new(label.clone()),
                        TextFont {
                            font_size: FontSize::Px(32.0),
                            ..default()
                        },
                        TextColor(if *enabled { ITEM_WHITE } else { ITEM_GREY }),
                    ));
                });
            }
            page.spawn((
                Text::new(NOTICE),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(ITEM_GREY),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(110.0),
                    bottom: Val::Px(24.0),
                    ..default()
                },
            ));
        });
}

fn spawn_main(mut commands: Commands, mut selected: ResMut<Selected>) {
    let items = [
        ("PLAY".to_string(), MenuAction::Play, true),
        ("OPTIONS".to_string(), MenuAction::Options, true),
        ("QUIT".to_string(), MenuAction::Quit, true),
    ];
    spawn_page(&mut commands, &mut selected, "PILOTFRAME", &items, false);
}

fn spawn_map_select(mut commands: Commands, mut selected: ResMut<Selected>) {
    let colony = colony_path().is_some();
    let items = [
        (
            "TEST MAP".to_string(),
            MenuAction::PickMap(ChosenMap::TestMap),
            true,
        ),
        (
            if colony {
                "COLONY".to_string()
            } else {
                "COLONY  (files not found)".to_string()
            },
            MenuAction::PickMap(ChosenMap::Colony),
            colony,
        ),
        ("BACK".to_string(), MenuAction::Back, true),
    ];
    spawn_page(&mut commands, &mut selected, "SELECT MAP", &items, false);
}

fn option_items(settings: &Settings) -> Vec<(String, MenuAction, bool)> {
    vec![
        (sensitivity_label(settings), MenuAction::Sensitivity, true),
        (fov_label(settings), MenuAction::Fov, true),
    ]
}

fn sensitivity_label(s: &Settings) -> String {
    format!("MOUSE SENSITIVITY   < {:.2} >", s.mouse_sensitivity)
}

fn fov_label(s: &Settings) -> String {
    format!("FIELD OF VIEW   < {:.0} >", s.fov_degrees)
}

fn spawn_options(mut commands: Commands, mut selected: ResMut<Selected>, settings: Res<Settings>) {
    let mut items = option_items(&settings);
    items.push(("BACK".to_string(), MenuAction::Back, true));
    spawn_page(&mut commands, &mut selected, "OPTIONS", &items, false);
}

/// Esc in game opens or closes the pause menu.
#[allow(clippy::too_many_arguments)]
fn toggle_pause(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut paused: ResMut<Paused>,
    mut selected: ResMut<Selected>,
    settings: Res<Settings>,
    roots: Query<Entity, With<MenuRoot>>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let pressed = keys.just_pressed(KeyCode::Escape)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::Start));
    if !pressed {
        return;
    }
    paused.0 = !paused.0;
    set_captured(&mut cursor, !paused.0);
    for e in &roots {
        commands.entity(e).despawn();
    }
    if paused.0 {
        let mut items = vec![("RESUME".to_string(), MenuAction::Resume, true)];
        items.extend(option_items(&settings));
        items.push(("MAIN MENU".to_string(), MenuAction::ToMainMenu, true));
        items.push(("QUIT".to_string(), MenuAction::Quit, true));
        spawn_page(&mut commands, &mut selected, "PAUSED", &items, true);
    }
}

/// Mouse: hovering selects, clicking activates.
fn pointer(
    items: Query<(&Interaction, &Item, &MenuAction), Changed<Interaction>>,
    mut selected: ResMut<Selected>,
    mut pending: ResMut<Pending>,
) {
    for (interaction, item, action) in &items {
        match interaction {
            Interaction::Hovered => selected.0 = item.index,
            Interaction::Pressed if item.enabled => {
                selected.0 = item.index;
                pending.0 = Some((*action, 1));
            }
            _ => {}
        }
    }
}

/// Keyboard and controller navigation.
fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    items: Query<(&Item, &MenuAction)>,
    mut selected: ResMut<Selected>,
    mut pending: ResMut<Pending>,
    paused: Res<Paused>,
) {
    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    let key = |k: &[KeyCode]| k.iter().any(|k| keys.just_pressed(*k));
    let count = items.iter().count();
    if count == 0 {
        return;
    }
    let mut move_by = |d: isize| {
        // Skip disabled items.
        for _ in 0..count {
            selected.0 = (selected.0 as isize + d).rem_euclid(count as isize) as usize;
            if items
                .iter()
                .any(|(i, _)| i.index == selected.0 && i.enabled)
            {
                break;
            }
        }
    };
    if key(&[KeyCode::ArrowDown, KeyCode::KeyS]) || pad(GamepadButton::DPadDown) {
        move_by(1);
    }
    if key(&[KeyCode::ArrowUp, KeyCode::KeyW]) || pad(GamepadButton::DPadUp) {
        move_by(-1);
    }
    let current = items
        .iter()
        .find(|(i, _)| i.index == selected.0 && i.enabled)
        .map(|(_, a)| *a);
    let Some(action) = current else { return };
    let is_setting = matches!(action, MenuAction::Sensitivity | MenuAction::Fov);
    let confirm =
        key(&[KeyCode::Enter, KeyCode::Space, KeyCode::NumpadEnter]) || pad(GamepadButton::South);
    let right = key(&[KeyCode::ArrowRight, KeyCode::KeyD]) || pad(GamepadButton::DPadRight);
    if confirm || (is_setting && right) {
        pending.0 = Some((action, 1));
    } else if is_setting
        && (key(&[KeyCode::ArrowLeft, KeyCode::KeyA]) || pad(GamepadButton::DPadLeft))
    {
        pending.0 = Some((action, -1));
    } else if (key(&[KeyCode::Escape, KeyCode::Backspace]) && !paused.0) || pad(GamepadButton::East)
    {
        // Esc in the pause menu is handled by toggle_pause.
        pending.0 = Some((MenuAction::Back, 1));
    }
}

#[allow(clippy::too_many_arguments)]
fn apply(
    mut pending: ResMut<Pending>,
    screen: Res<State<Screen>>,
    mut next: ResMut<NextState<Screen>>,
    mut chosen: ResMut<ChosenMap>,
    mut settings: ResMut<Settings>,
    mut paused: ResMut<Paused>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
    roots: Query<Entity, With<MenuRoot>>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let Some((action, dir)) = pending.0.take() else {
        return;
    };
    match action {
        MenuAction::Play => next.set(Screen::MapSelect),
        MenuAction::Options => next.set(Screen::Options),
        MenuAction::Quit => {
            exit.write(AppExit::Success);
        }
        MenuAction::PickMap(map) => {
            *chosen = map;
            next.set(Screen::Loading);
        }
        MenuAction::Back => match screen.get() {
            Screen::MapSelect | Screen::Options => next.set(Screen::MainMenu),
            _ => {}
        },
        MenuAction::Resume => {
            paused.0 = false;
            set_captured(&mut cursor, true);
            for e in &roots {
                commands.entity(e).despawn();
            }
        }
        MenuAction::ToMainMenu => next.set(Screen::MainMenu),
        MenuAction::Sensitivity => {
            settings.mouse_sensitivity =
                step_setting(settings.mouse_sensitivity, dir, SENSITIVITY_RANGE);
            settings.save();
        }
        MenuAction::Fov => {
            settings.fov_degrees = step_setting(settings.fov_degrees, dir, FOV_RANGE);
            settings.save();
        }
    }
}

/// Glow bar behind the selected item.
fn highlight(selected: Res<Selected>, mut items: Query<(&Item, &mut BackgroundColor)>) {
    for (item, mut bg) in &mut items {
        let want = if item.index == selected.0 && item.enabled {
            GLOW
        } else {
            Color::NONE
        };
        if bg.0 != want {
            bg.0 = want;
        }
    }
}

/// Keep setting values on screen up to date.
fn refresh_labels(
    settings: Res<Settings>,
    items: Query<(&MenuAction, &Children)>,
    mut texts: Query<&mut Text, With<ItemText>>,
) {
    if !settings.is_changed() {
        return;
    }
    for (action, children) in &items {
        let label = match action {
            MenuAction::Sensitivity => sensitivity_label(&settings),
            MenuAction::Fov => fov_label(&settings),
            _ => continue,
        };
        for child in children.iter() {
            if let Ok(mut text) = texts.get_mut(child) {
                text.0 = label.clone();
            }
        }
    }
}
