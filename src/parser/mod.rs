pub mod read_file;

use std::time::Instant;
use json_flat_parser::{FlatJsonValue, FlatJsonValueOwned, JsonArrayEntriesOwned, JSONParser, ParseOptions, ParseResultOwned, PointerKey, ValueType};
use json_flat_parser::lexer::Lexer;
use json_flat_parser::parser::Parser;
use crate::array_table::Column;
#[macro_export]
macro_rules! concat_string {
    () => { String::with_capacity(0) };
    ($($s:expr),+) => {{
        use std::ops::AddAssign;
        let mut len = 0;
        $(len.add_assign(AsRef::<str>::as_ref(&$s).len());)+
        let mut buf = String::with_capacity(len);
        $(buf.push_str($s.as_ref());)+
        buf
    }};
}


pub fn change_depth_array(previous_parse_result: ParseResultOwned, mut json_array: Vec<JsonArrayEntriesOwned>, depth: usize) -> Result<(Vec<JsonArrayEntriesOwned>, Vec<Column>), String> {
    let len = json_array.len();
    let mut new_json_array = Vec::with_capacity(json_array.len());
    let mut unique_keys: Vec<Column> = Vec::with_capacity(16);
    let start = Instant::now();
    for i in (0..len).rev() {
        let mut parse_result = previous_parse_result.clone_except_json();
        parse_result.json = json_array.pop().unwrap().entries;
        let mut options = ParseOptions::default().parse_array(false).max_depth(depth as u8);
        JSONParser::change_depth_owned(&mut parse_result, options)?;
         let mut vec = parse_result.json;

        for j in 0..vec.len() {
            let (k, _v) = &mut vec[j];
            let _i = i.to_string();
            let (prefix_len) = if let Some(ref started_parsing_at) = previous_parse_result.started_parsing_at {
                let prefix = concat_string!(started_parsing_at, "/", _i);
                prefix.len()
            } else if let Some(ref prefix) = previous_parse_result.parsing_prefix {
                let prefix = concat_string!(prefix, "/", _i);
                prefix.len()
            } else {
                let prefix = concat_string!("/", _i);
                prefix.len()
            };
            if !k.pointer.is_empty() {
                if k.pointer.len() <= prefix_len {
                    // panic!("ERROR, depth {} out of bounds of {}, expected to have a prefix of len {}", depth, k.pointer, prefix_len);
                    continue;
                }
                let key = &k.pointer[prefix_len..k.pointer.len()];
                let column = Column {
                    name: key.to_string(),
                    depth: k.depth,
                    value_type: k.value_type,
                    seen_count: 0,
                    order: unique_keys.len(),
                };
                if let Some(column) = unique_keys.iter_mut().find(|c| c.eq(&&column)) {
                    column.seen_count += 1;
                } else {
                    if !column.name.contains("#") {
                        unique_keys.push(column);
                    }
                }
            }
            k.index = i;
        }
        new_json_array.push(JsonArrayEntriesOwned { entries: vec, index: i });
    }
    new_json_array.reverse();
    unique_keys.sort();
    println!("took {}ms to change depth", start.elapsed().as_millis());
    Ok((new_json_array, unique_keys))
}


pub fn as_array(mut previous_parse_result: ParseResultOwned) -> Result<(Vec<JsonArrayEntriesOwned>, Vec<Column>), String> {
    if !matches!(previous_parse_result.json[0].0.value_type, ValueType::Array(_)) {
        return Err("Parsed json root is not an array".to_string());
    }
    let root_array_len = match previous_parse_result.json[0].0.value_type {
        ValueType::Array(root_array_len) => root_array_len,
        _ => panic!("")
    };
    let mut unique_keys: Vec<Column> = Vec::with_capacity(16);
    let mut res: Vec<JsonArrayEntriesOwned> = Vec::with_capacity(root_array_len);
    let mut j = previous_parse_result.json.len() - 1;
    let mut estimated_capacity = 16;
    for i in (0..root_array_len).rev() {
        let mut flat_json_values = FlatJsonValueOwned::with_capacity(estimated_capacity);
        let mut is_first_entry = true;
        let _i = i.to_string();
        loop {
            if j >= 0 && !previous_parse_result.json.is_empty() {
                let (k, _v) = &previous_parse_result.json[j];
                let (match_prefix, prefix_len) = if let Some(ref started_parsing_at) = previous_parse_result.started_parsing_at {
                    let prefix = concat_string!(started_parsing_at, "/", _i);
                    // println!("else if {}", prefix);
                    (k.pointer.starts_with(&prefix), prefix.len())
                } else if let Some(ref prefix) = previous_parse_result.parsing_prefix {
                    let prefix = concat_string!(prefix, "/", _i);
                    // println!("else if {}", prefix);
                    (k.pointer.starts_with(&prefix), prefix.len())
                } else {
                    let prefix = concat_string!("/", _i);
                    // println!("else {}", prefix);
                    (k.pointer.starts_with(&prefix), prefix.len())
                };

                if match_prefix {
                    if !k.pointer.is_empty() {
                        if k.pointer.len() < prefix_len{
                            panic!("{} len is < {}", k.pointer, prefix_len);
                        }
                        let key = &k.pointer[prefix_len..k.pointer.len()];
                        let column = Column {
                            name: key.to_string(),
                            depth: k.depth,
                            value_type: k.value_type,
                            seen_count: 1,
                            order: unique_keys.len(),
                        };
                        if let Some(column) = unique_keys.iter_mut().find(|c| c.eq(&&column)) {
                            column.seen_count += 1;
                        } else {
                            unique_keys.push(column);
                        }
                    }
                    if is_first_entry {
                        is_first_entry = false;
                        let prefix = &k.pointer[0..prefix_len];
                        flat_json_values.push((PointerKey::from_pointer_and_index(concat_string!(prefix, "/#"), ValueType::Number, k.depth, i, k.position), Some(i.to_string())));
                    }
                    let (mut k, v) = previous_parse_result.json.pop().unwrap();
                    k.index = i;
                    flat_json_values.push((k, v));
                } else {
                    break;
                }
                if j == 0 {
                    break;
                }
                j -= 1;
            } else {
                break;
            }
        }
        res.push(JsonArrayEntriesOwned { entries: flat_json_values, index: i });
    }
    res.reverse();
    unique_keys.sort();
    Ok((res, unique_keys))
}

pub fn filter_non_null_column(previous_parse_result: &Vec<JsonArrayEntriesOwned>, prefix: &str, non_null_columns: &Vec<String>) -> Vec<JsonArrayEntriesOwned> {
    let mut res: Vec<JsonArrayEntriesOwned> = Vec::with_capacity(previous_parse_result.len());
    for row in previous_parse_result {
        let mut should_add_row = true;
        for pointer in non_null_columns {
            let pointer_to_find = concat_string!(prefix, "/", row.index().to_string(), pointer);
            if let Some((_, value)) = row.find_node_at(&pointer_to_find) {
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
            res.push(row.clone());
        }
    }
    res
}
