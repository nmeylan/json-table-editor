use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::format;
use crate::parser::{FlatJsonValue, JSONParser, ParseOptions, PointerKey, ValueType};

#[derive(Debug)]
pub enum Value {
    Object(HashMap<String, Value>),
    Array(Vec<Value>),
    Number(f64),
    String(String),
    Bool(bool),
    Null,
}

fn serialize_to_json(mut data: FlatJsonValue) -> Value {
    let mut root = Value::Object(HashMap::with_capacity(128));

    let mut sorted_data = data;
    sorted_data.sort_by(|(a, _), (b, _)|
        // deepest values will go first, because we will iterate in reverse order from the array to pop value
        match b.depth.cmp(&a.depth) {
            Ordering::Equal => b.pointer.cmp(&a.pointer),
            cmp => cmp,
        });

    for i in (0..sorted_data.len()) {
        let (key, value) = sorted_data.pop().unwrap();
        let mut current_parent = &mut root;

        if key.depth == 1 {
            match current_parent {
                Value::Object(obj) => {
                    if matches!(key.value_type, ValueType::Object) {
                        obj.insert(key.pointer[1..].to_string(), Value::Object(HashMap::with_capacity(128)));
                    } else {
                        obj.insert(key.pointer[1..].to_string(), value_to_json(value, &key.value_type));
                    }
                }
                _ => panic!("only Object is accepted for root node")
            }
        } else {
            let parent = key.parent();
            let segments: Vec<&str> = key.pointer.split('/').filter(|s| !s.is_empty()).collect();
            let mut k = "";
            for j in 0..(segments.len() - 1) {
                let s = segments[j];
                match current_parent {
                    Value::Object(ref mut obj) => {
                        k = s;
                        current_parent = obj.get_mut(s).expect(format!("Expected to find parent for {}, current segment {}", key.pointer, s).as_str());
                    }
                    _ => panic!("only Object is accepted for root node")
                }
                println!("{} | {} | {:?}", key.depth, key.pointer, parent);
            }
            k = segments[segments.len() - 1];
            match current_parent {
                Value::Object(obj) => {
                    if matches!(key.value_type, ValueType::Object) {
                        obj.insert(k.to_string(), Value::Object(HashMap::with_capacity(128)));
                    } else {
                        obj.insert(k.to_string(), value_to_json(value, &key.value_type));
                    }
                }
                _ => panic!("only Object is accepted for root node")
            }
        }

    }

    root
}

// Helper function to convert string values to JSON values based on ValueType
fn value_to_json(value: Option<String>, value_type: &ValueType) -> Value {
    if let Some(value) = value {
        match value_type {
            ValueType::Number => value.parse::<f64>().map(Value::Number).unwrap_or(Value::Null),
            ValueType::String => Value::String(value),
            ValueType::Bool => Value::Bool(value == "true" || value == "1"),
            ValueType::Null => Value::Null,
            _ => Value::Null, // this should not happen as arrays and objects are handled separately
        }
    } else {
        Value::Null
    }
}

impl Value {
    fn to_json(&self) -> String {
        match self {
            Value::Object(obj) => {
                let members: Vec<String> = obj.iter().map(|(k, v)| format!("\"{}\":{}", k, v.to_json())).collect();
                format!("{{{}}}", members.join(","))
            }
            Value::Array(arr) => {
                let elements: Vec<String> = arr.iter().map(|v| v.to_json()).collect();
                format!("[{}]", elements.join(","))
            }
            Value::Number(num) => num.to_string(),
            Value::String(s) => format!("\"{}\"", s.replace("\"", "\\\"")),
            Value::Bool(b) => b.to_string(),
            Value::Null => "null".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::parser::{JSONParser, ParseOptions};
    use crate::parser::serializer::serialize_to_json;

    #[test]
    fn nested_object() {
        let json = r#"
        {
          "id": 1,
          "maxLevel": 99,
          "name": "NV_BASIC",
          "aaa": true,
          "bbb": null,
          "flags": {"a": true, "b": false, "c": {"nested": "Oui"}}
        }"#;

        let mut parser = JSONParser::new(json);
        let vec = parser.parse(ParseOptions::default()).unwrap().json;
        let value = serialize_to_json(vec);
        println!("{}", value.to_json());
    }
    #[test]
    fn simple_array() {
        let json = r#"
            [1,2,3]
        "#;

        let mut parser = JSONParser::new(json);
        let res = parser.parse(ParseOptions::default()).unwrap();
        let vec = res.json;
        let value = serialize_to_json(vec);
        println!("{:?}", value.to_json());
    }
}