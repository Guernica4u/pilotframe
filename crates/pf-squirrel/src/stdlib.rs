//! The standard functions launcher.dll gives every VM: Squirrel's base,
//! math and string libraries and the methods of each value type
//! (docs/port/scripts.md, "How they're registered"). Behaviour follows the
//! oracle cases; anything not covered yet is noted where it's written.

use std::rc::Rc;

use crate::fmt::{Flags, format_float};
use crate::table::Table;
use crate::value::*;
use crate::vm::{NativeError, NativeResult, Vm, native, throw};

pub fn install(vm: &mut Vm) {
    base(vm);
    math(vm);
    strings(vm);
    vm.builtins.number = number_methods();
    vm.builtins.string = string_methods();
    vm.builtins.array = array_methods();
    vm.builtins.table = table_methods();
    vm.builtins.function = function_methods();
    vm.builtins.class = class_methods();
    vm.builtins.instance = instance_methods();
    vm.builtins.weakref = weakref_methods();
}

fn set_const(vm: &mut Vm, name: &str, v: Value) {
    vm.root.borrow_mut().insert(Value::str(name), v);
}

// ---- Argument helpers ----

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or_default()
}

fn int_arg(args: &[Value], i: usize) -> Result<i32, NativeError> {
    match args.get(i) {
        Some(Value::Int(n)) => Ok(*n),
        Some(Value::Float(x)) => Ok(*x as i32),
        Some(Value::Bool(b)) => Ok(*b as i32),
        other => Err(type_error(i, "integer", other)),
    }
}

fn num_arg(args: &[Value], i: usize) -> Result<f32, NativeError> {
    match args.get(i) {
        Some(Value::Int(n)) => Ok(*n as f32),
        Some(Value::Float(x)) => Ok(*x),
        other => Err(type_error(i, "float", other)),
    }
}

fn str_arg(args: &[Value], i: usize) -> Result<SqStr, NativeError> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s.clone()),
        other => Err(type_error(i, "string", other)),
    }
}

fn type_error(i: usize, expected: &str, got: Option<&Value>) -> NativeError {
    let got = got.map_or("null", |v| v.type_name());
    throw(format!(
        "parameter {i} has an invalid type '{got}' ; expected: '{expected}'"
    ))
}

fn bytes_value(b: &[u8]) -> Value {
    Value::Str(SqStr::new(b))
}

