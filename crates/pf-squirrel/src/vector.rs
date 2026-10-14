//! The `Vector` class launcher.dll gives every VM (docs/port/scripts.md,
//! "Vector"): a class whose instances hold three 32-bit floats. Ported from
//! launcher.dll, keeping the order of float operations so results round
//! the same way.
//!
//! Angles are (pitch, yaw, roll) in degrees, Source style: forward is +x,
//! left is +y, up is +z.

use std::rc::Rc;

use crate::fmt::{Flags, format_float};
use crate::table::Table;
use crate::value::*;
use crate::vm::{NativeResult, Vm, throw};

/// What a Vector instance carries.
fn get(v: &Value) -> Option<[f32; 3]> {
    match v {
        Value::Instance(i) => match i.borrow().user {
            UserData::Vector(xyz) => Some(xyz),
            _ => None,
        },
        _ => None,
    }
}

fn this_vec(a: &[Value]) -> Result<[f32; 3], crate::vm::NativeError> {
    match &a[0] {
        Value::Instance(_) => get(&a[0]).ok_or_else(|| throw("null vector")),
        _ => Err(throw("the object is not a class instance")),
    }
}

fn set(v: &Value, xyz: [f32; 3]) {
    if let Value::Instance(i) = v {
        i.borrow_mut().user = UserData::Vector(xyz);
    }
}

/// The game's sine and cosine together (launcher.dll): the angle in turns,
/// folded into a quarter turn, then a degree-9 odd polynomial. Cosine is the
/// sine of the angle plus a quarter turn. Not the C library's, so not quite
/// `f32::sin`.
pub fn sin_cos(x: f32) -> (f32, f32) {
    (game_sin(x), game_sin(x + 1.570_796_4))
}

fn game_sin(x: f32) -> f32 {
    const INV_TAU: f32 = 0.159_154_94;
    let t = x * INV_TAU;
    let a = t.abs();
    // Fraction of the turn, keeping the sign of t.
    let r = (a + 8_388_608.0) - 8_388_608.0;
    let floor = if a < r { r - 1.0 } else { r };
    let f = f32::from_bits((a - floor).to_bits() ^ a.to_bits() ^ t.to_bits());
    // Fold into [-0.25, 0.25].
    let m = f.min(0.5 - f);
    let m = m.max(-0.5 - m);
    let m = m.min(0.5 - m);
    let s = m * m;
    // The coefficients as stored in launcher.dll (the last is just below
    // 2*pi; kept as is).
    let [c4, c3, c2, c1, c0] = [
        0x421e_2596u32,
        0xc299_197d,
        0x42a3_33b7,
        0xc225_5ddb,
        0x40c9_0fda,
    ]
    .map(f32::from_bits);
    ((((c4 * s + c3) * s + c2) * s + c1) * s + c0) * m
}

fn sin_cos3(angles: [f32; 3], scale: f32) -> ([f32; 3], [f32; 3]) {
    let mut s = [0.0; 3];
    let mut c = [0.0; 3];
    for i in 0..3 {
        (s[i], c[i]) = sin_cos(angles[i] * scale);
    }
    (s, c)
}

const DEG2RAD: f32 = 0.017_453_292;

/// Direction to angles (pitch, yaw, 0), each in [0, 360).
pub fn vector_angles(v: [f32; 3]) -> [f32; 3] {
    let (pitch, yaw);
    if v[1] == 0.0 && v[0] == 0.0 {
        yaw = 0.0;
        pitch = if v[2] <= 0.0 { 90.0 } else { 270.0 };
    } else {
        let mut y = v[1].atan2(v[0]) * 180.0 * 0.318_309_87;
        if y < 0.0 {
            y += 360.0;
        }
        yaw = y;
        let mut p = (-v[2]).atan2((v[0] * v[0] + v[1] * v[1]).sqrt()) * 180.0 * 0.318_309_87;
        if p < 0.0 {
            p += 360.0;
        }
        pitch = p;
    }
    [pitch, yaw, 0.0]
}

