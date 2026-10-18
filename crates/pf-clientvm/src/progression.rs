//! The XP table: scripts fill it with `SetXPForLevel` and read it with
//! `GetLevelForXP` (client.dll 0x1802c8c50 and 0x1802c8bf0).

use std::cell::RefCell;
use std::rc::Rc;

use pf_squirrel::{Value, throw};

use crate::ClientVm;

pub const MAX_LEVEL: i32 = 100;

#[derive(Default)]
pub struct XpTable {
    /// XP needed for each level; index 0 is unused.
    pub xp: Vec<i32>,
    /// The highest level set so far.
    pub last: i32,
}

impl XpTable {
    fn get(&self, level: i32) -> i32 {
        self.xp.get(level as usize).copied().unwrap_or(0)
    }

    pub fn level_for(&self, xp: i32) -> i32 {
        let mut level = self.last;
        while level > 0 {
            if self.get(level) <= xp {
                return level;
            }
            level -= 1;
        }
        1
    }
}

fn int(v: &Value) -> i32 {
    match v {
        Value::Int(n) => *n,
        Value::Float(x) => *x as i32,
        _ => 0,
    }
}

pub fn install(cvm: &mut ClientVm) {
    let table = Rc::new(RefCell::new(XpTable::default()));
    cvm.xp_table = Some(table.clone());
    let t = table.clone();
    cvm.vm.register("SetXPForLevel", 3, move |_, a| {
        let (level, xp) = (int(&a[1]), int(&a[2]));
        let mut t = t.borrow_mut();
        if level == 1 {
            if xp != 0 {
                return Err(throw("XP required for level 1 must be 0"));
            }
            t.last = 1;
        } else {
            if level < t.last + 1 {
                return Err(throw(format!(
                    "Cannot set XP required for level {level} more than once"
                )));
            }
            if t.last + 1 < level {
                return Err(throw(format!(
                    "Cannot set XP required for level {level} until XP has been set for all levels below it"
                )));
            }
            if level > MAX_LEVEL {
                return Err(throw(format!("Too many levels, max {MAX_LEVEL}")));
            }
        }
        let prev = t.get(level - 1);
        if xp <= prev && level > 1 {
            return Err(throw(format!(
                "Cannot set {xp} XP required for level {level} because is is not greater than the {prev} XP required for level {}",
                level - 1
            )));
        }
        if t.xp.len() <= level as usize {
            t.xp.resize(level as usize + 1, 0);
        }
        t.xp[level as usize] = xp;
        t.last = level;
        Ok(Value::Null)
    });
    let t = table.clone();
    cvm.vm.register("GetLevelForXP", 2, move |_, a| {
        let xp = int(&a[1]);
        if xp < 0 {
            return Err(throw(format!("XP {xp} too low")));
        }
        Ok(Value::Int(t.borrow().level_for(xp)))
    });
}
