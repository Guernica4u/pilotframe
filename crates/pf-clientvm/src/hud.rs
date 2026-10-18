//! HUD elements: client.dll's `CClientHudElement` class (C++
//! `CClientScriptHudElement`, registered at 0x18042ef50) wrapping VGUI
//! panels (pf-vgui), the `Hud` object, and VGUI screens
//! (`CreateClientsideVGuiScreen`, `C_VGuiScreen.GetPanel`).
//!
//! An element remembers its panel's position, size, colour and visibility
//! from when it was made (its "base" values). Setting a property stops any
//! animation running on it. At most 8000 elements exist at once, and a
//! panel can only be wrapped once ("HudElement "X" has already been
//! created."). The rules are from client.dll (addresses beside each method).

use std::cell::RefCell;
use std::rc::Rc;

use pf_squirrel::table::Table;
use pf_squirrel::value::{Class, Ref, new_ref};
use pf_squirrel::{NativeError, Value, Vm, native, throw};
use pf_vgui::anim::{Controller, Curve, Var};
use pf_vgui::{Control, PanelId, Rgba, Ui};

use crate::entities::{self, Entities};
use crate::{ClientVm, EntityInfo};

pub const MAX_ELEMENTS: usize = 8000;

/// One HUD element.
pub struct Element {
    pub panel: PanelId,
    pub instance: Value,
    /// What scripts asked for (`SetVisible`/`Show`/`Hide`).
    pub visible: bool,
    /// Hidden regardless (the game sets this for elements it hides itself).
    pub hidden: bool,
    pub base_pos: (f32, f32),
    pub base_size: (f32, f32),
    pub base_color: Rgba,
    /// Children fetched through it (`GetChild`/`HudElement`), newest first.
    pub children: Vec<usize>,
    pub parent_element: Option<usize>,
    /// `SetVisGroupID` (-1 until set, client.dll 0x1804293c0).
    pub vis_group: i32,
    /// `EnableKeyBindingIcons` was called.
    pub key_binding_icons: bool,
    /// `SetAutoText`.
    pub auto_text: Option<crate::autotext::AutoText>,
    /// `SetClampToScreen`: 0 none, 1 and 2 the game's clamp modes.
    pub clamp_to_screen: i32,
    /// `SetClampBounds( x, y )` (client.dll 0x180420a90) and
    /// `SetOffscreenArrow( bool )` (0x180420ab0).
    pub clamp_bounds: (f32, f32),
    pub offscreen_arrow: bool,
    /// `SetPulsate( minAlpha, maxAlpha, frequency )`: (min, max - min,
    /// frequency, phase start time); None after `ClearPulsate`.
    pub pulsate: Option<[f32; 4]>,
    /// `SetADSFade`, `SetFOVFade`, `SetDistanceFade`: the four numbers each
    /// (stored; the fading is drawn by pf-game).
    pub ads_fade: Option<[f32; 4]>,
    pub fov_fade: Option<[f32; 4]>,
    pub distance_fade: Option<[f32; 4]>,
}

pub struct HudState {
    pub ui: Ui,
    /// The HUD's animation controller (runs on real time).
    pub anim: Controller,
    /// Elements with auto text, newest first (the game's list).
    pub auto_text_order: Vec<usize>,
    /// `Crosshair_SetState` (CROSSHAIR_STATE_*).
    pub crosshair_state: i32,
    pub elements: Vec<Option<Element>>,
    class: Option<Ref<Class>>,
    /// Screen entity → its screen in `ui`, and the element of its root.
    screens: Vec<(crate::EntityId, usize, Option<usize>)>,
}

pub type SharedHud = Rc<RefCell<HudState>>;

fn element_index(v: &Value) -> Option<usize> {
    crate::classes::user_of::<HudHandle>(v).map(|h| h.0)
}

/// What a HUD element instance carries.
#[derive(Clone, Copy)]
struct HudHandle(usize);

/// The element behind `this`, or an error.
fn this_element(hud: &SharedHud, a: &[Value]) -> Result<usize, NativeError> {
    element_index(&a[0])
        .filter(|&i| hud.borrow().elements.get(i).is_some_and(Option::is_some))
        .ok_or_else(|| throw("invalid hud element"))
}

/// Make an element for `panel` (0x180429ea0).
fn create_element(hud: &SharedHud, vm: &mut Vm, panel: PanelId) -> Result<Value, NativeError> {
    let (class, base) = {
        let h = hud.borrow();
        let live = h.elements.iter().filter(|e| e.is_some()).count();
        if live >= MAX_ELEMENTS {
            return Err(throw("No free HudElements; see console log"));
        }
        let p = h.ui.panel(panel).ok_or_else(|| throw("invalid panel"))?;
        if p.element.is_some() {
            return Err(throw(format!(
                "HudElement \"{}\" has already been created.",
                p.name
            )));
        }
        (
            h.class.clone(),
            ((p.x, p.y), (p.w, p.h), p.fg_color, p.visible),
        )
    };
    let index = {
        let mut h = hud.borrow_mut();
        h.elements.push(None);
        h.elements.len() - 1
    };
    let class = class.ok_or_else(|| throw("HUD element class missing"))?;
    let instance = crate::classes::instantiate(vm, &class, crate::classes::user(HudHandle(index)))
        .map_err(NativeError::Throw)?;
    let mut h = hud.borrow_mut();
    h.elements[index] = Some(Element {
        panel,
        instance: instance.clone(),
        visible: base.3,
        hidden: false,
        base_pos: base.0,
        base_size: base.1,
        base_color: base.2,
        children: Vec::new(),
        parent_element: None,
        vis_group: -1,
        key_binding_icons: false,
        auto_text: None,
        pulsate: None,
        clamp_bounds: (0.0, 0.0),
        offscreen_arrow: false,
        clamp_to_screen: 0,
        ads_fade: None,
        fov_fade: None,
        distance_fade: None,
    });
    if let Some(p) = h.ui.panel_mut(panel) {
        p.element = Some(index);
    }
    Ok(instance)
}

