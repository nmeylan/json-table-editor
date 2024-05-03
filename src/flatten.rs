use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use serde_json::Value;

type PointerFragment = Vec<String>;
type ValueMap = Vec<(PointerKey, Option<String>)>;

#[derive(Debug)]
pub struct PointerKey {
    pub pointer: String,
    pub value_type: ValueType,
}

impl PartialEq<Self> for PointerKey {
    fn eq(&self, other: &Self) -> bool {
        self.pointer.eq(&other.pointer)
    }
}

impl Eq for PointerKey{}

impl Hash for PointerKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pointer.hash(state);
    }
}

impl PointerKey {
    pub fn from_pointer(pointer: String, value_type: ValueType) -> Self {
        Self {
            pointer,
            value_type,
        }
    }
    pub fn wrap(pointer: String) -> Self {
        Self {
            pointer,
            value_type: ValueType::None,
        }
    }
}

#[derive(Eq, Hash, PartialEq, Debug)]
pub enum ValueType {
    Array,
    Object,
    Number,
    String,
    Bool,
    Null,
    None,
}


#[derive(Clone, Debug)]
pub struct Column {
    pub(crate) name: String,
    pub(crate) depth: u8,
}

impl Eq for Column {}

impl PartialEq<Self> for Column {
    fn eq(&self, other: &Self) -> bool {
        self.name.eq(&other.name)
    }
}

impl PartialOrd<Self> for Column {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.name.cmp(&other.name))
    }
}

impl Ord for Column {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

pub fn flatten(values: &Vec<Value>, max_depth: u8) -> (Vec<Vec<(PointerKey, Option<String>)>>, Vec<Column>) {
    let mut rows = Vec::with_capacity(values.len());
    let mut unique_keys: Vec<Column> = Vec::with_capacity(1000);
    for value in values {
        let mut columns: Vec<(PointerKey, Option<String>)> =  Vec::with_capacity(100);
        let mut pointer_fragment: Vec<String> = Vec::with_capacity(max_depth as usize);
        process(value, &mut unique_keys, &mut pointer_fragment, &mut columns, 0, max_depth as i32);
        rows.push(columns);
    }
    (rows, unique_keys)
}

pub fn process(value: &Value, unique_keys: &mut Vec<Column>, route: &mut PointerFragment, target: &mut ValueMap, depth: i32, max_depth: i32) {
    let pointer = route.concat();

    let column = Column {
        name: pointer.clone(),
        depth: depth as u8,
    };
    if !unique_keys.contains(&column) {
        unique_keys.push(column);
    }
    match value {
        Value::Null => {
            target.push((PointerKey::from_pointer(pointer, ValueType::Null), None));
        }
        Value::Bool(b) => {
            target.push((PointerKey::from_pointer(pointer, ValueType::Bool), format!("{}", value).into()));
        }
        Value::Number(n) => {
            target.push((PointerKey::from_pointer(pointer, ValueType::Number), format!("{}", value).into()));
        }
        Value::String(s) => {
            target.push((PointerKey::from_pointer(pointer, ValueType::String), format!("{}", value).into()));
        }
        Value::Array(arr) => {
            target.push((PointerKey::from_pointer(pointer, ValueType::Array), format!("{}", value).into()));
        }
        Value::Object(obj) => {
            if depth < max_depth {
                for (key, val) in obj {
                    route.push(format!("/{}", escape(key.as_str())));
                    process(val, unique_keys, route, target, depth + 1, max_depth);
                }
            } else {
                target.push((PointerKey::from_pointer(route.concat(), ValueType::Object), format!("{}", value).into()));
            }
        }
    }
    route.pop();
}

fn escape<'a>(value: &'a str) -> String {
    value.replace("~", "~0").replace("/", "~1")
}

#[allow(dead_code)]
fn unescape<'a>(value: &'a str) -> String {
    value.replace("~1", "/").replace("~0", "~")
}