/// A method: its name, parameter count (Squirrel's convention) and body.
type Method = (&'static str, i32, fn(&mut Vm, &[Value]) -> NativeResult);

fn methods(list: &[Method]) -> Table {
    let mut t = Table::new();
    for &(name, n, f) in list {
        t.insert(Value::str(name), native(name, n, f));
    }
    t
}

// ---- Base library ----

fn base(vm: &mut Vm) {
    vm.register("print", 2, |vm, a| {
        let text = vm.to_bytes(&a[1])?;
        (vm.out)(&String::from_utf8_lossy(&text));
        Ok(Value::Null)
    });
    vm.register("type", 2, |_, a| Ok(Value::str(a[1].type_name())));
    vm.register("getroottable", 1, |vm, _| Ok(Value::Table(vm.root.clone())));
    vm.register("getconsttable", 1, |vm, _| {
        Ok(Value::Table(vm.consts.clone()))
    });
    vm.register("setroottable", 2, |vm, a| match &a[1] {
        Value::Table(t) => Ok(Value::Table(std::mem::replace(&mut vm.root, t.clone()))),
        other => Err(type_error(1, "table", Some(other))),
    });
    vm.register("compilestring", -2, |vm, a| {
        let src = str_arg(a, 1)?;
        let name = match a.get(2) {
            Some(Value::Str(s)) => s.to_string_lossy(),
            _ => "unnamedbuffer".into(),
        };
        vm.compile(&src.to_string_lossy(), &name).map_err(throw)
    });
    vm.register("assert", 2, |_, a| {
        if a[1].truthy() {
            Ok(Value::Null)
        } else {
            Err(throw("assertion failed"))
        }
    });
    vm.register("array", -2, |_, a| {
        let n = int_arg(a, 1)?;
        if n < 0 {
            return Err(throw("negative size"));
        }
        let fill = arg(a, 2);
        Ok(Value::Array(new_ref(vec![fill; n as usize])))
    });
    vm.register("collectgarbage", 1, |_, _| Ok(Value::Int(0)));
    vm.register("setconsttable", 2, |vm, a| match &a[1] {
        Value::Table(t) => Ok(Value::Table(std::mem::replace(&mut vm.consts, t.clone()))),
        other => Err(type_error(1, "table", Some(other))),
    });
    vm.register("enabledebuginfo", 2, |_, _| Ok(Value::Null));
    vm.register("printtodiag", 2, |vm, a| {
        let text = vm.to_bytes(&a[1])?;
        (vm.out)(&String::from_utf8_lossy(&text));
        Ok(Value::Null)
    });
    vm.register("dummy", 0, |_, _| Ok(Value::Null));
    // The current Squirrel thread object. We have no such value type yet; only
    // the game's debugger code (`__serialize_state`) uses it.
    vm.register("getthread", 0, |_, _| Ok(Value::Null));
    // launcher.dll's own three: `developer()` is the developer console
    // variable (0 in the shipped game).
    vm.register("developer", 1, |vm, _| Ok(Value::Int(vm.developer)));
    vm.register("GetFunctionSignature", 3, |_, a| {
        let Value::Closure(c) = &a[1] else {
            return Ok(Value::Null);
        };
        let name = match &a[2] {
            Value::Str(s) if !s.is_empty() => s.to_string_lossy(),
            _ => c.proto.name.as_deref().unwrap_or("<unnamed>").to_string(),
        };
        let params: Vec<&str> = c.proto.param_names.iter().map(|p| &**p).collect();
        Ok(Value::str(&format!(
            "function {name}({})",
            params.join(", ")
        )))
    });
    // IsValid: null is invalid; an instance is valid only if it's a live
    // engine entity (asked of the host); any other value is valid.
    vm.register("IsValid", 2, |vm, a| {
        Ok(Value::Bool(match &a[1] {
            Value::Null => false,
            Value::Instance(_) => vm.entity_is_valid.as_ref().is_some_and(|f| f(&a[1])),
            _ => true,
        }))
    });
    vm.register("seterrorhandler", 2, |_, _| Ok(Value::Null));
    vm.register("setdebughook", 2, |_, _| Ok(Value::Null));
    vm.register("getstackinfos", 2, |_, _| Ok(Value::Null));
    set_const(vm, "_version_", Value::str("Squirrel 2.2.3 stable"));
    set_const(vm, "_charsize_", Value::Int(1));
    set_const(vm, "_intsize_", Value::Int(4));
    set_const(vm, "_floatsize_", Value::Int(4));
}

// ---- Math ----

fn math(vm: &mut Vm) {
    fn unary(vm: &mut Vm, name: &str, f: fn(f32) -> f32) {
        vm.register(name, 2, move |_, a| Ok(Value::Float(f(num_arg(a, 1)?))));
    }
    unary(vm, "sqrt", f32::sqrt);
    unary(vm, "sin", f32::sin);
    unary(vm, "cos", f32::cos);
    unary(vm, "tan", f32::tan);
    unary(vm, "asin", f32::asin);
    unary(vm, "acos", f32::acos);
    unary(vm, "atan", f32::atan);
    unary(vm, "log", f32::ln);
    unary(vm, "log10", f32::log10);
    unary(vm, "exp", f32::exp);
    unary(vm, "fabs", f32::abs);
    unary(vm, "floor", f32::floor);
    unary(vm, "ceil", f32::ceil);
    vm.register("atan2", 3, |_, a| {
        Ok(Value::Float(num_arg(a, 1)?.atan2(num_arg(a, 2)?)))
    });
    vm.register("pow", 3, |_, a| {
        Ok(Value::Float(num_arg(a, 1)?.powf(num_arg(a, 2)?)))
    });
    // `abs` gives an integer (oracle F026).
    vm.register("abs", 2, |_, a| {
        Ok(Value::Int(int_arg(a, 1)?.wrapping_abs()))
    });
    // `rand` is the C library's: the Microsoft generator.
    let seed = Rc::new(std::cell::Cell::new(1u32));
    let s = seed.clone();
    vm.register("srand", 2, move |_, a| {
        s.set(int_arg(a, 1)? as u32);
        Ok(Value::Null)
    });
    vm.register("rand", 1, move |_, _| {
        let next = seed.get().wrapping_mul(214_013).wrapping_add(2_531_011);
        seed.set(next);
        Ok(Value::Int(((next >> 16) & 0x7fff) as i32))
    });
    set_const(vm, "RAND_MAX", Value::Int(32767));
    set_const(vm, "PI", Value::Float(std::f32::consts::PI));
}

// ---- String library ----

fn strings(vm: &mut Vm) {
    vm.register("format", -2, |vm, a| format(vm, a).map(Value::Str));
    // Split on any of the separator characters, dropping empty pieces
    // (oracle S016).
    vm.register("split", 3, |_, a| {
        let s = str_arg(a, 1)?;
        let sep = str_arg(a, 2)?;
        let parts: Vec<Value> = s
            .bytes()
            .split(|b| sep.bytes().contains(b))
            .filter(|p| !p.is_empty())
            .map(bytes_value)
            .collect();
        Ok(Value::Array(new_ref(parts)))
    });
    fn is_space(b: &u8) -> bool {
        matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
    }
    vm.register("strip", 2, |_, a| {
        let s = str_arg(a, 1)?;
        let b = s.bytes();
        let start = b.iter().position(|c| !is_space(c)).unwrap_or(b.len());
        let end = b
            .iter()
            .rposition(|c| !is_space(c))
            .map_or(start, |i| i + 1);
        Ok(bytes_value(&b[start..end]))
    });
    vm.register("lstrip", 2, |_, a| {
        let s = str_arg(a, 1)?;
        let b = s.bytes();
        let start = b.iter().position(|c| !is_space(c)).unwrap_or(b.len());
        Ok(bytes_value(&b[start..]))
    });
    vm.register("rstrip", 2, |_, a| {
        let s = str_arg(a, 1)?;
        let b = s.bytes();
        let end = b.iter().rposition(|c| !is_space(c)).map_or(0, |i| i + 1);
        Ok(bytes_value(&b[..end]))
    });
}

/// `format(fmt, ...)`: C `printf` conversions over script values.
fn format(vm: &mut Vm, a: &[Value]) -> Result<SqStr, NativeError> {
    let fmt = str_arg(a, 1)?;
    let f = fmt.bytes();
    let mut out = Vec::new();
    let mut next = 2;
    let mut i = 0;
    while i < f.len() {
        if f[i] != b'%' {
            out.push(f[i]);
            i += 1;
            continue;
        }
        i += 1;
        if f.get(i) == Some(&b'%') {
            out.push(b'%');
            i += 1;
            continue;
        }
        let mut flags = Flags::default();
        while let Some(&c) = f.get(i) {
            match c {
                b'-' => flags.left = true,
                b'+' => flags.plus = true,
                b' ' => flags.space = true,
                b'#' => flags.alt = true,
                b'0' => flags.zero = true,
                _ => break,
            }
            i += 1;
        }
        let mut width = 0usize;
        while let Some(c) = f.get(i).filter(|c| c.is_ascii_digit()) {
            width = width * 10 + (c - b'0') as usize;
            i += 1;
        }
        let mut precision = None;
        if f.get(i) == Some(&b'.') {
            i += 1;
            let mut p = 0usize;
            while let Some(c) = f.get(i).filter(|c| c.is_ascii_digit()) {
                p = p * 10 + (c - b'0') as usize;
                i += 1;
            }
            precision = Some(p);
        }
        let Some(&conv) = f.get(i) else {
            return Err(throw("invalid format"));
        };
        i += 1;
        if next >= a.len() {
            return Err(throw("not enough paramters for the given format string"));
        }
        let v = a[next].clone();
        next += 1;
        let (body, numeric): (Vec<u8>, bool) = match conv {
            b'd' | b'i' => {
                let n = int_arg(a, next - 1)?;
                (int_text(n as i64, 10, false, precision, &flags), true)
            }
            b'u' => (
                int_text(
                    int_arg(a, next - 1)? as u32 as i64,
                    10,
                    false,
                    precision,
                    &flags,
                ),
                true,
            ),
            b'x' | b'X' | b'o' => {
                let n = int_arg(a, next - 1)? as u32 as i64;
                let radix = if conv == b'o' { 8 } else { 16 };
                (int_text(n, radix, conv == b'X', precision, &flags), true)
            }
            b'c' => (vec![int_arg(a, next - 1)? as u8], false),
            b's' => {
                // Not checked yet whether the game accepts non-strings here.
                let mut s = vm.to_bytes(&v)?;
                if let Some(p) = precision {
                    s.truncate(p);
                }
                (s, false)
            }
            b'f' | b'e' | b'E' | b'g' | b'G' => {
                let x = num_arg(a, next - 1)?;
                (
                    format_float(x as f64, conv, precision, &flags).into_bytes(),
                    true,
                )
            }
            _ => return Err(throw("invalid format")),
        };
        if body.len() >= width {
            out.extend(body);
        } else if flags.left {
            out.extend(&body);
            out.extend(std::iter::repeat_n(b' ', width - body.len()));
        } else if flags.zero && numeric {
            // Zeros go after the sign.
            let signed = matches!(body.first(), Some(b'-' | b'+' | b' '));
            let (sign, digits) = body.split_at(signed as usize);
            out.extend(sign);
            out.extend(std::iter::repeat_n(b'0', width - body.len()));
            out.extend(digits);
        } else {
            out.extend(std::iter::repeat_n(b' ', width - body.len()));
            out.extend(body);
        }
    }
    Ok(SqStr::new(&out))
}

fn int_text(n: i64, radix: u32, upper: bool, precision: Option<usize>, flags: &Flags) -> Vec<u8> {
    let neg = n < 0;
    let mut digits = match radix {
        16 if upper => format!("{:X}", n.unsigned_abs()),
        16 => format!("{:x}", n.unsigned_abs()),
        8 => format!("{:o}", n.unsigned_abs()),
        _ => n.unsigned_abs().to_string(),
    };
    if let Some(p) = precision {
        while digits.len() < p {
            digits.insert(0, '0');
        }
    }
    let prefix = if neg {
        "-"
    } else if flags.plus && radix == 10 {
        "+"
    } else if flags.space && radix == 10 {
        " "
    } else if flags.alt && radix == 16 && n != 0 {
        if upper { "0X" } else { "0x" }
    } else {
        ""
    };
    format!("{prefix}{digits}").into_bytes()
}

// ---- Per-type methods ----

fn number_methods() -> Table {
    methods(&[
        ("tointeger", 1, |_, a| {
            Ok(Value::Int(match a[0] {
                Value::Int(i) => i,
                Value::Float(x) => x as i32,
                Value::Bool(b) => b as i32,
                _ => 0,
            }))
        }),
        ("tofloat", 1, |_, a| {
            Ok(Value::Float(match a[0] {
                Value::Int(i) => i as f32,
                Value::Float(x) => x,
                Value::Bool(b) => b as i32 as f32,
                _ => 0.0,
            }))
        }),
        ("tostring", 1, |vm, a| {
            Ok(Value::str(&vm.plain_string(&a[0])))
        }),
        ("tochar", 1, |_, a| {
            let n = int_arg(a, 0)?;
            Ok(bytes_value(&[n as u8]))
        }),
        ("weakref", 1, |_, a| Ok(a[0].clone())),
    ])
}

/// A script string as a number, or None (then `tointeger`/`tofloat` give
/// null, oracle F033).
fn parse_number(s: &str) -> Option<Value> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u32::from_str_radix(hex, 16)
            .ok()
            .map(|n| Value::Int(n as i32));
    }
    if let Ok(i) = s.parse::<i64>() {
        return Some(Value::Int(i as i32));
    }
    s.parse::<f32>().ok().map(Value::Float)
}