/// A child panel of an element's panel by name, wrapped (0x180429f80).
fn child_element(
    hud: &SharedHud,
    vm: &mut Vm,
    parent: PanelId,
    name: &str,
) -> Result<Value, NativeError> {
    let found = hud.borrow().ui.find_child(parent, name);
    match found {
        Some(child) => create_element(hud, vm, child),
        None => {
            let pname = hud
                .borrow()
                .ui
                .panel(parent)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            Err(throw(format!(
                "HudElement \"{name}\" could not be found on panel \"{pname}\"."
            )))
        }
    }
}

fn str_arg(a: &[Value], i: usize) -> String {
    match a.get(i) {
        Some(Value::Str(s)) => s.to_string_lossy(),
        _ => String::new(),
    }
}

fn num_arg(a: &[Value], i: usize) -> f32 {
    match a.get(i) {
        Some(Value::Int(n)) => *n as f32,
        Some(Value::Float(x)) => *x,
        _ => 0.0,
    }
}

fn int_pair(x: f32, y: f32) -> Value {
    Value::Array(new_ref(vec![Value::Int(x as i32), Value::Int(y as i32)]))
}

/// Apply `visible && !hidden` to the panel (0x18041fe60).
fn update_visibility(h: &mut HudState, i: usize) {
    let Some(e) = h.elements[i].as_ref() else {
        return;
    };
    let (on, panel) = (e.visible && !e.hidden, e.panel);
    if let Some(p) = h.ui.panel_mut(panel) {
        p.visible = on;
    }
}

