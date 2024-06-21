pub mod read_file;

use std::collections::HashMap;
use std::mem;
use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use json_flat_parser::{FlatJsonValue, JsonArrayEntries, JSONParser, ParseOptions, ParseResult, PointerKey, ValueType};
use rayon::iter::ParallelIterator;
use rayon::iter::IndexedParallelIterator;
use rayon::iter::IntoParallelIterator;
use rayon::prelude::{ParallelSlice, ParallelSliceMut};
use crate::array_table::{Column, NON_NULL_FILTER_VALUE};
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


pub fn change_depth_array(previous_parse_result: ParseResult<String>, mut json_array: Vec<JsonArrayEntries<String>>, depth: usize) -> Result<(Vec<JsonArrayEntries<String>>, Vec<Column>, usize), String> {
    let len = json_array.len();
    let mut new_json_array = Arc::new(Mutex::new(Vec::with_capacity(json_array.len())));
    let start = Instant::now();
    let mut chunks = json_array.par_chunks_mut(len / 8);

    let unique_keys_by_chunks = chunks.into_par_iter().map(|mut chunk| {
        let mut unique_keys: Vec<Column> = Vec::with_capacity(16);
        for json_array_entry in chunk {
            let mut parse_result = previous_parse_result.clone_except_json();
            parse_result.json = mem::take(&mut json_array_entry.entries);
            let mut options = ParseOptions::default().parse_array(false).max_depth(depth as u8);
            JSONParser::change_depth_owned(&mut parse_result, options).unwrap();
            let mut vec = parse_result.json;

            for j in 0..vec.len() {
                let entry = &mut vec[j];
                let _i = json_array_entry.index.to_string();
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
                if !entry.pointer.pointer.is_empty() {
                    if entry.pointer.pointer.len() <= prefix_len {
                        // panic!("ERROR, depth {} out of bounds of {}, expected to have a prefix of len {}", depth, entry.pointer.pointer, prefix_len);
                        continue;
                    }
                    let key = &entry.pointer.pointer[prefix_len..entry.pointer.pointer.len()];
                    let column = Column {
                        name: key.to_string(),
                        depth: entry.pointer.depth,
                        value_type: entry.pointer.value_type,
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
                entry.pointer.index = json_array_entry.index;
            }
            let mut new_json_array_guard = new_json_array.lock().unwrap();
            new_json_array_guard.push(JsonArrayEntries::<String> { entries: vec, index: json_array_entry.index });
        }
        unique_keys
    }).collect::<Vec<Vec<Column>>>();
    let mut unique_keys: Vec<Column> = Vec::with_capacity(unique_keys_by_chunks[0].len() + 16);
    for unique_keys_chunk in unique_keys_by_chunks {
        for column_chunk in unique_keys_chunk {
            if let Some(column) = unique_keys.iter_mut().find(|c| c.eq(&&column_chunk)) {
                column.seen_count += column_chunk.seen_count;
            } else {
                if !column_chunk.name.contains("#") {
                    unique_keys.push(column_chunk);
                }
            }
        }
    }
    let mut new_json_array_guard = new_json_array.lock().unwrap();
    new_json_array_guard.sort_unstable_by(|a, b| a.index.cmp(&b.index));
    unique_keys.sort();

    Ok((mem::take(&mut new_json_array_guard), unique_keys, 4))
}
pub fn as_array(mut previous_parse_result: ParseResult<String>) -> Result<(Vec<JsonArrayEntries<String>>, Vec<Column>), String> {
    if !matches!(previous_parse_result.json[0].pointer.value_type, ValueType::Array(_)) {
        return Err("Parsed json root is not an array".to_string());
    }
    let root_array_len = match previous_parse_result.json[0].pointer.value_type {
        ValueType::Array(root_array_len) => root_array_len,
        _ => panic!("")
    };
    let mut unique_keys: Vec<Column> = Vec::with_capacity(16);
    let mut res: Vec<JsonArrayEntries<String>> = Vec::with_capacity(root_array_len);
    let mut j = previous_parse_result.json.len() - 1;
    let mut estimated_capacity = 16;
    for i in (0..root_array_len).rev() {
        let mut flat_json_values: Vec<FlatJsonValue<String>> = Vec::with_capacity(estimated_capacity);
        let mut is_first_entry = true;
        let _i = i.to_string();
        loop {
            if j >= 0 && !previous_parse_result.json.is_empty() {
                let entry = &previous_parse_result.json[j];
                let (match_prefix, prefix_len) = if let Some(ref started_parsing_at) = previous_parse_result.started_parsing_at {
                    let prefix = concat_string!(started_parsing_at, "/", _i);
                    // println!("else if {}", prefix);
                    (entry.pointer.pointer.starts_with(&prefix), prefix.len())
                } else if let Some(ref prefix) = previous_parse_result.parsing_prefix {
                    let prefix = concat_string!(prefix, "/", _i);
                    // println!("else if {}", prefix);
                    (entry.pointer.pointer.starts_with(&prefix), prefix.len())
                } else {
                    let prefix = concat_string!("/", _i);
                    // println!("else {}", prefix);
                    (entry.pointer.pointer.starts_with(&prefix), prefix.len())
                };

                if match_prefix {
                    if !entry.pointer.pointer.is_empty() {
                        if entry.pointer.pointer.len() < prefix_len {
                            panic!("{} len is < {}", entry.pointer.pointer, prefix_len);
                        }
                        let key = &entry.pointer.pointer[prefix_len..entry.pointer.pointer.len()];
                        let column = Column {
                            name: key.to_string(),
                            depth: entry.pointer.depth,
                            value_type: entry.pointer.value_type,
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
                        let prefix = &entry.pointer.pointer[0..prefix_len];
                        flat_json_values.push(FlatJsonValue { pointer: PointerKey::from_pointer_and_index(concat_string!(prefix, "/#"), ValueType::Number, 0, i, entry.pointer.position), value: Some(i.to_string())});
                    }
                    let mut entry= previous_parse_result.json.pop().unwrap();
                    entry.pointer.index = i;
                    flat_json_values.push(entry);
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
        res.push(JsonArrayEntries::<String> { entries: flat_json_values, index: i });
    }
    res.reverse();
    unique_keys.sort();
    Ok((res, unique_keys))
}

pub fn filter_columns(previous_parse_result: &Vec<JsonArrayEntries<String>>, prefix: &str, filters: &HashMap<String, Vec<String>>) -> Vec<usize> {
    let mut res: Vec<usize> = Vec::with_capacity(previous_parse_result.len());
    for row in previous_parse_result {
        let mut should_add_row = true;
        for (pointer, filters) in filters {
            let pointer_to_find = concat_string!(prefix, "/", row.index().to_string(), pointer);
            let mut filters_clone = Vec::with_capacity(filters.len());
            let mut should_filter_by_non_null = false;
            for filter in filters {
                if filter.eq(NON_NULL_FILTER_VALUE) {
                    should_filter_by_non_null = true;
                } else {
                    filters_clone.push(filter.clone());
                }
            }
            if let Some(entry) = row.find_node_at(&pointer_to_find) {
                if should_filter_by_non_null && entry.value.is_none() {
                    should_add_row = false;
                    break;
                }
                if !filters_clone.is_empty() {
                    if !filters_clone.contains(entry.value.as_ref().unwrap()) {
                        should_add_row = false;
                        break;
                    }
                }
            } else {
                should_add_row = false;
                break;
            }
        }

        if should_add_row {
            res.push(row.index);
        }
    }
    res
}
pub fn search_occurrences(previous_parse_result: &Vec<JsonArrayEntries<String>>, term: &str) -> Vec<usize> {
    let mut res: Vec<usize> = vec![];
    for json_array_entry in previous_parse_result.iter() {
        for entry in &json_array_entry.entries {
            if !matches!(entry.pointer.value_type, ValueType::String) {
                continue;
            }
            if let Some(ref value) = entry.value {
                if value.to_lowercase().contains(term) {
                    res.push(json_array_entry.index);
                    break;
                }
            }
        }
    }
    res
}