fn string_methods() -> Table {
    methods(&[
        ("len", 1, |_, a| Ok(Value::Int(str_arg(a, 0)?.len() as i32))),
        ("tostring", 1, |_, a| Ok(a[0].clone())),
        ("tointeger", 1, |_, a| {
            let s = str_arg(a, 0)?;
            Ok(match parse_number(&s.to_string_lossy()) {
                Some(Value::Float(x)) => Value::Int(x as i32),
                Some(v) => v,
                None => Value::Null,
            })
        }),
        ("tofloat", 1, |_, a| {
            let s = str_arg(a, 0)?;
            Ok(match parse_number(&s.to_string_lossy()) {
                Some(Value::Int(i)) => Value::Float(i as f32),
                Some(v) => v,
                None => Value::Null,
            })
        }),
        ("slice", -2, |_, a| {
            let s = str_arg(a, 0)?;
            let (start, end) = slice_range(a, s.len())?;
            Ok(bytes_value(&s.bytes()[start..end]))
        }),
        ("find", -2, |_, a| {
            let s = str_arg(a, 0)?;
            let what = str_arg(a, 1)?;
            let from = if a.len() > 2 {
                int_arg(a, 2)?.max(0) as usize
            } else {
                0
            };
            let hay = s.bytes();
            let needle = what.bytes();
            if from > hay.len() {
                return Ok(Value::Null);
            }
            let found = if needle.is_empty() {
                Some(from)
            } else {
                hay[from..]
                    .windows(needle.len())
                    .position(|w| w == needle)
                    .map(|p| p + from)
            };
            Ok(found.map_or(Value::Null, |i| Value::Int(i as i32)))
        }),
        ("tolower", 1, |_, a| {
            Ok(bytes_value(&str_arg(a, 0)?.bytes().to_ascii_lowercase()))
        }),
        ("toupper", 1, |_, a| {
            Ok(bytes_value(&str_arg(a, 0)?.bytes().to_ascii_uppercase()))
        }),
        ("weakref", 1, |_, a| Ok(a[0].clone())),
    ])
}