pub fn install(cvm: &mut ClientVm) -> SharedHud {
    let host = cvm.host.clone();
    let mut ui = Ui::new(&|p| host.read_script(p));
    // The HUD's root panel and its elements (client.dll 0x18041eb90).
    let res = if host.optimized_lobby() {
        "resource/UI/HudScripted_lobby.res"
    } else {
        "resource/UI/HudScripted_mp.res"
    };
    let (w, h) = host.screen_size();
    ui.create_hud(res, (w as f32, h as f32), &|p| host.read_script(p));
    let hud: SharedHud = Rc::new(RefCell::new(HudState {
        ui,
        anim: Controller::default(),
        auto_text_order: Vec::new(),
        crosshair_state: 0,
        elements: Vec::new(),
        class: None,
        screens: Vec::new(),
    }));

    let m = Table::new();
    type Body = Box<dyn Fn(&mut Vm, &[Value]) -> pf_squirrel::NativeResult>;
    let m = Rc::new(RefCell::new(m));
    let m2 = m.clone();
    let add = |name: &'static str, n: i32, f: Body| {
        m2.borrow_mut()
            .insert(Value::str(name), native(name, n, move |vm, a| f(vm, a)));
    };
    // With the type mask launcher.dll gives the method.
    let m3 = m.clone();
    let add_checked = |name: &'static str, n: i32, mask: &'static str, f: Body| {
        m3.borrow_mut().insert(
            Value::str(name),
            pf_squirrel::native_checked(name, n, mask, move |vm, a| f(vm, a)),
        );
    };

    // Child lookup (0x18042a040): makes the child's element.
    for name in ["HudElement", "GetChild"] {
        let hud = hud.clone();
        add(
            name,
            2,
            Box::new(move |vm, a| {
                let i = this_element(&hud, a)?;
                let panel = hud.borrow().elements[i]
                    .as_ref()
                    .map(|e| e.panel)
                    .unwrap_or(0);
                let v = child_element(&hud, vm, panel, &str_arg(a, 1))?;
                if let Some(c) = element_index(&v) {
                    let mut h = hud.borrow_mut();
                    if let Some(e) = h.elements[c].as_mut() {
                        e.parent_element = Some(i);
                    }
                    if let Some(e) = h.elements[i].as_mut() {
                        e.children.insert(0, c);
                    }
                }
                Ok(v)
            }),
        );
    }
    let h2 = hud.clone();
    add(
        "HasChild",
        2,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            let h = h2.borrow();
            let panel = h.elements[i].as_ref().map(|e| e.panel).unwrap_or(0);
            Ok(Value::Bool(
                h.ui.find_child(panel, &str_arg(a, 1)).is_some(),
            ))
        }),
    );
    // GetParent (0x180426cc0): the parent panel's element, or null.
    let h2 = hud.clone();
    add(
        "GetParent",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            let h = h2.borrow();
            let panel = h.elements[i].as_ref().map(|e| e.panel).unwrap_or(0);
            let parent = h.ui.panel(panel).and_then(|p| p.parent);
            let el = parent.and_then(|p| h.ui.panel(p)).and_then(|p| p.element);
            Ok(el
                .and_then(|e| h.elements[e].as_ref())
                .map_or(Value::Null, |e| e.instance.clone()))
        }),
    );
    let h2 = hud.clone();
    add(
        "GetName",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            let h = h2.borrow();
            let panel = h.elements[i].as_ref().map(|e| e.panel).unwrap_or(0);
            Ok(Value::str(
                &h.ui
                    .panel(panel)
                    .map(|p| p.name.clone())
                    .unwrap_or_default(),
            ))
        }),
    );

    // Visibility (0x180425840 SetVisible, 0x1804257b0 Show, 0x180425800
    // Hide, 0x18041fe60 UpdateVisibility).
    let h2 = hud.clone();
    add(
        "SetVisible",
        2,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            let mut h = h2.borrow_mut();
            if let Some(e) = h.elements[i].as_mut() {
                e.visible = a[1].truthy();
            }
            update_visibility(&mut h, i);
            Ok(Value::Null)
        }),
    );
    for (name, on) in [("Show", true), ("Hide", false)] {
        let h2 = hud.clone();
        add(
            name,
            1,
            Box::new(move |_, a| {
                let i = this_element(&h2, a)?;
                let mut h = h2.borrow_mut();
                if let Some(e) = h.elements[i].as_mut() {
                    e.visible = on;
                }
                update_visibility(&mut h, i);
                Ok(Value::Null)
            }),
        );
    }
    let h2 = hud.clone();
    add(
        "UpdateVisibility",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            update_visibility(&mut h2.borrow_mut(), i);
            Ok(Value::Null)
        }),
    );
    let h2 = hud.clone();
    add(
        "IsVisible",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            let h = h2.borrow();
            let panel = h.elements[i].as_ref().map(|e| e.panel).unwrap_or(0);
            Ok(Value::Bool(h.ui.panel(panel).is_some_and(|p| p.visible)))
        }),
    );

    // SetText( text, ... ) (0x180420b50): labels only; up to five
    // arguments for a localized string's %s1..%s5.
    let h2 = hud.clone();
    add(
        "SetText",
        -2,
        Box::new(move |vm, a| {
            let i = this_element(&h2, a)?;
            let mut args = Vec::new();
            for v in a.iter().skip(2).take(5) {
                if v.is_null() {
                    break;
                }
                let mut s = String::from_utf8_lossy(&vm.to_bytes(v)?).into_owned();
                s.truncate(0x7f);
                args.push(s);
            }
            let text = str_arg(a, 1);
            let mut h = h2.borrow_mut();
            let panel = h.elements[i].as_ref().map(|e| e.panel).unwrap_or(0);
            let p =
                h.ui.panel_mut(panel)
                    .ok_or_else(|| throw("invalid panel"))?;
            if p.control != Control::Label {
                return Err(throw(format!("No label element with name '{}'.", p.name)));
            }
            p.text = text;
            p.text_args = args;
            Ok(Value::Null)
        }),
    );
    // SetImage( name ) (0x180420d20): image panels only.
    let h2 = hud.clone();
    add(
        "SetImage",
        2,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            let mut h = h2.borrow_mut();
            let panel = h.elements[i].as_ref().map(|e| e.panel).unwrap_or(0);
            let p =
                h.ui.panel_mut(panel)
                    .ok_or_else(|| throw("invalid panel"))?;
            if p.control != Control::ImagePanel {
                return Err(throw(format!(
                    "No ImagePanel element with name '{}'.",
                    p.name
                )));
            }
            p.image = Some(str_arg(a, 1));
            Ok(Value::Null)
        }),
    );

    // Colour (0x180421130): an image panel's draw colour, otherwise the
    // foreground colour. SetColor( r, g, b[, a] ) or SetColor( [r, g, b, a] ).
    let h2 = hud.clone();
    add(
        "SetColor",
        -2,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            cancel(&h2, i, &[Var::FgColor]);
            let c = color_args(a, &h2, i)?;
            set_color(&mut h2.borrow_mut(), i, c);
            Ok(Value::Null)
        }),
    );
    // SetColorBG (0x1804298f0): the same arguments, the background colour.
    let h2 = hud.clone();
    add(
        "SetColorBG",
        -2,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            cancel(&h2, i, &[Var::BgColor]);
            let c = color_args(a, &h2, i)?;
            with_panel(&h2, i, |p| p.bg_color = c);
            Ok(Value::Null)
        }),
    );
    // SetVisGroupID( id ) (0x18041fec0): which visibility group the element
    // belongs to (`hud_setvisgroup` and friends show and hide by group).
    let h2 = hud.clone();
    add(
        "SetVisGroupID",
        2,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            if let Some(e) = h2.borrow_mut().elements[i].as_mut() {
                e.vis_group = num_arg(a, 1) as i32;
            }
            Ok(Value::Null)
        }),
    );
    // EnableKeyBindingIcons (0x180420030): show `%command%` tokens in the
    // label's text as the bound key's icon (docs/port/hud.md; drawn by
    // pf-game).
    let h2 = hud.clone();
    add(
        "EnableKeyBindingIcons",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            if let Some(e) = h2.borrow_mut().elements[i].as_mut() {
                e.key_binding_icons = true;
            }
            Ok(Value::Null)
        }),
    );
    // IsValidInternal (launcher.dll 0x18000c920, on every class): the
    // element still exists.
    let h2 = hud.clone();
    add(
        "IsValidInternal",
        1,
        Box::new(move |_, a| Ok(Value::Bool(this_element(&h2, a).is_ok()))),
    );
    // The ...OverTime methods (masks and argument order from client.dll;
    // an optional last interpolator, 0 = linear).
    // MoveOverTime( x, y, duration[, interp] ) (0x180426200).
    let h2 = hud.clone();
    add_checked(
        "MoveOverTime",
        -4,
        ".nnn",
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            move_over_time(
                &h2,
                i,
                int_at(a, 1),
                int_at(a, 2),
                num_arg(a, 3),
                int_at(a, 4),
            )?;
            Ok(Value::Null)
        }),
    );
    // OffsetOverTime( dx, dy, duration[, interp] ) (0x1804262d0): from the
    // current position.
    let h2 = hud.clone();
    add_checked(
        "OffsetOverTime",
        -4,
        ".nnn",
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            let (x, y) = read_panel(&h2, i, |p| (p.x as i32, p.y as i32));
            move_over_time(
                &h2,
                i,
                x + int_at(a, 1),
                y + int_at(a, 2),
                num_arg(a, 3),
                int_at(a, 4),
            )?;
            Ok(Value::Null)
        }),
    );
    // SetXOverTime / SetYOverTime( v, duration[, interp] ) (0x1804263e0,
    // 0x1804264c0) and OffsetXOverTime / OffsetYOverTime (0x1804265a0,
    // 0x180426680): the other coordinate is animated to where it is now.
    for (name, set_x, offset) in [
        ("SetXOverTime", true, false),
        ("SetYOverTime", false, false),
        ("OffsetXOverTime", true, true),
        ("OffsetYOverTime", false, true),
    ] {
        let h2 = hud.clone();
        add_checked(
            name,
            -3,
            ".nn",
            Box::new(move |_, a| {
                let i = this_element(&h2, a)?;
                let (x, y) = read_panel(&h2, i, |p| (p.x as i32, p.y as i32));
                let v = int_at(a, 1);
                let (nx, ny) = match (set_x, offset) {
                    (true, false) => (v, y),
                    (false, false) => (x, v),
                    (true, true) => (x + v, y),
                    (false, true) => (x, y + v),
                };
                move_over_time(&h2, i, nx, ny, num_arg(a, 2), int_at(a, 3))?;
                Ok(Value::Null)
            }),
        );
    }
    // ReturnToBasePosOverTime( duration, interp ) (0x180421c10).
    let h2 = hud.clone();
    add_checked(
        "ReturnToBasePosOverTime",
        3,
        ".nn",
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            let b = h2.borrow().elements[i]
                .as_ref()
                .map(|e| e.base_pos)
                .unwrap_or_default();
            move_over_time(&h2, i, b.0 as i32, b.1 as i32, num_arg(a, 1), int_at(a, 2))?;
            Ok(Value::Null)
        }),
    );
    // ScaleOverTime( sx, sy, duration[, interp] ) (0x180426790) and
    // ReturnToBaseScaleOverTime( duration, interp ) (0x180426760).
    let h2 = hud.clone();
    add_checked(
        "ScaleOverTime",
        -4,
        ".nnn",
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            scale_over_time(
                &h2,
                i,
                num_arg(a, 1),
                num_arg(a, 2),
                num_arg(a, 3),
                int_at(a, 4),
            )?;
            Ok(Value::Null)
        }),
    );
    let h2 = hud.clone();
    add_checked(
        "ReturnToBaseScaleOverTime",
        3,
        ".nn",
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            scale_over_time(&h2, i, 1.0, 1.0, num_arg(a, 1), int_at(a, 2))?;
            Ok(Value::Null)
        }),
    );
    // ColorOverTime( r, g, b, a, duration[, interp] ) (0x180429940) and
    // ColorOverTimeDelayed( r, g, b, a, duration, delay[, interp] )
    // (0x180429c10): each component's low 8 bits.
    for (name, delayed) in [("ColorOverTime", false), ("ColorOverTimeDelayed", true)] {
        let h2 = hud.clone();
        let (n, mask) = if delayed {
            (-7, ".iiiinn")
        } else {
            (-6, ".iiiin")
        };
        add_checked(
            name,
            n,
            mask,
            Box::new(move |_, a| {
                let i = this_element(&h2, a)?;
                let rgba = [1, 2, 3, 4].map(|k| int_at(a, k) as u8);
                let (delay, interp) = if delayed {
                    (num_arg(a, 6), int_at(a, 7))
                } else {
                    (0.0, int_at(a, 6))
                };
                color_over_time(&h2, i, rgba, num_arg(a, 5), delay, interp)?;
                Ok(Value::Null)
            }),
        );
    }
    // FadeOverTime( alpha, duration[, interp] ) (0x180429aa0),
    // FadeOverTimeDelayed( alpha, duration, delay[, interp] ) (0x180429d70)
    // and HideOverTime( duration[, interp] ) (0x180429b50, alpha 0): the
    // current colour with the new alpha.
    for (name, n, mask) in [
        ("FadeOverTime", -3, ".in"),
        ("FadeOverTimeDelayed", -4, ".inn"),
        ("HideOverTime", -2, ".n"),
    ] {
        let h2 = hud.clone();
        add_checked(
            name,
            n,
            mask,
            Box::new(move |_, a| {
                let i = this_element(&h2, a)?;
                let (alpha, dur, delay, interp) = match name {
                    "FadeOverTime" => (int_at(a, 1), num_arg(a, 2), 0.0, int_at(a, 3)),
                    "FadeOverTimeDelayed" => {
                        (int_at(a, 1), num_arg(a, 2), num_arg(a, 3), int_at(a, 4))
                    }
                    _ => (0, num_arg(a, 1), 0.0, int_at(a, 2)),
                };
                let mut rgba = current_color(&h2.borrow(), i);
                rgba[3] = alpha as u8;
                color_over_time(&h2, i, rgba, dur, delay, interp)?;
                Ok(Value::Null)
            }),
        );
    }

    let h2 = hud.clone();
    add_checked(
        "SetClampBounds",
        3,
        ".nn",
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            if let Some(e) = h2.borrow_mut().elements[i].as_mut() {
                e.clamp_bounds = (num_arg(a, 1), num_arg(a, 2));
            }
            Ok(Value::Null)
        }),
    );
    let h2 = hud.clone();
    add_checked(
        "SetOffscreenArrow",
        2,
        ".b",
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            if let Some(e) = h2.borrow_mut().elements[i].as_mut() {
                e.offscreen_arrow = matches!(a[1], Value::Bool(true));
            }
            Ok(Value::Null)
        }),
    );
    // SetBaseAlpha( a ) (0x1804210e0): the base colour's alpha.
    let h2 = hud.clone();
    add_checked(
        "SetBaseAlpha",
        2,
        ".n",
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            if let Some(e) = h2.borrow_mut().elements[i].as_mut() {
                e.base_color[3] = int_at(a, 1) as u8;
            }
            Ok(Value::Null)
        }),
    );
    // SetPulsate( minAlpha, maxAlpha, frequency ) (0x18042c1e0): the pulse
    // keeps its phase when changed while running; ClearPulsate
    // (0x1804258a0) stops it and puts the panel's alpha back to 255.
    let (h2, host) = (hud.clone(), cvm.host.clone());
    add_checked(
        "SetPulsate",
        4,
        ".nnn",
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            let (lo, hi, freq) = (num_arg(a, 1), num_arg(a, 2), num_arg(a, 3));
            let now = host.time();
            let mut h = h2.borrow_mut();
            let Some(e) = h.elements[i].as_mut() else {
                return Ok(Value::Null);
            };
            let start = match e.pulsate {
                None => now,
                Some([_, _, old_freq, old_start]) => {
                    // Keep the phase: whole turns dropped, the rest carried
                    // over to the new frequency.
                    let turns = (now - old_start) * old_freq * 0.159_154_94;
                    let frac = turns - turns.floor();
                    now - (frac * std::f32::consts::TAU) / freq
                }
            };
            e.pulsate = Some([lo, hi - lo, freq, start]);
            Ok(Value::Null)
        }),
    );
    let h2 = hud.clone();
    add(
        "ClearPulsate",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            if let Some(e) = h2.borrow_mut().elements[i].as_mut() {
                e.pulsate = None;
            }
            with_panel(&h2, i, |p| p.panel_alpha = 255.0);
            Ok(Value::Null)
        }),
    );
    // SetAutoText( text, type, value ) (0x18041ff10 -> 0x1804163d0): only
    // a label gets one; the old one goes first.
    let h2 = hud.clone();
    add_checked(
        "SetAutoText",
        4,
        ".snn",
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            let is_label = read_panel(&h2, i, |p| p.control == Control::Label);
            let made = crate::autotext::create(&str_arg(a, 1), int_at(a, 2), num_arg(a, 3));
            let mut h = h2.borrow_mut();
            h.auto_text_order.retain(|&e| e != i);
            if let Some(e) = h.elements[i].as_mut() {
                e.auto_text = None;
            }
            let made = made.map_err(throw)?;
            if let (true, Some(at)) = (is_label, made) {
                if let Some(e) = h.elements[i].as_mut() {
                    e.auto_text = Some(at);
                }
                h.auto_text_order.insert(0, i);
            }
            Ok(Value::Null)
        }),
    );
    // EnableAutoText / DisableAutoText (0x18041ffd0, 0x18041fff0).
    for (name, on) in [("EnableAutoText", true), ("DisableAutoText", false)] {
        let h2 = hud.clone();
        add(
            name,
            1,
            Box::new(move |_, a| {
                let i = this_element(&h2, a)?;
                let mut h = h2.borrow_mut();
                match h.elements[i].as_mut().and_then(|e| e.auto_text.as_mut()) {
                    Some(at) => at.enabled = on,
                    None => return Err(throw(format!("{name}: No auto text currently set."))),
                }
                Ok(Value::Null)
            }),
        );
    }
    // SetClampToScreen( type ) (0x180420a20): 0, 1 or 2.
    let h2 = hud.clone();
    add_checked(
        "SetClampToScreen",
        2,
        ".n",
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            let t = int_at(a, 1);
            if !(0..3).contains(&t) {
                return Err(throw(format!("Invalid screen clamp type ({t}).")));
            }
            if let Some(e) = h2.borrow_mut().elements[i].as_mut() {
                e.clamp_to_screen = t;
            }
            Ok(Value::Null)
        }),
    );
    // SetADSFade / SetFOVFade / SetDistanceFade( four numbers )
    // (0x18042c090, 0x18042c100, 0x18042c170): the ADS fade is on while its
    // first number is below 1, the others while their second is above 0.
    for name in ["SetADSFade", "SetFOVFade", "SetDistanceFade"] {
        let h2 = hud.clone();
        add_checked(
            name,
            5,
            ".nnnn",
            Box::new(move |_, a| {
                let i = this_element(&h2, a)?;
                let v = [1, 2, 3, 4].map(|k| num_arg(a, k));
                let on = if name == "SetADSFade" {
                    v[0] < 1.0
                } else {
                    v[1] > 0.0
                };
                if let Some(e) = h2.borrow_mut().elements[i].as_mut() {
                    let slot = match name {
                        "SetADSFade" => &mut e.ads_fade,
                        "SetFOVFade" => &mut e.fov_fade,
                        _ => &mut e.distance_fade,
                    };
                    *slot = on.then_some(v);
                }
                Ok(Value::Null)
            }),
        );
    }
    // SetRotation( degrees ) (0x1804214c0): image panels only.
    let h2 = hud.clone();
    add_checked(
        "SetRotation",
        2,
        ".n",
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            let (is_image, name) = read_panel(&h2, i, |p| {
                (p.control == Control::ImagePanel, p.name.clone())
            });
            if !is_image {
                return Err(throw(format!("No ImagePanel element with name '{name}'.")));
            }
            let r = num_arg(a, 1);
            with_panel(&h2, i, |p| p.rotation = r);
            Ok(Value::Null)
        }),
    );
    // Progress bar sources (0x180420740, 0x1804207a0, 0x180420830): progress
    // bars only. Remap( inLo, inHi, outLo, outHi ).
    for (name, n, mask) in [
        ("SetBarProgressSource", 2, ".n"),
        ("SetBarProgressSourceEntity", 2, ".."),
        ("SetBarProgressRemap", 5, ".nnnn"),
    ] {
        let h2 = hud.clone();
        add_checked(
            name,
            n,
            mask,
            Box::new(move |_, a| {
                let i = this_element(&h2, a)?;
                let (is_bar, pname) = read_panel(&h2, i, |p| {
                    (p.control == Control::ProgressBar, p.name.clone())
                });
                if !is_bar {
                    return Err(throw(format!(
                        "Hud element is not a progress bar. ({pname})"
                    )));
                }
                let mut h = h2.borrow_mut();
                let panel = h.elements[i].as_ref().map(|e| e.panel).unwrap_or(0);
                if let Some(p) = h.ui.panel_mut(panel) {
                    match name {
                        "SetBarProgressSource" => p.bar.source = int_at(a, 1),
                        "SetBarProgressSourceEntity" => {
                            p.bar.source_entity = crate::entities::entity_id(&a[1])
                        }
                        _ => p.bar.remap = Some([1, 2, 3, 4].map(|k| num_arg(a, k))),
                    }
                }
                Ok(Value::Null)
            }),
        );
    }

    // SetAlpha( a ) (0x180425b80): the current colour with a new alpha.
    let h2 = hud.clone();
    add(
        "SetAlpha",
        2,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            cancel(&h2, i, &[Var::FgColor]);
            let mut h = h2.borrow_mut();
            let mut c = current_color(&h, i);
            c[3] = num_arg(a, 1) as i32 as u8;
            set_color(&mut h, i, c);
            Ok(Value::Null)
        }),
    );
    let h2 = hud.clone();
    add(
        "GetAlpha",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            Ok(Value::Int(current_color(&h2.borrow(), i)[3] as i32))
        }),
    );
    let h2 = hud.clone();
    add(
        "GetColor",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            let c = current_color(&h2.borrow(), i);
            Ok(Value::Array(new_ref(
                c.iter().map(|&v| Value::Int(v as i32)).collect(),
            )))
        }),
    );
    // ReturnToBaseColor (0x180425a30).
    let h2 = hud.clone();
    add(
        "ReturnToBaseColor",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            cancel(&h2, i, &[Var::FgColor]);
            let mut h = h2.borrow_mut();
            let c = h.elements[i]
                .as_ref()
                .map(|e| e.base_color)
                .unwrap_or_default();
            set_color(&mut h, i, c);
            Ok(Value::Null)
        }),
    );
    let h2 = hud.clone();
    add(
        "GetBaseColor",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            let c = h2.borrow().elements[i]
                .as_ref()
                .map(|e| e.base_color)
                .unwrap_or_default();
            Ok(Value::Array(new_ref(
                c.iter().map(|&v| Value::Int(v as i32)).collect(),
            )))
        }),
    );
    let h2 = hud.clone();
    add(
        "GetBaseAlpha",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            let c = h2.borrow().elements[i]
                .as_ref()
                .map(|e| e.base_color)
                .unwrap_or_default();
            Ok(Value::Int(c[3] as i32))
        }),
    );

    // Position and size (0x180420d90 SetPos, 0x180420190 GetPos,
    // 0x1804213d0 SetSize, 0x1804200c0/d0 GetWidth/GetHeight, 0x1804202d0
    // GetBasePos).
    let h2 = hud.clone();
    add(
        "SetPos",
        3,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            cancel(&h2, i, &[Var::XPos, Var::YPos]);
            let (x, y) = (num_arg(a, 1) as i32, num_arg(a, 2) as i32);
            if x.abs() > 0x8000 || y.abs() > 0x8000 {
                return Err(throw(format!(
                    "Hud element position [{x}, {y}] is out of range [32768, 32768]"
                )));
            }
            with_panel(&h2, i, |p| (p.x, p.y) = (x as f32, y as f32));
            Ok(Value::Null)
        }),
    );
    let h2 = hud.clone();
    add(
        "GetPos",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            let (x, y) = read_panel(&h2, i, |p| (p.x, p.y));
            Ok(int_pair(x, y))
        }),
    );
    let h2 = hud.clone();
    add(
        "SetSize",
        3,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            cancel(&h2, i, &[Var::Wide, Var::Tall]);
            let (w, hgt) = (num_arg(a, 1) as i32 as f32, num_arg(a, 2) as i32 as f32);
            with_panel(&h2, i, |p| (p.w, p.h) = (w, hgt));
            Ok(Value::Null)
        }),
    );
    let h2 = hud.clone();
    add(
        "SetWidth",
        2,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            cancel(&h2, i, &[Var::Wide]);
            let w = num_arg(a, 1) as i32 as f32;
            with_panel(&h2, i, |p| p.w = w);
            Ok(Value::Null)
        }),
    );
    let h2 = hud.clone();
    add(
        "SetHeight",
        2,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            cancel(&h2, i, &[Var::Tall]);
            let hh = num_arg(a, 1) as i32 as f32;
            with_panel(&h2, i, |p| p.h = hh);
            Ok(Value::Null)
        }),
    );
    let h2 = hud.clone();
    add(
        "GetSize",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            let (w, hh) = read_panel(&h2, i, |p| (p.w, p.h));
            Ok(int_pair(w, hh))
        }),
    );
    let h2 = hud.clone();
    add(
        "GetWidth",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            Ok(Value::Int(read_panel(&h2, i, |p| p.w) as i32))
        }),
    );
    let h2 = hud.clone();
    add(
        "GetHeight",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            Ok(Value::Int(read_panel(&h2, i, |p| p.h) as i32))
        }),
    );
    type Base = fn(&Element) -> Value;
    let bases: [(&'static str, Base); 7] = [
        ("GetBasePos", |e| int_pair(e.base_pos.0, e.base_pos.1)),
        ("GetBaseSize", |e| int_pair(e.base_size.0, e.base_size.1)),
        ("GetBaseX", |e| Value::Int(e.base_pos.0 as i32)),
        ("GetBaseY", |e| Value::Int(e.base_pos.1 as i32)),
        ("GetBaseWidth", |e| Value::Int(e.base_size.0 as i32)),
        ("GetBaseHeight", |e| Value::Int(e.base_size.1 as i32)),
        ("IsLabel", |_| Value::Null),
    ];
    for (name, f) in bases.into_iter().take(6) {
        let h2 = hud.clone();
        add(
            name,
            1,
            Box::new(move |_, a| {
                let i = this_element(&h2, a)?;
                Ok(h2.borrow().elements[i].as_ref().map_or(Value::Null, f))
            }),
        );
    }
    let h2 = hud.clone();
    add(
        "IsLabel",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            Ok(Value::Bool(read_panel(&h2, i, |p| {
                p.control == Control::Label
            })))
        }),
    );
    let h2 = hud.clone();
    add(
        "ReturnToBasePos",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            cancel(&h2, i, &[Var::XPos, Var::YPos]);
            let b = h2.borrow().elements[i]
                .as_ref()
                .map(|e| e.base_pos)
                .unwrap_or_default();
            with_panel(&h2, i, |p| (p.x, p.y) = b);
            Ok(Value::Null)
        }),
    );
    let h2 = hud.clone();
    add(
        "ReturnToBaseSize",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            cancel(&h2, i, &[Var::Wide, Var::Tall, Var::Wide, Var::Tall]);
            let b = h2.borrow().elements[i]
                .as_ref()
                .map(|e| e.base_size)
                .unwrap_or_default();
            with_panel(&h2, i, |p| (p.w, p.h) = b);
            Ok(Value::Null)
        }),
    );

    // SetBarProgress( fraction ) (0x180420390): progress bars only.
    let h2 = hud.clone();
    add(
        "SetBarProgress",
        2,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            let v = num_arg(a, 1);
            let mut h = h2.borrow_mut();
            let panel = h.elements[i].as_ref().map(|e| e.panel).unwrap_or(0);
            let p =
                h.ui.panel_mut(panel)
                    .ok_or_else(|| throw("invalid panel"))?;
            if p.control != Control::ProgressBar {
                return Err(throw(format!(
                    "Hud element is not a progress bar. ({})",
                    p.name
                )));
            }
            p.bar_progress = v;
            Ok(Value::Null)
        }),
    );
    let h2 = hud.clone();
    add(
        "GetBarProgress",
        1,
        Box::new(move |_, a| {
            let i = this_element(&h2, a)?;
            Ok(Value::Float(read_panel(&h2, i, |p| p.bar_progress)))
        }),
    );

    let class = crate::classes::register(
        &mut cvm.vm,
        "CClientHudElement",
        "CClientScriptHudElement",
        None,
        m.borrow().shallow_clone(),
    );
    hud.borrow_mut().class = Some(class);

    // The Hud object (CClientScriptHud).
    let mut hm = Table::new();
    let host = cvm.host.clone();
    hm.insert(
        Value::str("GetScreenSize"),
        native("GetScreenSize", 1, move |_, _| {
            let (w, h) = host.screen_size();
            Ok(int_pair(w as f32, h as f32))
        }),
    );
    // HudElement( name ) (0x18042a000): a direct child of the HUD's root
    // panel, wrapped (an error if missing or already wrapped).
    let h2 = hud.clone();
    hm.insert(
        Value::str("HudElement"),
        pf_squirrel::native_checked("HudElement", 2, ".s", move |vm, a| {
            let root = h2.borrow().ui.hud_root.ok_or_else(|| throw("no HUD"))?;
            child_element(&h2, vm, root, &str_arg(a, 1))
        }),
    );
    // GetOrCreateHudElement( name ) (0x18042a890): the child's existing
    // element, or a new one. (A missing child would crash the game.)
    let h2 = hud.clone();
    hm.insert(
        Value::str("GetOrCreateHudElement"),
        pf_squirrel::native_checked("GetOrCreateHudElement", 2, ".s", move |vm, a| {
            let name = str_arg(a, 1);
            let child = {
                let h = h2.borrow();
                let root = h.ui.hud_root.ok_or_else(|| throw("no HUD"))?;
                h.ui.find_child(root, &name)
            };
            let Some(child) = child else {
                return Err(throw(format!("HudElement \"{name}\" could not be found")));
            };
            let existing = h2.borrow().ui.panel(child).and_then(|p| p.element);
            if let Some(e) = existing {
                return Ok(h2.borrow().elements[e]
                    .as_ref()
                    .map_or(Value::Null, |e| e.instance.clone()));
            }
            create_element(&h2, vm, child)
        }),
    );
    let hud_class = crate::classes::register(
        &mut cvm.vm,
        "CClientScriptHud",
        "CClientScriptHud",
        None,
        hm,
    );
    let hud_obj = crate::classes::instantiate(&mut cvm.vm, &hud_class, crate::classes::user(()))
        .unwrap_or_default();
    cvm.vm.root.borrow_mut().insert(Value::str("Hud"), hud_obj);

    hud
}

