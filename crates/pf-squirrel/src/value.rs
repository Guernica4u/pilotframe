//! Script values. Integers and floats are 32-bit, as in the game
//! (docs/port/scripts.md, "Numbers").

use std::cell::RefCell;
use std::fmt;
use std::rc::{Rc, Weak};

use crate::compiler::Proto;
use crate::table::Table;
use crate::vm::{NativeResult, Vm};

/// A shared, mutable object. (Rust: `Rc` counts references like Squirrel's
/// own reference counting; `RefCell` allows changing the object through any
/// of them.)
pub type Ref<T> = Rc<RefCell<T>>;

pub fn new_ref<T>(v: T) -> Ref<T> {
    Rc::new(RefCell::new(v))
}

#[derive(Clone, Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Int(i32),
    Float(f32),
    Str(SqStr),
    Table(Ref<Table>),
    Array(Ref<Vec<Value>>),
    Closure(Rc<Closure>),
    Native(Rc<NativeFn>),
    Class(Ref<Class>),
    Instance(Ref<Instance>),
    WeakRef(Rc<WeakValue>),
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(SqStr::new(s.as_bytes()))
    }

    /// The name `typeof` gives (before any `_typeof` metamethod).
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Int(_) => "integer",
            Value::Float(_) => "float",
            Value::Str(_) => "string",
            Value::Table(_) => "table",
            Value::Array(_) => "array",
            Value::Closure(_) => "function",
            Value::Native(_) => "native function",
            Value::Class(_) => "class",
            Value::Instance(_) => "instance",
            Value::WeakRef(_) => "weakref",
        }
    }

    /// Squirrel's truth test: null, false, 0 and 0.0 are false; everything
    /// else (including "" and empty arrays) is true (oracle Y005–Y009).
    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            _ => true,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Same type and same value, or the same object. This is how table keys
    /// and `==` on objects compare.
    pub fn raw_eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits() || a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            _ => match (self.object_id(), other.object_id()) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            },
        }
    }

    /// The address of a reference-counted object (None for plain values).
    pub fn object_id(&self) -> Option<usize> {
        Some(match self {
            Value::Table(r) => Rc::as_ptr(r) as *const u8 as usize,
            Value::Array(r) => Rc::as_ptr(r) as *const u8 as usize,
            Value::Closure(r) => Rc::as_ptr(r) as *const u8 as usize,
            Value::Native(r) => Rc::as_ptr(r) as *const u8 as usize,
            Value::Class(r) => Rc::as_ptr(r) as *const u8 as usize,
            Value::Instance(r) => Rc::as_ptr(r) as *const u8 as usize,
            Value::WeakRef(r) => Rc::as_ptr(r) as *const u8 as usize,
            Value::Str(s) => Rc::as_ptr(&s.0) as *const u8 as usize,
            _ => return None,
        })
    }

    /// The key hash the game's tables use (docs/port/scripts.md, "Tables").
    pub fn key_hash(&self) -> u64 {
        match self {
            Value::Bool(b) => *b as u64,
            Value::Int(i) => *i as i64 as u64,
            // Truncated to an integer, as the game does.
            Value::Float(f) => *f as i32 as i64 as u64,
            Value::Str(s) => s.hash(),
            // Any other object: its address shifted right by 3. Addresses
            // differ from run to run in the game too, so only the rule
            // matters.
            _ => (self.object_id().unwrap_or(0) >> 3) as u32 as i32 as i64 as u64,
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => write!(f, "{x}f"),
            Value::Str(s) => write!(f, "{:?}", s.to_string_lossy()),
            other => write!(f, "<{}>", other.type_name()),
        }
    }
}

/// A string. Bytes, not UTF-8 (scripts index strings by byte: `"x"[0]` is
/// 120). The hash is worked out once, as in the game.
#[derive(Clone)]
pub struct SqStr(Rc<StrData>);

struct StrData {
    hash: u64,
    bytes: Box<[u8]>,
}

impl SqStr {
    pub fn new(bytes: &[u8]) -> SqStr {
        SqStr(Rc::new(StrData {
            hash: string_hash(bytes),
            bytes: bytes.into(),
        }))
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0.bytes
    }

    pub fn hash(&self) -> u64 {
        self.0.hash
    }

    pub fn len(&self) -> usize {
        self.0.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.bytes.is_empty()
    }

    pub fn to_string_lossy(&self) -> String {
        String::from_utf8_lossy(&self.0.bytes).into_owned()
    }
}

impl PartialEq for SqStr {
    fn eq(&self, other: &SqStr) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
            || (self.0.hash == other.0.hash && self.0.bytes == other.0.bytes)
    }
}