/// `slice(start, end)` bounds: negative counts from the end.
fn slice_range(a: &[Value], len: usize) -> Result<(usize, usize), NativeError> {
    let len = len as i32;
    let mut start = if a.len() > 1 { int_arg(a, 1)? } else { 0 };
    let mut end = if a.len() > 2 { int_arg(a, 2)? } else { len };
    if start < 0 {
        start += len;
    }
    if end < 0 {
        end += len;
    }
    if end < start {
        return Err(throw("wrong indexes"));
    }
    if start < 0 || end > len {
        return Err(throw("slice out of range"));
    }
    Ok((start as usize, end as usize))
}

fn array_of(v: &Value) -> Result<Ref<Vec<Value>>, NativeError> {
    match v {
        Value::Array(a) => Ok(a.clone()),
        other => Err(type_error(0, "array", Some(other))),
    }
}

fn array_methods() -> Table {
    methods(&[
        ("len", 1, |_, a| {
            Ok(Value::Int(array_of(&a[0])?.borrow().len() as i32))
        }),
        ("append", 2, |_, a| {
            array_of(&a[0])?.borrow_mut().push(a[1].clone());
            Ok(Value::Null)
        }),
        ("push", 2, |_, a| {
            array_of(&a[0])?.borrow_mut().push(a[1].clone());
            Ok(Value::Null)
        }),
        ("extend", 2, |_, a| {
            let other = array_of(&a[1])?.borrow().clone();
            array_of(&a[0])?.borrow_mut().extend(other);
            Ok(a[0].clone())
        }),
        ("pop", 1, |_, a| {
            array_of(&a[0])?
                .borrow_mut()
                .pop()
                .ok_or_else(|| throw("empty array"))
        }),
        ("top", 1, |_, a| {
            array_of(&a[0])?
                .borrow()
                .last()
                .cloned()
                .ok_or_else(|| throw("top() on a empty array"))
        }),
        ("insert", 3, |_, a| {
            let arr = array_of(&a[0])?;
            let i = int_arg(a, 1)?;
            let mut arr = arr.borrow_mut();
            if i < 0 || i as usize > arr.len() {
                return Err(throw("index out of range"));
            }
            arr.insert(i as usize, a[2].clone());
            Ok(Value::Null)
        }),
        ("remove", 2, |_, a| {
            let arr = array_of(&a[0])?;
            let i = int_arg(a, 1)?;
            let mut arr = arr.borrow_mut();
            if i < 0 || i as usize >= arr.len() {
                return Err(throw("index out of range"));
            }
            Ok(arr.remove(i as usize))
        }),
        ("resize", -2, |_, a| {
            let n = int_arg(a, 1)?;
            if n < 0 {
                return Err(throw("negative size"));
            }
            let fill = arg(a, 2);
            array_of(&a[0])?.borrow_mut().resize(n as usize, fill);
            Ok(Value::Null)
        }),
        ("reverse", 1, |_, a| {
            array_of(&a[0])?.borrow_mut().reverse();
            Ok(Value::Null)
        }),
        ("clear", 1, |_, a| {
            array_of(&a[0])?.borrow_mut().clear();
            Ok(Value::Null)
        }),
        ("slice", -2, |_, a| {
            let arr = array_of(&a[0])?;
            let arr = arr.borrow();
            let (start, end) = slice_range(a, arr.len())?;
            Ok(Value::Array(new_ref(arr[start..end].to_vec())))
        }),
        ("sort", -1, |vm, a| {
            let arr = array_of(&a[0])?;
            let func = a.get(1).cloned();
            let n = arr.borrow().len() as i64;
            quicksort(vm, &arr, 0, n - 1, func.as_ref())?;
            Ok(Value::Null)
        }),
        ("tostring", 1, |vm, a| {
            Ok(Value::str(&vm.plain_string(&a[0])))
        }),
        ("weakref", 1, |_, a| {
            Ok(Value::WeakRef(Rc::new(WeakValue::new(&a[0]))))
        }),
    ])
}

