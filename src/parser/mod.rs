pub mod read_file;

use std::collections::HashMap;
use std::mem;
use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use json_flat_parser::{FlatJsonValue, FlatJsonValueOwned, JsonArrayEntriesOwned, JSONParser, ParseOptions, ParseResultOwned, PointerKey, ValueType};
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


pub fn change_depth_array(previous_parse_result: ParseResultOwned, mut json_array: Vec<JsonArrayEntriesOwned>, depth: usize) -> Result<(Vec<JsonArrayEntriesOwned>, Vec<Column>, usize), String> {
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
                let (k, _v) = &mut vec[j];
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
                k.index = json_array_entry.index;
            }
            let mut new_json_array_guard = new_json_array.lock().unwrap();
            new_json_array_guard.push(JsonArrayEntriesOwned { entries: vec, index: json_array_entry.index });
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
// pub fn change_depth_array(previous_parse_result: ParseResultOwned, mut json_array: Vec<JsonArrayEntriesOwned>, depth: usize) -> Result<(Vec<JsonArrayEntriesOwned>, Vec<Column>, usize), String> {
//     let len = json_array.len();
//     let mut new_json_array = Vec::with_capacity(json_array.len());
//     let mut unique_keys: Vec<Column> = Vec::with_capacity(16);
//     let start = Instant::now();
//     let mut max_depth = previous_parse_result.max_json_depth;
//     for i in (0..len).rev() {
//         let mut parse_result = previous_parse_result.clone_except_json();
//         parse_result.json = json_array.pop().unwrap().entries;
//         let mut options = ParseOptions::default().parse_array(false).max_depth(depth as u8);
//         JSONParser::change_depth_owned(&mut parse_result, options)?;
//         if max_depth < parse_result.max_json_depth {
//             max_depth = parse_result.max_json_depth;
//         }
//         let mut vec = parse_result.json;
//         for j in 0..vec.len() {
//             let (k, _v) = &mut vec[j];
//             let _i = i.to_string();
//             let (prefix_len) = if let Some(ref started_parsing_at) = previous_parse_result.started_parsing_at {
//                 let prefix = concat_string!(started_parsing_at, "/", _i);
//                 prefix.len()
//             } else if let Some(ref prefix) = previous_parse_result.parsing_prefix {
//                 let prefix = concat_string!(prefix, "/", _i);
//                 prefix.len()
//             } else {
//                 let prefix = concat_string!("/", _i);
//                 prefix.len()
//             };
//             if !k.pointer.is_empty() {
//                 if k.pointer.len() <= prefix_len {
//                     // panic!("ERROR, depth {} out of bounds of {}, expected to have a prefix of len {}", depth, k.pointer, prefix_len);
//                     continue;
//                 }
//                 let key = &k.pointer[prefix_len..k.pointer.len()];
//                 let column = Column {
//                     name: key.to_string(),
//                     depth: k.depth,
//                     value_type: k.value_type,
//                     seen_count: 0,
//                     order: unique_keys.len(),
//                 };
//                 if let Some(column) = unique_keys.iter_mut().find(|c| c.eq(&&column)) {
//                     column.seen_count += 1;
//                 } else {
//                     if !column.name.contains("#") {
//                         unique_keys.push(column);
//                     }
//                 }
//             }
//             k.index = i;
//         }
//         new_json_array.push(JsonArrayEntriesOwned { entries: vec, index: i });
//     }
//     new_json_array.reverse();
//     // new_json_array[0].entries.iter().for_each(|(k, v)| println!("{} {} {}", k.pointer, k.depth, v.is_some()));
//     unique_keys.sort();
//     println!("took {}ms to change depth", start.elapsed().as_millis());
//     Ok((new_json_array, unique_keys, max_depth))
// }

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
                        if k.pointer.len() < prefix_len {
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
                        flat_json_values.push((PointerKey::from_pointer_and_index(concat_string!(prefix, "/#"), ValueType::Number, 0, i, k.position), Some(i.to_string())));
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

pub fn filter_columns(previous_parse_result: &Vec<JsonArrayEntriesOwned>, prefix: &str, filters: &HashMap<String, Vec<String>>) -> Vec<JsonArrayEntriesOwned> {
    let mut res: Vec<JsonArrayEntriesOwned> = Vec::with_capacity(previous_parse_result.len());
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
            if let Some((_, value)) = row.find_node_at(&pointer_to_find) {
                if should_filter_by_non_null && value.is_none() {
                    should_add_row = false;
                    break;
                }
                if !filters_clone.is_empty() {
                    if !filters_clone.contains(value.as_ref().unwrap()) {
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
            res.push(row.clone());
        }
    }
    res
}
pub fn search_occurrences(previous_parse_result: &Vec<JsonArrayEntriesOwned>, term: &str) -> Vec<usize> {
    let mut res: Vec<usize> = vec![];
    for json_array_entry in previous_parse_result.iter() {
        for (p, v) in &json_array_entry.entries {
            if !matches!(p.value_type, ValueType::String) {
                continue;
            }
            if let Some(value) = v {
                if value.to_lowercase().contains(term) {
                    res.push(json_array_entry.index);
                    break;
                }
            }
        }
    }
    res
}