/// Forward from (pitch, yaw), with the C library's sine and cosine (as
/// `AnglesToForward` does in the game).
pub fn angles_forward(a: [f32; 3]) -> [f32; 3] {
    let (cp, sp) = ((a[0] * DEG2RAD).cos(), (a[0] * DEG2RAD).sin());
    let (cy, sy) = ((a[1] * DEG2RAD).cos(), (a[1] * DEG2RAD).sin());
    [cy * cp, sy * cp, -sp]
}

/// Right and up from angles (the game's sine and cosine). Right points to
/// the right (−y at zero angles).
pub fn angles_right_up(a: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let ([sp, sy, sr], [cp, cy, cr]) = sin_cos3(a, DEG2RAD);
    let right = [cr * sy - sr * sp * cy, -cr * cy - sr * sp * sy, -sr * cp];
    let up = [cr * sp * cy + sr * sy, cr * sp * sy - cy * sr, cr * cp];
    (right, up)
}

/// The rotation matrix of some angles (rows, 3x3).
fn angle_matrix(a: [f32; 3]) -> [[f32; 3]; 3] {
    let ([sp, sy, sr], [cp, cy, cr]) = sin_cos3(a, DEG2RAD);
    [
        [cy * cp, sr * sp * cy - cr * sy, cr * sp * cy + sr * sy],
        [cp * sy, sr * sp * sy + cr * cy, cr * sp * sy - cy * sr],
        [-sp, cp * sr, cr * cp],
    ]
}

type Quat = [f32; 4];

fn angle_quat(a: [f32; 3]) -> Quat {
    let ([sp, sy, sr], [cp, cy, cr]) = sin_cos3(a, 0.008_726_646);
    [
        cy * cp * sr - sy * cr * sp,
        cy * cr * sp + sy * cp * sr,
        sy * cr * cp - cy * sr * sp,
        cy * cr * cp + sy * sr * sp,
    ]
}

/// `q` or `-q`, whichever is nearer `p`.
fn quat_align(p: &Quat, q: &Quat) -> Quat {
    let d: Vec<f32> = (0..4).map(|i| p[i] - q[i]).collect();
    let s: Vec<f32> = (0..4).map(|i| q[i] + p[i]).collect();
    if s[0] * s[0] + s[1] * s[1] + s[2] * s[2] + s[3] * s[3]
        < d[0] * d[0] + d[1] * d[1] + d[2] * d[2] + d[3] * d[3]
    {
        [-q[0], -q[1], -q[2], -q[3]]
    } else {
        *q
    }
}

fn quat_mult(p: &Quat, q: &Quat) -> Quat {
    let q = quat_align(p, q);
    [
        ((q[2] * p[1] + q[3] * p[0]) - q[1] * p[2]) + q[0] * p[3],
        (q[3] * p[1] - q[2] * p[0]) + q[0] * p[2] + q[1] * p[3],
        (q[1] * p[0] - q[0] * p[1]) + q[3] * p[2] + q[2] * p[3],
        q[3] * p[3] - (q[1] * p[1] + q[0] * p[0] + q[2] * p[2]),
    ]
}

fn quat_invert(q: &Quat) -> Quat {
    let mut out = [-q[0], -q[1], -q[2], q[3]];
    let n = q[1] * q[1] + q[0] * q[0] + q[2] * q[2] + q[3] * q[3];
    if n != 0.0 {
        let inv = 1.0 / n;
        out = [inv * -q[0], inv * -q[1], inv * -q[2], inv * out[3]];
    }
    out
}

fn quat_slerp(p: &Quat, q: &Quat, t: f32) -> Quat {
    let q = quat_align(p, q);
    let cos = q[1] * p[1] + q[0] * p[0] + p[2] * q[2] + p[3] * q[3];
    if cos + 1.0 <= 1e-6 {
        let mut out = [-q[1], q[0], -q[3], q[2]];
        let sclp = ((1.0 - t) * 1.570_796_4).sin();
        let sclq = (t * 1.570_796_4).sin();
        out[0] = sclp * p[0] + sclq * out[0];
        out[1] = sclq * out[1] + sclp * p[1];
        out[2] = sclq * out[2] + sclp * p[2];
        out
    } else {
        let (sclp, sclq) = if 1.0 - cos <= 1e-6 {
            (1.0 - t, t)
        } else {
            let omega = cos.acos();
            let sinom = omega.sin();
            (
                ((1.0 - t) * omega).sin() * (1.0 / sinom),
                (omega * t).sin() * (1.0 / sinom),
            )
        };
        [
            q[0] * sclq + p[0] * sclp,
            sclq * q[1] + sclp * p[1],
            sclq * q[2] + sclp * p[2],
            sclq * q[3] + sclp * p[3],
        ]
    }
}