/// The game's `sort` (launcher.dll): quicksort with the first element as
/// the pivot, so equal elements end up where the game puts them. `func`
/// gets (element, pivot) with the root table as `this`, and returns <0, 0
/// or >0.
fn quicksort(
    vm: &mut Vm,
    arr: &Ref<Vec<Value>>,
    l: i64,
    r: i64,
    func: Option<&Value>,
) -> Result<(), NativeError> {
    if l >= r {
        return Ok(());
    }
    let pivot = arr.borrow()[l as usize].clone();
    let cmp = |vm: &mut Vm, i: i64| -> Result<i32, NativeError> {
        let item = arr.borrow().get(i as usize).cloned().unwrap_or_default();
        match func {
            None => Ok(vm.compare(&item, &pivot)?),
            Some(f) => {
                let root = Value::Table(vm.root.clone());
                match vm.call(f, root, &[item, pivot.clone()])? {
                    Value::Int(n) => Ok(n),
                    Value::Float(x) => Ok(x as i32),
                    _ => Err(throw(
                        "numeric value expected as return value of the compare function",
                    )),
                }
            }
        }
    };
    let (mut i, mut j) = (l, r + 1);
    loop {
        loop {
            i += 1;
            if i > r || cmp(vm, i)? > 0 {
                break;
            }
        }
        loop {
            j -= 1;
            if j < 0 {
                return Err(throw("Invalid qsort, probably compare function defect"));
            }
            if cmp(vm, j)? <= 0 {
                break;
            }
        }
        if j <= i {
            arr.borrow_mut().swap(l as usize, j as usize);
            quicksort(vm, arr, l, j - 1, func)?;
            return quicksort(vm, arr, j + 1, r, func);
        }
        arr.borrow_mut().swap(i as usize, j as usize);
    }
}