fn with_panel(hud: &SharedHud, i: usize, f: impl FnOnce(&mut pf_vgui::Panel)) {
    let mut h = hud.borrow_mut();
    let panel = h.elements[i].as_ref().map(|e| e.panel).unwrap_or(0);
    if let Some(p) = h.ui.panel_mut(panel) {
        f(p);
    }
}

fn read_panel<T: Default>(hud: &SharedHud, i: usize, f: impl FnOnce(&pf_vgui::Panel) -> T) -> T {
    let h = hud.borrow();
    let panel = h.elements[i].as_ref().map(|e| e.panel).unwrap_or(0);
    h.ui.panel(panel).map(f).unwrap_or_default()
}

/// Cancel running animations of these variables on the element's panel
/// (one per entry, as the game's cancel removes the first match).
fn cancel(hud: &SharedHud, i: usize, vars: &[Var]) {
    let mut h = hud.borrow_mut();
    let panel = h.elements[i].as_ref().map(|e| e.panel).unwrap_or(0);
    for v in vars {
        h.anim.cancel(panel, v);
    }
}

/// An interpolator number from script, or the game's error.
fn curve(n: i32) -> Result<Curve, NativeError> {
    Curve::from_index(n).ok_or_else(|| throw(format!("Invalid animation interpolator ({n}).")))
}

