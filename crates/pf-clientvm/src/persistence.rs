//! Persistent player data: the definition (enums and variables) and the
//! client script functions that read it. In the game the server loads the
//! definition (`persistent_player_data_version_N.pdef`) and sends it to
//! each client; it isn't in the install, so our game builds it from the
//! install's scripts (`pdef.rs`, owner's decision 2026-10-17). The rules
//! here are ported from engine.dll and client.dll
//! (docs/port/scripts.md, "Persistence").

use std::rc::Rc;

use pf_squirrel::{Value, throw};

use crate::ClientVm;

/// The engine's limits (engine.dll 0x1802170e0).
pub const MAX_ENUMS: usize = 0x20;
pub const MAX_ENUM_ITEMS: usize = 0x400;

/// An enum: its items in order. Index 0 is often `NULL`, which the
/// engine stores as "" (engine.dll 0x180217620).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PdefEnum {
    pub name: String,
    pub items: Vec<String>,
    /// Where the items came from and how sure we are (ours).
    pub source: String,
    /// True when the items or their order are a stand-in.
    pub placeholder: bool,
}

/// The persistent data definition.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PersistenceDef {
    pub enums: Vec<PdefEnum>,
}

impl PersistenceDef {
    /// Add an enum the way the engine's parser does: a first item `NULL`
    /// is stored as "", a repeated item is refused. Errors are the
    /// engine's.
    pub fn add_enum(
        &mut self,
        name: &str,
        items: &[String],
        source: &str,
        placeholder: bool,
    ) -> Result<(), String> {
        if self.find_enum(name).is_some() {
            return Err(format!(
                "Cannot define enum with name '{name}'. An enum with this name has already been defined."
            ));
        }
        if self.enums.len() >= MAX_ENUMS {
            return Err(format!(
                "Cannot define new enum '{name}', already at limit of {MAX_ENUMS} enums."
            ));
        }
        if items.is_empty() {
            return Err(format!("Enum '{name}' has no entries."));
        }
        let total: usize = self.enums.iter().map(|e| e.items.len()).sum();
        if total + items.len() > MAX_ENUM_ITEMS {
            // The engine reports this and carries on.
            eprintln!("Hit PDATA_MAX_ENUM_ITEM_NAMES limit of {MAX_ENUM_ITEMS} items.");
        }
        let mut out: Vec<String> = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            if out.iter().any(|o| o.eq_ignore_ascii_case(item)) {
                return Err(format!(
                    "Enum '{name}' already has an entry named '{item}'."
                ));
            }
            out.push(if i == 0 && item.eq_ignore_ascii_case("NULL") {
                String::new()
            } else {
                item.clone()
            });
        }
        self.enums.push(PdefEnum {
            name: name.to_string(),
            items: out,
            source: source.to_string(),
            placeholder,
        });
        Ok(())
    }

    /// Enum names match ignoring ASCII case (engine.dll 0x18020f3d0).
    pub fn find_enum(&self, name: &str) -> Option<&PdefEnum> {
        self.enums
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(name))
    }

    /// Item count (engine.dll 0x180211620).
    pub fn enum_count(&self, name: &str) -> Option<i32> {
        self.find_enum(name).map(|e| e.items.len() as i32)
    }

    /// An item's index, matched ignoring ASCII case (0x180216220).
    pub fn enum_index(&self, name: &str, item: &str) -> Option<i32> {
        let e = self.find_enum(name)?;
        e.items
            .iter()
            .position(|i| i.eq_ignore_ascii_case(item))
            .map(|i| i as i32)
    }

    /// An item's name (0x180211650).
    pub fn enum_item(&self, name: &str, index: i32) -> Option<&str> {
        let e = self.find_enum(name)?;
        usize::try_from(index)
            .ok()
            .and_then(|i| e.items.get(i))
            .map(|s| s.as_str())
    }
}

fn s(a: &[Value], i: usize) -> String {
    match a.get(i) {
        Some(Value::Str(s)) => s.to_string_lossy(),
        _ => String::new(),
    }
}

const NOT_LOADED: &str = "Persistence datadef is not loaded.";

pub fn install(cvm: &mut ClientVm) {
    let vm = &mut cvm.vm;
    let def = |host: &Rc<dyn crate::Host>| host.persistence_def().ok_or_else(|| throw(NOT_LOADED));

    // PersistenceGetEnumCount( enum ) (client.dll 0x1802c8e30).
    let host = cvm.host.clone();
    vm.register_checked("PersistenceGetEnumCount", 2, ".s", move |_, a| {
        let d = def(&host)?;
        let name = s(a, 1);
        d.enum_count(&name)
            .map(Value::Int)
            .ok_or_else(|| throw(format!("Unknown persistent enum \"{name}\".")))
    });

    // PersistenceGetEnumIndexForItemName( enum, item ) (0x1802c8ea0).
    let host = cvm.host.clone();
    vm.register_checked(
        "PersistenceGetEnumIndexForItemName",
        3,
        ".ss",
        move |_, a| {
            let d = def(&host)?;
            d.enum_index(&s(a, 1), &s(a, 2))
                .map(Value::Int)
                .ok_or_else(|| throw("Invalid enum or item name."))
        },
    );

    // PersistenceGetEnumItemNameForIndex( enum, index ) (0x1802c8f30).
    let host = cvm.host.clone();
    vm.register_checked(
        "PersistenceGetEnumItemNameForIndex",
        3,
        ".sn",
        move |_, a| {
            let d = def(&host)?;
            let index = match a[2] {
                Value::Int(n) => n,
                Value::Float(x) => x as i32,
                _ => 0,
            };
            d.enum_item(&s(a, 1), index)
                .map(Value::str)
                .ok_or_else(|| throw("Invalid enum or item index."))
        },
    );

    // PersistenceEnumValueIsValid( enum, item ) (0x1802c8fc0): an unknown
    // enum is an error, an unknown item just false.
    let host = cvm.host.clone();
    vm.register_checked("PersistenceEnumValueIsValid", 3, ".ss", move |_, a| {
        let d = def(&host)?;
        let name = s(a, 1);
        if d.find_enum(&name).is_none() {
            return Err(throw(format!("Unknown persistent enum \"{name}\".")));
        }
        Ok(Value::Bool(d.enum_index(&name, &s(a, 2)).is_some()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn enums_follow_the_engine_rules() {
        let mut d = PersistenceDef::default();
        d.add_enum("Colors", &strings(&["NULL", "red", "Green"]), "test", false)
            .unwrap();
        assert_eq!(d.enum_count("colors"), Some(3));
        assert_eq!(d.enum_index("COLORS", "green"), Some(2));
        assert_eq!(d.enum_index("colors", ""), Some(0));
        assert_eq!(d.enum_item("colors", 0), Some(""));
        assert_eq!(d.enum_item("colors", 3), None);
        assert_eq!(d.enum_item("colors", -1), None);
        assert!(d.add_enum("colors", &strings(&["x"]), "", false).is_err());
        assert_eq!(
            d.add_enum("dup", &strings(&["a", "A"]), "", false),
            Err("Enum 'dup' already has an entry named 'A'.".into())
        );
    }
}