fn table_of(v: &Value) -> Result<Ref<Table>, NativeError> {
    match v {
        Value::Table(t) => Ok(t.clone()),
        other => Err(type_error(0, "table", Some(other))),
    }
}

fn table_methods() -> Table {
    methods(&[
        ("len", 1, |_, a| {
            Ok(Value::Int(table_of(&a[0])?.borrow().len() as i32))
        }),
        ("rawget", 2, |vm, a| {
            let t = table_of(&a[0])?;
            let v = t.borrow().get(&a[1]).cloned();
            v.ok_or_else(|| {
                let k = vm.plain_string(&a[1]);
                throw(format!("the index '{k}' does not exist"))
            })
        }),
        ("rawset", 3, |_, a| {
            if a[1].is_null() {
                return Err(throw("null cannot be used as index"));
            }
            table_of(&a[0])?
                .borrow_mut()
                .insert(a[1].clone(), a[2].clone());
            Ok(a[0].clone())
        }),
        ("rawdelete", 2, |_, a| {
            Ok(table_of(&a[0])?
                .borrow_mut()
                .remove(&a[1])
                .unwrap_or_default())
        }),
        ("rawin", 2, |_, a| {
            Ok(Value::Bool(table_of(&a[0])?.borrow().contains(&a[1])))
        }),
        ("clear", 1, |_, a| {
            let t = table_of(&a[0])?;
            let delegate = t.borrow().delegate.clone();
            *t.borrow_mut() = Table::new();
            t.borrow_mut().delegate = delegate;
            Ok(Value::Null)
        }),
        ("tostring", 1, |vm, a| {
            Ok(Value::str(&vm.plain_string(&a[0])))
        }),
        ("weakref", 1, |_, a| {
            Ok(Value::WeakRef(Rc::new(WeakValue::new(&a[0]))))
        }),
    ])
}