/// Quaternion to angles, through the rotation matrix.
fn quat_angles(q: &Quat) -> [f32; 3] {
    let x2 = q[0] * 2.0;
    let w2 = q[3] * 2.0;
    let yy2 = q[1] * 2.0 * q[1];
    let zz2 = q[2] * 2.0 * q[2];
    let xx = 1.0 - x2 * q[0];
    let m0 = (1.0 - yy2) - zz2;
    let m4 = x2 * q[1] + w2 * q[2];
    let m1 = x2 * q[1] - w2 * q[2];
    let m8 = x2 * q[2] - w2 * q[1];
    let m5 = xx - zz2;
    let m10 = xx - yy2;
    let m9 = w2 * q[0] + q[1] * 2.0 * q[2];
    // MatrixAngles.
    let xy = (m4 * m4 + m0 * m0).sqrt();
    if xy <= 0.001 {
        [
            (-m8).atan2(xy) * 57.295_776,
            (-m1).atan2(m5) * 57.295_776,
            0.0,
        ]
    } else {
        [
            (-m8).atan2(xy) * 57.295_776,
            m4.atan2(m0) * 57.295_776,
            m9.atan2(m10) * 57.295_776,
        ]
    }
}

fn arg_vec(a: &[Value], i: usize) -> Result<[f32; 3], crate::vm::NativeError> {
    a.get(i)
        .and_then(get)
        .ok_or_else(|| throw("expected a Vector"))
}