fn int_at(a: &[Value], i: usize) -> i32 {
    match a.get(i) {
        Some(Value::Int(n)) => *n,
        Some(Value::Float(x)) => *x as i32,
        _ => 0,
    }
}

/// Move to (x, y) over time (client.dll 0x180421ab0): both within ±32768;
/// running x and y animations are cancelled, then y and x run.
fn move_over_time(
    hud: &SharedHud,
    i: usize,
    x: i32,
    y: i32,
    dur: f32,
    interp: i32,
) -> Result<(), NativeError> {
    let c = curve(interp)?;
    if !(-0x8000..=0x8000).contains(&x) || !(-0x8000..=0x8000).contains(&y) {
        return Err(throw(format!(
            "MoveOverTime: position ({x}, {y}) is out of range (32768)"
        )));
    }
    cancel(hud, i, &[Var::XPos, Var::YPos]);
    let mut h = hud.borrow_mut();
    let panel = h.elements[i].as_ref().map(|e| e.panel).unwrap_or(0);
    h.anim.run(
        panel,
        Var::YPos,
        [y as f32, 0.0, 0.0, 0.0],
        0.0,
        dur,
        c,
        1.0,
    );
    h.anim.run(
        panel,
        Var::XPos,
        [x as f32, 0.0, 0.0, 0.0],
        0.0,
        dur,
        c,
        1.0,
    );
    Ok(())
}