fn function_methods() -> Table {
    methods(&[
        ("call", -2, |vm, a| {
            Ok(vm.call(&a[0], a[1].clone(), &a[2..])?)
        }),
        ("pcall", -2, |vm, a| {
            Ok(vm.call(&a[0], a[1].clone(), &a[2..]).unwrap_or_default())
        }),
        ("acall", 2, |vm, a| {
            let args = array_of(&a[1])?.borrow().clone();
            let (this, rest) = args
                .split_first()
                .ok_or_else(|| throw("not enough params in the stack"))?;
            Ok(vm.call(&a[0], this.clone(), rest)?)
        }),
        ("pacall", 2, |vm, a| {
            let args = array_of(&a[1])?.borrow().clone();
            let (this, rest) = args
                .split_first()
                .ok_or_else(|| throw("not enough params in the stack"))?;
            Ok(vm.call(&a[0], this.clone(), rest).unwrap_or_default())
        }),
        ("bindenv", 2, |_, a| match &a[0] {
            Value::Closure(c) => Ok(Value::Closure(Rc::new(Closure {
                proto: c.proto.clone(),
                free: c.free.clone(),
                defaults: c.defaults.clone(),
                env: Some(a[1].clone()),
            }))),
            Value::Native(n) => Ok(Value::Native(Rc::new(NativeFn {
                name: n.name.clone(),
                nparams: n.nparams,
                f: n.f.clone(),
                env: Some(a[1].clone()),
            }))),
            other => Ok(other.clone()),
        }),
        ("getinfos", 1, |_, a| {
            let mut t = Table::new();
            match &a[0] {
                Value::Closure(c) => {
                    let p = &c.proto;
                    t.insert(Value::str("native"), Value::Bool(false));
                    t.insert(
                        Value::str("name"),
                        p.name.as_deref().map_or(Value::Null, Value::str),
                    );
                    t.insert(Value::str("src"), Value::str(&p.source));
                    t.insert(Value::str("varargs"), Value::Bool(p.varargs));
                }
                Value::Native(n) => {
                    t.insert(Value::str("native"), Value::Bool(true));
                    t.insert(Value::str("name"), Value::str(&n.name));
                    t.insert(Value::str("paramscheck"), Value::Int(n.nparams));
                }
                _ => {}
            }
            Ok(Value::Table(new_ref(t)))
        }),
        ("tostring", 1, |vm, a| {
            Ok(Value::str(&vm.plain_string(&a[0])))
        }),
        ("weakref", 1, |_, a| {
            Ok(Value::WeakRef(Rc::new(WeakValue::new(&a[0]))))
        }),
    ])
}

