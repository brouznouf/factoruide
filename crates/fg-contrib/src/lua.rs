//! Loading Lua data files (Questie exports, SavedVariables) into plain Rust values.
//!
//! Lua tables keep their keys so that sparse maps (`{[12] = ...}`) and positional rows
//! with holes (`{"a", nil, 3}`) stay distinguishable.

use anyhow::{Context, Result};
use mlua::{Lua, Value as LV};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    Int(i64),
    Str(String),
}

#[derive(Debug, Clone)]
pub enum Val {
    Nil,
    Bool(bool),
    Num(f64),
    Str(String),
    Table(BTreeMap<Key, Val>),
}

impl Val {
    pub fn get(&self, i: i64) -> &Val {
        match self {
            Val::Table(t) => t.get(&Key::Int(i)).unwrap_or(&Val::Nil),
            _ => &Val::Nil,
        }
    }

    pub fn field(&self, name: &str) -> &Val {
        match self {
            Val::Table(t) => t.get(&Key::Str(name.to_owned())).unwrap_or(&Val::Nil),
            _ => &Val::Nil,
        }
    }

    pub fn int(&self) -> Option<i64> {
        match self {
            Val::Num(n) => Some(*n as i64),
            Val::Str(s) => s.parse().ok(),
            _ => None,
        }
    }

    pub fn num(&self) -> Option<f64> {
        match self {
            Val::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Val::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, Val::Nil)
    }

    /// Integer-keyed entries in key order (handles holes).
    pub fn entries(&self) -> Vec<(i64, &Val)> {
        match self {
            Val::Table(t) => t
                .iter()
                .filter_map(|(k, v)| match k {
                    Key::Int(i) => Some((*i, v)),
                    Key::Str(_) => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Values of a list, ignoring holes.
    pub fn list(&self) -> Vec<&Val> {
        self.entries().into_iter().map(|(_, v)| v).collect()
    }

    pub fn ints(&self) -> Vec<i64> {
        self.list().into_iter().filter_map(Val::int).collect()
    }

    pub fn str_entries(&self) -> Vec<(&str, &Val)> {
        match self {
            Val::Table(t) => t
                .iter()
                .filter_map(|(k, v)| match k {
                    Key::Str(s) => Some((s.as_str(), v)),
                    Key::Int(_) => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::Value as J;
        match self {
            Val::Nil => J::Null,
            Val::Bool(b) => J::Bool(*b),
            Val::Num(n) if n.fract() == 0.0 && n.abs() < 9e15 => J::from(*n as i64),
            Val::Num(n) => J::from(*n),
            Val::Str(s) => J::String(s.clone()),
            Val::Table(t) => {
                let dense = t.keys().enumerate().all(|(i, k)| *k == Key::Int(i as i64 + 1));
                if dense {
                    J::Array(t.values().map(Val::to_json).collect())
                } else {
                    J::Object(
                        t.iter()
                            .map(|(k, v)| {
                                let k = match k {
                                    Key::Int(i) => i.to_string(),
                                    Key::Str(s) => s.clone(),
                                };
                                (k, v.to_json())
                            })
                            .collect(),
                    )
                }
            }
        }
    }
}

pub fn convert(v: &LV) -> Result<Val> {
    Ok(match v {
        LV::Boolean(b) => Val::Bool(*b),
        LV::Integer(i) => Val::Num(*i as f64),
        LV::Number(n) => Val::Num(*n),
        LV::String(s) => Val::Str(s.to_string_lossy().clone()),
        LV::Table(t) => {
            let mut map = BTreeMap::new();
            for pair in t.clone().pairs::<LV, LV>() {
                let (k, v) = pair?;
                let key = match k {
                    LV::Integer(i) => Key::Int(i),
                    LV::Number(n) if n.fract() == 0.0 => Key::Int(n as i64),
                    LV::String(s) => Key::Str(s.to_string_lossy().clone()),
                    other => Key::Str(format!("{other:?}")),
                };
                map.insert(key, convert(&v)?);
            }
            Val::Table(map)
        }
        _ => Val::Nil,
    })
}

pub fn new_lua() -> Lua {
    Lua::new()
}

/// Run a Lua file and convert its return value.
pub fn eval_file(lua: &Lua, path: &Path) -> Result<Val> {
    let src = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let v: LV = lua
        .load(&src[..])
        .set_name(path.display().to_string())
        .eval()
        .with_context(|| format!("evaluating {}", path.display()))?;
    convert(&v)
}

/// Run a SavedVariables-like file (assigns globals) and return the named globals.
pub fn load_globals(lua: &Lua, path: &Path, names: &[&str]) -> Result<Vec<Val>> {
    let src = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    lua.load(&src[..])
        .set_name(path.display().to_string())
        .exec()
        .with_context(|| format!("executing {}", path.display()))?;
    names.iter().map(|n| convert(&lua.globals().get::<LV>(*n)?)).collect()
}