/// Scale to the base size times (sx, sy) over time (0x180421f90).
fn scale_over_time(
    hud: &SharedHud,
    i: usize,
    sx: f32,
    sy: f32,
    dur: f32,
    interp: i32,
) -> Result<(), NativeError> {
    let c = curve(interp)?;
    cancel(hud, i, &[Var::Wide, Var::Tall]);
    let mut h = hud.borrow_mut();
    let (panel, (bw, bh)) = h.elements[i]
        .as_ref()
        .map(|e| (e.panel, e.base_size))
        .unwrap_or_default();
    h.anim.run(
        panel,
        Var::Wide,
        [(bw as i32) as f32 * sx, 0.0, 0.0, 0.0],
        0.0,
        dur,
        c,
        1.0,
    );
    h.anim.run(
        panel,
        Var::Tall,
        [(bh as i32) as f32 * sy, 0.0, 0.0, 0.0],
        0.0,
        dur,
        c,
        1.0,
    );
    Ok(())
}

/// Colour over time (0x180426870): FgColor to `rgba` after `delay`.
fn color_over_time(
    hud: &SharedHud,
    i: usize,
    rgba: Rgba,
    dur: f32,
    delay: f32,
    interp: i32,
) -> Result<(), NativeError> {
    let c = curve(interp)?;
    cancel(hud, i, &[Var::FgColor]);
    let mut h = hud.borrow_mut();
    let panel = h.elements[i].as_ref().map(|e| e.panel).unwrap_or(0);
    let end = rgba.map(|v| v as f32);
    h.anim.run(panel, Var::FgColor, end, delay, dur, c, 1.0);
    Ok(())
}