fn class_methods() -> Table {
    methods(&[
        ("instance", 1, |_, a| match &a[0] {
            Value::Class(c) => {
                c.borrow_mut().locked = true;
                let mut values = Table::new();
                for (k, v) in c.borrow().members.iter() {
                    if !matches!(v, Value::Closure(_) | Value::Native(_)) {
                        values.insert(k.clone(), v.clone());
                    }
                }
                Ok(Value::Instance(new_ref(Instance {
                    class: c.clone(),
                    values,
                    user: UserData::None,
                })))
            }
            other => Err(type_error(0, "class", Some(other))),
        }),
        ("rawin", 2, |_, a| match &a[0] {
            Value::Class(c) => Ok(Value::Bool(c.borrow().members.contains(&a[1]))),
            _ => Ok(Value::Bool(false)),
        }),
        ("getattributes", 2, |_, _| Ok(Value::Null)),
        ("setattributes", 3, |_, _| Ok(Value::Null)),
        ("tostring", 1, |vm, a| {
            Ok(Value::str(&vm.plain_string(&a[0])))
        }),
        ("weakref", 1, |_, a| {
            Ok(Value::WeakRef(Rc::new(WeakValue::new(&a[0]))))
        }),
    ])
}

fn instance_methods() -> Table {
    methods(&[
        ("getclass", 1, |_, a| match &a[0] {
            Value::Instance(i) => Ok(Value::Class(i.borrow().class.clone())),
            other => Err(type_error(0, "instance", Some(other))),
        }),
        ("rawin", 2, |_, a| match &a[0] {
            Value::Instance(i) => {
                let i = i.borrow();
                Ok(Value::Bool(
                    i.values.contains(&a[1]) || i.class.borrow().members.contains(&a[1]),
                ))
            }
            _ => Ok(Value::Bool(false)),
        }),
        ("tostring", 1, |vm, a| {
            Ok(Value::str(&vm.plain_string(&a[0])))
        }),
        ("weakref", 1, |_, a| {
            Ok(Value::WeakRef(Rc::new(WeakValue::new(&a[0]))))
        }),
    ])
}

fn weakref_methods() -> Table {
    methods(&[
        ("ref", 1, |_, a| match &a[0] {
            Value::WeakRef(w) => Ok(w.get()),
            other => Ok(other.clone()),
        }),
        ("tostring", 1, |vm, a| {
            Ok(Value::str(&vm.plain_string(&a[0])))
        }),
        ("weakref", 1, |_, a| Ok(a[0].clone())),
    ])
}