/// The game's string hash (launcher.dll; docs/port/scripts.md, "Strings").
pub fn string_hash(bytes: &[u8]) -> u64 {
    let n = bytes.len() as u64;
    let step = ((n | 32) >> 5) as usize;
    let mut h = n;
    let mut i = 0;
    let mut left = bytes.len();
    while left >= step {
        // A signed char, masked to 16 bits.
        let c = (bytes[i] as i8 as i32 & 0xffff) as u64;
        h ^= h.wrapping_mul(32).wrapping_add(h >> 2).wrapping_add(c);
        i += step;
        left -= step;
    }
    h
}

/// A script function plus what was bound to it when it was created.
pub struct Closure {
    pub proto: Rc<Proto>,
    /// Squirrel 2 free variables (`function() : (a, b)`), copied in.
    pub free: Vec<Value>,
    /// Default parameter values, worked out when the function was created.
    pub defaults: Vec<Value>,
    /// The `this` set by `bindenv` (None: the caller's).
    pub env: Option<Value>,
}

/// An engine (Rust) function scripts can call.
pub struct NativeFn {
    pub name: Rc<str>,
    /// Parameter count including `this`, Squirrel's convention: 0 means any,
    /// a positive count is exact, a negative one is a minimum.
    pub nparams: i32,
    pub f: NativeFnPtr,
    /// The `this` set by `bindenv` (None: the caller's). The game's scripts
    /// rely on it: `SpinOff.bindenv( scope )( f )` runs `f` with `scope` as
    /// `this`.
    pub env: Option<Value>,
}

pub type NativeFnPtr = Rc<dyn Fn(&mut Vm, &[Value]) -> NativeResult>;

pub struct Class {
    pub base: Option<Ref<Class>>,
    /// Fields (with their default values) and methods, the base class's
    /// first.
    pub members: Table,
    /// Set once an instance exists; the class can't be changed after.
    pub locked: bool,
}

pub struct Instance {
    pub class: Ref<Class>,
    /// This instance's fields, a shallow copy of the class defaults (so a
    /// default array is shared by every instance, oracle C005).
    pub values: Table,
    /// What a native class keeps in its instances.
    pub user: UserData,
}

/// The native data behind an instance of an engine class.
#[derive(Clone, Default)]
pub enum UserData {
    #[default]
    None,
    /// A `Vector` (launcher.dll).
    Vector([f32; 3]),
    /// Something of the host's (an entity handle, a HUD element...).
    Host(Rc<dyn std::any::Any>),
}

/// `weakref()`: a reference that doesn't keep an object alive.
pub enum WeakValue {
    /// Plain values are copied (Squirrel returns the value itself for these).
    Plain(Value),
    Table(Weak<RefCell<Table>>),
    Array(Weak<RefCell<Vec<Value>>>),
    Closure(Weak<Closure>),
    Native(Weak<NativeFn>),
    Class(Weak<RefCell<Class>>),
    Instance(Weak<RefCell<Instance>>),
}

impl WeakValue {
    pub fn new(v: &Value) -> WeakValue {
        match v {
            Value::Table(r) => WeakValue::Table(Rc::downgrade(r)),
            Value::Array(r) => WeakValue::Array(Rc::downgrade(r)),
            Value::Closure(r) => WeakValue::Closure(Rc::downgrade(r)),
            Value::Native(r) => WeakValue::Native(Rc::downgrade(r)),
            Value::Class(r) => WeakValue::Class(Rc::downgrade(r)),
            Value::Instance(r) => WeakValue::Instance(Rc::downgrade(r)),
            other => WeakValue::Plain(other.clone()),
        }
    }

    /// The object, or null once it's gone.
    pub fn get(&self) -> Value {
        let up = |o: Option<Value>| o.unwrap_or(Value::Null);
        match self {
            WeakValue::Plain(v) => v.clone(),
            WeakValue::Table(w) => up(w.upgrade().map(Value::Table)),
            WeakValue::Array(w) => up(w.upgrade().map(Value::Array)),
            WeakValue::Closure(w) => up(w.upgrade().map(Value::Closure)),
            WeakValue::Native(w) => up(w.upgrade().map(Value::Native)),
            WeakValue::Class(w) => up(w.upgrade().map(Value::Class)),
            WeakValue::Instance(w) => up(w.upgrade().map(Value::Instance)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_strings_hash_every_byte() {
        // h = 1, then h ^ (h*32 + h/4 + 'a').
        assert_eq!(string_hash(b"a"), 1 ^ (32 + 97));
        assert_eq!(string_hash(b""), 0);
    }
}