fn current_color(h: &HudState, i: usize) -> Rgba {
    let panel = h.elements[i].as_ref().map(|e| e.panel).unwrap_or(0);
    h.ui.panel(panel).map(|p| p.fg_color).unwrap_or_default()
}

fn set_color(h: &mut HudState, i: usize, c: Rgba) {
    let panel = h.elements[i].as_ref().map(|e| e.panel).unwrap_or(0);
    if let Some(p) = h.ui.panel_mut(panel) {
        p.fg_color = c;
    }
}

/// The colour arguments of `SetColor`/`SetColorBG` (client.dll
/// 0x1804295f0): `( [r, g, b] )`, `( [r, g, b, a] )`, `( [r, g, b], a )`,
/// `( r, g, b )` or `( r, g, b, a )`, each read as an integer and kept to
/// its low 8 bits. A missing alpha is the element's current foreground (or
/// draw) colour's alpha, for either call.
fn color_args(a: &[Value], hud: &SharedHud, i: usize) -> Result<Rgba, NativeError> {
    let byte = |v: &Value| match v {
        Value::Int(n) => *n as u8,
        Value::Float(x) => *x as i32 as u8,
        _ => 0,
    };
    let alpha = current_color(&hud.borrow(), i)[3];
    let from_array = |v: &Value, sizes: &[usize]| -> Result<Vec<u8>, NativeError> {
        let Value::Array(arr) = v else {
            return Err(throw(
                "SetColor/SetColorBG called with invalid argument type; must be array.",
            ));
        };
        let arr = arr.borrow();
        if !sizes.contains(&arr.len()) {
            return Err(throw(
                "SetColor/SetColorBG called with invalid number of elements in array.",
            ));
        }
        Ok(arr.iter().map(byte).collect())
    };
    let c = match a.len() {
        2 => from_array(&a[1], &[3, 4])?,
        3 => {
            let mut c = from_array(&a[1], &[3])?;
            c.push(byte(&a[2]));
            c
        }
        4 | 5 => a[1..].iter().map(byte).collect(),
        _ => {
            return Err(throw(
                "SetColor/SetColorBG called with invalid number of parameters.",
            ));
        }
    };
    Ok([c[0], c[1], c[2], c.get(3).copied().unwrap_or(alpha)])
}