/// Install the class as `Vector` in the root table.
pub(crate) fn install(vm: &mut Vm) {
    let class = new_ref(Class {
        base: None,
        members: Table::new(),
        locked: false,
    });
    let class_value = Value::Class(class.clone());
    let make = move |_: &mut Vm, xyz: [f32; 3]| -> Value {
        Value::Instance(new_ref(Instance {
            class: class.clone(),
            values: Table::new(),
            user: UserData::Vector(xyz),
        }))
    };
    let make = Rc::new(make);
    let mut m = Table::new();
    type Body = Box<dyn Fn(&mut Vm, &[Value]) -> NativeResult>;
    let mut add = |name: &str, n: i32, f: Body| {
        let f: NativeFnPtr = Rc::from(f);
        m.insert(
            Value::str(name),
            Value::Native(Rc::new(NativeFn {
                name: name.into(),
                nparams: n,
                f,
                env: None,
            })),
        );
    };

    // Missing or non-number arguments are 0.
    add(
        "constructor",
        0,
        Box::new(|_, a| {
            let mut xyz = [0.0; 3];
            for (i, v) in a.iter().skip(1).take(3).enumerate() {
                xyz[i] = match v {
                    Value::Int(n) => *n as f32,
                    Value::Float(x) => *x,
                    _ => 0.0,
                };
            }
            match &a[0] {
                Value::Instance(_) => set(&a[0], xyz),
                _ => return Err(throw("the object is not a class instance")),
            }
            Ok(Value::Null)
        }),
    );
    add(
        "_get",
        2,
        Box::new(|vm, a| {
            let xyz = this_vec(a)?;
            let Value::Str(k) = &a[1] else {
                let got = vm.plain_string(&a[1]);
                return Err(throw(format!(
                    "wrong argument type, expected 'string' got '{got:.50}'"
                )));
            };
            match k.bytes() {
                [c @ b'x'..=b'z'] => Ok(Value::Float(xyz[(c - b'x') as usize])),
                _ => Err(throw(format!(
                    "the index '{}' does not exist",
                    k.to_string_lossy()
                ))),
            }
        }),
    );
    add(
        "_set",
        3,
        Box::new(|vm, a| {
            let mut xyz = this_vec(a)?;
            let Value::Str(k) = &a[1] else {
                let got = vm.plain_string(&a[1]);
                return Err(throw(format!(
                    "wrong argument type, expected 'string' got '{got:.50}'"
                )));
            };
            let x = match a[2] {
                Value::Int(n) => n as f32,
                Value::Float(x) => x,
                _ => return Err(throw("expected a number")),
            };
            match k.bytes() {
                [c @ b'x'..=b'z'] => {
                    xyz[(c - b'x') as usize] = x;
                    set(&a[0], xyz);
                    Ok(Value::Float(x))
                }
                _ => Err(throw(format!(
                    "the index '{}' does not exist",
                    k.to_string_lossy()
                ))),
            }
        }),
    );
    add(
        "_tostring",
        0,
        Box::new(|_, a| {
            let [x, y, z] = this_vec(a)?;
            let f = |v: f32| format_float(v as f64, b'f', None, &Flags::default());
            Ok(Value::str(&format!(
                "(vector : ({}, {}, {}))",
                f(x),
                f(y),
                f(z)
            )))
        }),
    );
    add("_typeof", 0, Box::new(|_, _| Ok(Value::str("Vector"))));
    // foreach over a Vector: x, y, z.
    add(
        "_nexti",
        0,
        Box::new(|_, a| {
            Ok(match a.get(1) {
                None | Some(Value::Null) => Value::str("x"),
                Some(Value::Str(s)) if s.bytes() == b"x" => Value::str("y"),
                Some(Value::Str(s)) if s.bytes() == b"y" => Value::str("z"),
                _ => Value::Null,
            })
        }),
    );
    let mk = make.clone();
    add(
        "_add",
        2,
        Box::new(move |vm, a| {
            let p = this_vec(a)?;
            let q = arg_vec(a, 1)?;
            Ok(mk(vm, [p[0] + q[0], p[1] + q[1], p[2] + q[2]]))
        }),
    );
    let mk = make.clone();
    add(
        "_sub",
        2,
        Box::new(move |vm, a| {
            let p = this_vec(a)?;
            let q = arg_vec(a, 1)?;
            Ok(mk(vm, [p[0] - q[0], p[1] - q[1], p[2] - q[2]]))
        }),
    );
    let mk = make.clone();
    add(
        "_mul",
        2,
        Box::new(move |vm, a| {
            let p = this_vec(a)?;
            let s = match a[1] {
                Value::Int(n) => n as f32,
                Value::Float(x) => x,
                _ => {
                    return Err(throw(
                        "Tried to scale vector by something other than a number",
                    ));
                }
            };
            Ok(mk(vm, [s * p[0], s * p[1], s * p[2]]))
        }),
    );
    add(
        "ToKVString",
        0,
        Box::new(|_, a| {
            let [x, y, z] = this_vec(a)?;
            let f = |v: f32| format_float(v as f64, b'f', None, &Flags::default());
            Ok(Value::str(&format!("{} {} {}", f(x), f(y), f(z))))
        }),
    );
    add(
        "Length",
        0,
        Box::new(|_, a| {
            let [x, y, z] = this_vec(a)?;
            Ok(Value::Float((x * x + y * y + z * z).sqrt()))
        }),
    );
    add(
        "LengthSqr",
        0,
        Box::new(|_, a| {
            let [x, y, z] = this_vec(a)?;
            Ok(Value::Float(x * x + y * y + z * z))
        }),
    );
    add(
        "Length2D",
        0,
        Box::new(|_, a| {
            let [x, y, _] = this_vec(a)?;
            Ok(Value::Float((x * x + y * y).sqrt()))
        }),
    );
    add(
        "Length2DSqr",
        0,
        Box::new(|_, a| {
            let [x, y, _] = this_vec(a)?;
            Ok(Value::Float(x * x + y * y))
        }),
    );
    add(
        "Dot",
        2,
        Box::new(|_, a| {
            let p = this_vec(a)?;
            let q = arg_vec(a, 1)?;
            Ok(Value::Float(p[0] * q[0] + p[1] * q[1] + p[2] * q[2]))
        }),
    );
    let mk = make.clone();
    add(
        "Cross",
        2,
        Box::new(move |vm, a| {
            let p = this_vec(a)?;
            let q = arg_vec(a, 1)?;
            Ok(mk(
                vm,
                [
                    p[1] * q[2] - p[2] * q[1],
                    p[2] * q[0] - p[0] * q[2],
                    p[0] * q[1] - p[1] * q[0],
                ],
            ))
        }),
    );
    // Norm and Normalize: normalize in place, return the old length.
    for name in ["Norm", "Normalize"] {
        add(
            name,
            0,
            Box::new(|_, a| {
                let [x, y, z] = this_vec(a)?;
                let len = (x * x + y * y + z * z).sqrt();
                let inv = 1.0 / (len + f32::MIN_POSITIVE);
                set(&a[0], [x * inv, y * inv, z * inv]);
                Ok(Value::Float(len))
            }),
        );
    }
    let mk = make.clone();
    add(
        "GetAngles",
        0,
        Box::new(move |vm, a| Ok(mk(vm, vector_angles(this_vec(a)?)))),
    );
    let mk = make.clone();
    add(
        "AnglesToForward",
        0,
        Box::new(move |vm, a| Ok(mk(vm, angles_forward(this_vec(a)?)))),
    );
    let mk = make.clone();
    add(
        "AnglesToRight",
        0,
        Box::new(move |vm, a| Ok(mk(vm, angles_right_up(this_vec(a)?).0))),
    );
    let mk = make.clone();
    add(
        "AnglesToUp",
        0,
        Box::new(move |vm, a| Ok(mk(vm, angles_right_up(this_vec(a)?).1))),
    );
    let mk = make.clone();
    add(
        "AnglesCompose",
        2,
        Box::new(move |vm, a| {
            let p = angle_quat(this_vec(a)?);
            let q = angle_quat(arg_vec(a, 1)?);
            Ok(mk(vm, quat_angles(&quat_mult(&p, &q))))
        }),
    );
    let mk = make.clone();
    add(
        "AnglesInverse",
        0,
        Box::new(move |vm, a| {
            let q = angle_quat(this_vec(a)?);
            Ok(mk(vm, quat_angles(&quat_invert(&q))))
        }),
    );
    let mk = make.clone();
    add(
        "AnglesLerp",
        3,
        Box::new(move |vm, a| {
            let p = angle_quat(this_vec(a)?);
            let q = angle_quat(arg_vec(a, 1)?);
            let t = match a[2] {
                Value::Int(n) => n as f32,
                Value::Float(x) => x,
                _ => 0.0,
            };
            Ok(mk(vm, quat_angles(&quat_slerp(&p, &q, t))))
        }),
    );
    // This vector rotated by the argument's angles.
    let mk = make.clone();
    add(
        "RotateAngles",
        2,
        Box::new(move |vm, a| {
            let v = this_vec(a)?;
            let m = angle_matrix(arg_vec(a, 1)?);
            Ok(mk(
                vm,
                [
                    m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
                    m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
                    m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
                ],
            ))
        }),
    );
    drop(add);
    if let Value::Class(c) = &class_value {
        c.borrow_mut().members = m;
    }
    vm.root
        .borrow_mut()
        .insert(Value::str("Vector"), class_value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_sine_is_close_to_the_real_one() {
        for i in -40..40 {
            let x = i as f32 * 0.37;
            let (s, c) = sin_cos(x);
            assert!((s - x.sin()).abs() < 2e-6, "sin {x}");
            assert!((c - x.cos()).abs() < 2e-6, "cos {x}");
        }
    }

    #[test]
    fn angles_round_trip() {
        let a = vector_angles([0.0, 1.0, 0.0]);
        assert_eq!(a, [0.0, 90.0, 0.0]);
        let f = angles_forward([0.0, 90.0, 0.0]);
        assert!(f[1] > 0.999 && f[0].abs() < 1e-6);
        let (r, u) = angles_right_up([0.0, 0.0, 0.0]);
        assert!((r[1] + 1.0).abs() < 1e-6 && (u[2] - 1.0).abs() < 1e-6);
        let q = angle_quat([10.0, 20.0, 30.0]);
        let b = quat_angles(&q);
        assert!(
            (b[0] - 10.0).abs() < 1e-3 && (b[1] - 20.0).abs() < 1e-3 && (b[2] - 30.0).abs() < 1e-3
        );
    }
}
