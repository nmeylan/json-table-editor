use std::cmp::Ordering;

use std::hash::{Hash, Hasher};

type PointerFragment = Vec<String>;
type ValueMap = Vec<(PointerKey, Option<String>)>;

#[derive(Debug)]
pub struct PointerKey {
    pub pointer: String,
    pub value_type: ValueType,
    pub index: usize,
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
    pub fn from_pointer(pointer: String, value_type: ValueType, index: usize) -> Self {
        Self {
            pointer,
            value_type,
            index,
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


pub fn flatten(values: &Vec<Value>, max_depth: u8, non_null_columns: &Vec<String>) -> (Vec<Vec<(PointerKey, Option<String>)>>, Vec<Column>) {
    let mut rows = Vec::with_capacity(values.len());
    let mut unique_keys: Vec<Column> = Vec::with_capacity(1000);
    let mut i = 0;
    for value in values {
        let mut columns: Vec<(PointerKey, Option<String>)> =  Vec::with_capacity(100);
        columns.push((PointerKey::from_pointer("#".to_string(), ValueType::Number, i), Some(format!("{}", i))));
        let mut pointer_fragment: Vec<String> = Vec::with_capacity(max_depth as usize);
        process(value, &mut unique_keys, &mut pointer_fragment, &mut columns, 0, max_depth as i32, i);
        let mut should_add_row = true;
        for non_null_column in non_null_columns {
            if let Some((_, value)) = columns.iter().find(|(p, _)| p.pointer.eq(non_null_column)) {
                if value.is_none() {
                    should_add_row = false;
                    break;
                }
            } else {
                should_add_row = false;
                break;
            }
        }
        if should_add_row {
            rows.push(columns);
        }
        i += 1;
    }
    (rows, unique_keys)
}

pub fn value_at(value: &Value, pointer: &str) -> Option<Value> {
    let mut pointer_fragment: Vec<String> = Vec::with_capacity(10);
    process_value_at(value, &mut pointer_fragment, pointer)
}

fn process_value_at(value: &Value, route: &mut PointerFragment, pointer_to_match: &str) -> Option<Value> {

    match value {
        Value::Array(arr) => {
            let mut i = 0;
            for val in arr {
                let pointer = route.concat();
                if pointer.eq(pointer_to_match) {
                    return Some(value.clone());
                }
                route.push(format!("/{}", i));
                i += 1;
                if let Some(found) = process_value_at(val, route, pointer_to_match) {
                    return Some(found);
                }
            }
        }
        Value::Object(obj) => {
            let pointer = route.concat();
            if pointer.eq(pointer_to_match) {
                return Some(value.clone());
            }
            for (key, val) in obj {
                route.push(format!("/{}", escape(key.as_str())));
                if let Some(found) = process_value_at(val, route, pointer_to_match) {
                    return Some(found);
                }
            }
        }
        _ => {
            let pointer = route.concat();
            if pointer.eq(pointer_to_match) {
                return Some(value.clone());
            }
        }
    }
    route.pop();
    None
}

pub fn process(value: &Value, unique_keys: &mut Vec<Column>, route: &mut PointerFragment, target: &mut ValueMap, depth: i32, max_depth: i32, count: usize) {
    let pointer = route.concat();

    if !pointer.is_empty() {
        let column = Column {
            name: pointer.clone(),
            depth: depth as u8,
        };
        if !unique_keys.contains(&column) {
            unique_keys.push(column);
        }
    }
    match value {
        Value::Null => {
            target.push((PointerKey::from_pointer(pointer, ValueType::Null, count), None));
        }
        Value::Bool(_b) => {
            target.push((PointerKey::from_pointer(pointer, ValueType::Bool, count), format!("{}", value).into()));
        }
        Value::Number(_n) => {
            target.push((PointerKey::from_pointer(pointer, ValueType::Number, count), format!("{}", value).into()));
        }
        Value::String(_s) => {
            target.push((PointerKey::from_pointer(pointer, ValueType::String, count), format!("{}", value).into()));
        }
        Value::Array(_arr) => {
            target.push((PointerKey::from_pointer(pointer, ValueType::Array, count), format!("{}", value).into()));
        }
        Value::Object(obj) => {
            if depth < max_depth {
                for (key, val) in obj {
                    route.push(format!("/{}", escape(key.as_str())));
                    process(val, unique_keys, route, target, depth + 1, max_depth, count);
                }
            } else {
                target.push((PointerKey::from_pointer(route.concat(), ValueType::Object, count), format!("{}", value).into()));
            }
        }
    }
    route.pop();
}

fn escape(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

#[allow(dead_code)]
fn unescape(value: &str) -> String {
    value.replace("~1", "/").replace("~0", "~")
}