/// `CreateClientsideVGuiScreen( name, pass, origin, angles, width, height )`
/// (0x1802b5250) and `C_VGuiScreen.GetPanel()` (0x1800f0920).
pub fn install_screens(cvm: &mut ClientVm, hud: &SharedHud, ents: &entities::Shared) {
    let host = cvm.host.clone();
    let h2 = hud.clone();
    let e2 = ents.clone();
    cvm.vm
        .register("CreateClientsideVGuiScreen", 7, move |vm, a| {
            let name = str_arg(a, 1);
            let screen = {
                let mut h = h2.borrow_mut();
                let read = |p: &str| host.read_script(p);
                h.ui.create_screen(&name, &read)
            };
            let Some(screen) = screen else {
                return Ok(Value::Null);
            };
            let id = Entities::create_client_only(
                &e2,
                EntityInfo {
                    script_class: "C_VGuiScreen".into(),
                    classname: "vgui_screen".into(),
                    signifier: "vgui_screen".into(),
                    ..Default::default()
                },
            );
            h2.borrow_mut().screens.push((id, screen, None));
            Ok(Entities::instance(&e2, vm, &*host, id))
        });

    let h2 = hud.clone();
    let get_panel = native("GetPanel", 1, move |vm, a| {
        let Some(id) = entities::entity_id(&a[0]) else {
            return Err(throw("Failed to find panel"));
        };
        let found = h2.borrow().screens.iter().position(|s| s.0 == id);
        let Some(k) = found else {
            return Err(throw("Failed to find panel"));
        };
        let (screen, existing) = {
            let h = h2.borrow();
            (h.screens[k].1, h.screens[k].2)
        };
        if let Some(e) = existing {
            return Ok(h2.borrow().elements[e]
                .as_ref()
                .map_or(Value::Null, |e| e.instance.clone()));
        }
        let root = h2.borrow().ui.screens[screen].root;
        let v = create_element(&h2, vm, root)?;
        h2.borrow_mut().screens[k].2 = element_index(&v);
        Ok(v)
    });
    let class = ents.borrow().classes.get("C_VGuiScreen").cloned();
    if let Some(c) = class {
        c.borrow_mut()
            .members
            .insert(Value::str("GetPanel"), get_panel);
    }
}

/// Crosshair_SetState( state ) (client.dll 0x1802b5850).
pub fn install_crosshair(cvm: &mut ClientVm, hud: &SharedHud) {
    let h2 = hud.clone();
    cvm.vm
        .register_checked("Crosshair_SetState", 2, ".n", move |_, a| {
            let s = int_at(a, 1);
            if !(0..3).contains(&s) {
                return Err(throw(format!(
                    "Enum state value {s} does not index CROSSHAIR_STATE_COUNT"
                )));
            }
            h2.borrow_mut().crosshair_state = s;
            Ok(Value::Null)
        });
}

impl ClientVm {
    /// Advance the HUD's animations to `real_time` (seconds of real time:
    /// the game runs its HUD controller on `Plat_FloatTime`, not game
    /// time).
    pub fn hud_frame(&mut self, real_time: f32) {
        if let Some(hud) = &self.hud {
            let mut h = hud.borrow_mut();
            let h = &mut *h;
            h.anim.update(&mut h.ui, real_time);
        }
    }
}
