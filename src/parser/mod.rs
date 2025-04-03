use std::borrow::Cow;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hasher};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::{fs, mem};

use crate::array_table::{Column, NON_NULL_FILTER_VALUE};
use crate::panels::{ReplaceMode, SearchReplaceResponse};
use json_flat_parser::{
    FlatJsonValue, JSONParser, JsonArrayEntries, ParseOptions, ParseResult, PointerKey, ValueType,
};
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use rayon::prelude::ParallelSliceMut;
use regex_lite::Regex;

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

pub fn change_depth_array<'array>(
    previous_parse_result: ParseResult<String>,
    mut json_array: Vec<JsonArrayEntries<String>>,
    depth: usize,
) -> Result<(Vec<JsonArrayEntries<String>>, Vec<Column<'array>>, usize), String> {
    let mut len = json_array.len();
    let new_json_array = Arc::new(Mutex::new(Vec::with_capacity(json_array.len())));

    if len < 8 {
        len = 8;
    }
    let chunks = json_array.par_chunks_mut(len / 8);

    let unique_keys_by_chunks = chunks
        .into_par_iter()
        .map(|chunk| {
            let mut unique_keys: Vec<Column> = Vec::with_capacity(16);
            for json_array_entry in chunk {
                let mut parse_result = previous_parse_result.clone_except_json();
                parse_result.json = mem::take(&mut json_array_entry.entries);
                let options = ParseOptions::default()
                    .parse_array(false)
                    .max_depth(depth as u8);
                let last_index = parse_result.json.len().max(1) - 1;
                JSONParser::change_depth_owned(&mut parse_result, options).unwrap();
                let new_last_index = parse_result.json.len().max(1) - 1;
                parse_result.json.swap(last_index, new_last_index);
                let mut vec = parse_result.json;

                for j in 0..vec.len() {
                    let entry = &mut vec[j];
                    let _i = json_array_entry.index.to_string();
                    let prefix_len = if let Some(ref started_parsing_at) =
                        previous_parse_result.started_parsing_at
                    {
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
                        let mut column = Column {
                            name: Cow::from(key.to_string()),
                            depth: entry.pointer.depth,
                            value_type: entry.pointer.value_type,
                            seen_count: 0,
                            order: unique_keys.len(),
                            id: unique_keys.len(),
                        };
                        if let Some(column) = unique_keys.iter_mut().find(|c| c.eq(&&column)) {
                            entry.pointer.column_id = column.id;
                            column.seen_count += 1;
                        } else if !column.name.contains('#') {
                            let mut hasher = DefaultHasher::new();
                            hasher.write(column.name.as_bytes());
                            column.id = hasher.finish() as usize;
                            entry.pointer.column_id = column.id;
                            unique_keys.push(column);
                        }
                    }
                }
                let mut new_json_array_guard = new_json_array.lock().unwrap();
                new_json_array_guard.push(JsonArrayEntries::<String> {
                    entries: vec,
                    index: json_array_entry.index,
                });
            }
            unique_keys
        })
        .collect::<Vec<Vec<Column>>>();
    let mut unique_keys: Vec<Column> = Vec::with_capacity(unique_keys_by_chunks[0].len() + 16);
    for unique_keys_chunk in unique_keys_by_chunks {
        for column_chunk in unique_keys_chunk {
            if let Some(column) = unique_keys.iter_mut().find(|c| c.eq(&&column_chunk)) {
                column.seen_count += column_chunk.seen_count;
            } else if !column_chunk.name.contains('#') {
                unique_keys.push(column_chunk);
            }
        }
    }
    let mut new_json_array_guard = new_json_array.lock().unwrap();
    new_json_array_guard.sort_unstable_by(|a, b| a.index.cmp(&b.index));
    unique_keys.sort();

    Ok((mem::take(&mut new_json_array_guard), unique_keys, 4))
}
pub fn as_array<'array>(
    mut previous_parse_result: ParseResult<String>,
) -> Result<(Vec<JsonArrayEntries<String>>, Vec<Column<'array>>), String> {
    let (root_value, start_index, mut end_index) =
        if let Some(ref started_parsing_at) = previous_parse_result.started_parsing_at {
            let mut root_value = previous_parse_result.json
                [previous_parse_result.started_parsing_at_index_start]
                .clone();
            (
                root_value,
                previous_parse_result.started_parsing_at_index_start,
                previous_parse_result.started_parsing_at_index_end,
            )
        } else {
            (previous_parse_result.json[0].clone(), 0, 0)
        };

    if !matches!(root_value.pointer.value_type, ValueType::Array(_)) {
        return Err("Parsed json root is not an array".to_string());
    }
    let root_array_len = match root_value.pointer.value_type {
        ValueType::Array(root_array_len) => root_array_len,
        _ => panic!(""),
    };
    if end_index == 0 {
        end_index = previous_parse_result.json.len() - 1;
    }
    let mut unique_keys: Vec<Column> = Vec::with_capacity(16);
    let mut res: Vec<JsonArrayEntries<String>> = Vec::with_capacity(root_array_len);
    let mut j = end_index;
    let estimated_capacity = 16;
    for i in (0..root_array_len).rev() {
        let mut flat_json_values: Vec<FlatJsonValue<String>> =
            Vec::with_capacity(estimated_capacity);
        let mut is_first_entry = true;
        let _i = i.to_string();
        loop {
            if !previous_parse_result.json.is_empty() {
                let entry = &mut previous_parse_result.json[j];
                let (match_prefix, prefix_len) = if let Some(ref started_parsing_at) =
                    previous_parse_result.started_parsing_at
                {
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
                        let mut column = Column {
                            name: Cow::from(key.to_string()),
                            depth: entry.pointer.depth,
                            value_type: entry.pointer.value_type,
                            seen_count: 1,
                            order: unique_keys.len(),
                            id: 0,
                        };
                        if let Some(existing_column) =
                            unique_keys.iter_mut().find(|c| c.eq(&&column))
                        {
                            existing_column.seen_count += 1;
                            if existing_column.value_type.eq(&ValueType::Null) {
                                existing_column.value_type = column.value_type;
                            }
                            entry.pointer.column_id = existing_column.id;
                        } else {
                            let mut hasher = DefaultHasher::new();
                            hasher.write(column.name.as_bytes());
                            column.id = hasher.finish() as usize;
                            entry.pointer.column_id = column.id;
                            unique_keys.push(column);
                        }
                    }
                    if is_first_entry {
                        is_first_entry = false;
                        let prefix = &entry.pointer.pointer[0..prefix_len];
                        flat_json_values.push(row_number_entry(i, entry.pointer.position, prefix));
                    }
                    let entry = previous_parse_result.json.pop().unwrap();
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
        if !flat_json_values.is_empty() {
            res.push(JsonArrayEntries::<String> {
                entries: flat_json_values,
                index: i,
            });
        }
    }
    res.reverse();
    unique_keys.sort();
    Ok((res, unique_keys))
}

pub fn row_number_entry(i: usize, position: usize, prefix: &str) -> FlatJsonValue<String> {
    FlatJsonValue {
        pointer: PointerKey::from_pointer(
            concat_string!(prefix, "/#"),
            ValueType::Number,
            0,
            position,
        ),
        value: Some(i.to_string()),
    }
}

#[cfg(windows)]
const LINE_ENDING: &'static [u8] = ",\r\n".as_bytes();
#[cfg(not(windows))]
const LINE_ENDING: &[u8] = ",\n".as_bytes();

pub fn save_to_buffer<T: Write>(
    parent_pointer: &str,
    array: &[JsonArrayEntries<String>],
    buffer: &mut T,
) -> std::io::Result<()> {
    if !parent_pointer.is_empty() {
        let split = parent_pointer.split('/');
        for frag in split {
            if frag.is_empty() {
                continue;
            }
            let b = &frag.as_bytes()[0];
            if *b >= 0x30 && *b <= 0x39 {
                buffer.write_all("[".as_bytes()).unwrap();
            } else {
                buffer
                    .write_all(format!("{{\"{}\":", frag).as_bytes())
                    .unwrap();
            }
        }
    }

    buffer.write_all("[".as_bytes()).unwrap();
    for (i, entry) in array.iter().enumerate() {
        if let Some(serialized_entry) = entry.entries.last() {
            buffer
                .write_all(serialized_entry.value.as_ref().unwrap().as_bytes())
                .unwrap();
            if i < array.len() - 1 {
                buffer.write_all(LINE_ENDING).unwrap();
            }
        }
    }
    buffer.write_all("]".as_bytes())?;
    if !parent_pointer.is_empty() {
        let split = parent_pointer.split('/');
        for frag in split {
            if frag.is_empty() {
                continue;
            }
            let b = &frag.as_bytes()[0];
            if *b >= 0x30 && *b <= 0x39 {

                buffer.write_all("]".as_bytes()).unwrap();
            } else {
                buffer.write_all("}".as_bytes()).unwrap();
            }
        }
    }
    buffer.flush()?;
    Ok(())
}

pub fn save_to_file(
    parent_pointer: &str,
    array: &[JsonArrayEntries<String>],
    file_path: &Path,
) -> std::io::Result<()> {
    // let start = crate::compatibility::now();
    let file = fs::File::create(file_path)?;
    let mut file = BufWriter::new(file);
    save_to_buffer(parent_pointer, array, &mut file)?;
    // println!("serialize and save file took {}ms", start.elapsed().as_millis());
    Ok(())
}

pub fn filter_columns(
    previous_parse_result: &Vec<JsonArrayEntries<String>>,
    prefix: &str,
    filters: &HashMap<String, Vec<String>>,
) -> Vec<usize> {
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
                if !filters_clone.is_empty()
                    && (entry.value.as_ref().is_none()
                        || !filters_clone.contains(entry.value.as_ref().unwrap()))
                {
                    should_add_row = false;
                    break;
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
pub fn search_occurrences(
    previous_parse_result: &[JsonArrayEntries<String>],
    term: &str,
) -> Vec<usize> {
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

pub fn replace_occurrences(
    previous_parse_result: &[JsonArrayEntries<String>],
    search_replace_response: SearchReplaceResponse,
) -> Vec<(FlatJsonValue<String>, usize)> {
    let column_ids = if let Some(ref selected_columns) = search_replace_response.selected_column {
        selected_columns
            .iter()
            .map(|c| c.id)
            .collect::<Vec<usize>>()
    } else {
        vec![]
    };
    let mut new_values: Vec<(FlatJsonValue<String>, usize)> = vec![];
    for json_array_entry in previous_parse_result.iter() {
        for entry in json_array_entry.entries.iter() {
            if column_ids.contains(&entry.pointer.column_id) {
                if let Some(ref value) = entry.value {
                    match search_replace_response.replace_mode {
                        ReplaceMode::MatchingCase => {
                            let new_value = if let Some(ref replace_value) =
                                search_replace_response.replace_value
                            {
                                Some(value.replace(
                                    search_replace_response.search_criteria.as_str(),
                                    replace_value,
                                ))
                            } else {
                                if (search_replace_response.search_criteria.as_str().is_empty()
                                    && value.is_empty())
                                    || (!search_replace_response
                                        .search_criteria
                                        .as_str()
                                        .is_empty()
                                        && value.contains(
                                            search_replace_response.search_criteria.as_str(),
                                        ))
                                {
                                    None
                                } else {
                                    Some(value.clone())
                                }
                            };
                            new_values.push((
                                FlatJsonValue {
                                    pointer: entry.pointer.clone(),
                                    value: new_value,
                                },
                                json_array_entry.index,
                            ));
                        }
                        ReplaceMode::Regex => {
                            let re = Regex::new(search_replace_response.search_criteria.as_str())
                                .unwrap();
                            let new_value = replace_with_regex(&search_replace_response, value, re);
                            new_values.push((
                                FlatJsonValue {
                                    pointer: entry.pointer.clone(),
                                    value: new_value,
                                },
                                json_array_entry.index,
                            ));
                        }
                        ReplaceMode::ExactWord => {
                            let re = Regex::new(&format!(
                                r"\b{}\b",
                                regex_lite::escape(
                                    search_replace_response.search_criteria.as_str()
                                )
                            ))
                            .unwrap();
                            let new_value = replace_with_regex(&search_replace_response, value, re);
                            new_values.push((
                                FlatJsonValue {
                                    pointer: entry.pointer.clone(),
                                    value: new_value,
                                },
                                json_array_entry.index,
                            ));
                        }
                        ReplaceMode::Simple => {
                            let re = Regex::new(&format!(
                                "(?i){}",
                                regex_lite::escape(
                                    search_replace_response.search_criteria.as_str()
                                )
                            ))
                            .unwrap();
                            let new_value = replace_with_regex(&search_replace_response, value, re);
                            new_values.push((
                                FlatJsonValue {
                                    pointer: entry.pointer.clone(),
                                    value: new_value,
                                },
                                json_array_entry.index,
                            ));
                        }
                    }
                }
            }
        }
    }
    new_values
}

fn replace_with_regex(
    search_replace_response: &SearchReplaceResponse,
    value: &str,
    re: Regex,
) -> Option<String> {
    let new_value = if let Some(ref replace_value) = search_replace_response.replace_value {
        Some(re.replace_all(value, replace_value.as_str()).to_string())
    } else if (search_replace_response.search_criteria.is_empty() && value.is_empty())
        || (!search_replace_response.search_criteria.is_empty() && re.is_match(value))
    {
        None
    } else {
        if (search_replace_response.search_criteria.as_str().is_empty() && value.is_empty())
            || (!search_replace_response.search_criteria.as_str().is_empty() && re.is_match(value))
        {
            None
        } else {
            Some(value.clone())
        }
    };
    new_value
}

#[cfg(test)]
mod tests {
    use crate::array_table::Column;
    use crate::panels::{ReplaceMode, SearchReplaceResponse};
    use crate::parser::{as_array, replace_occurrences};
    use json_flat_parser::{JSONParser, ParseOptions};

    #[test]
    fn test_replace() {
        let json = r#"
        {"skills": [
        {
          "description": "Cart Termination",
          "duration2": 5000,
          "element": "Weapon",
          "damageType": "Single",
          "hitCount": 1,
          "id": 485,
          "maxLevel": 10,
          "name": "WS_CARTTERMINATION",
          "range": -2,
          "targetType": "Target",
          "type": "Offensive",
          "damageflags": {
            "ignoreAtkCard": true
          },
          "flags": {
            "ignoreAutoGuard": true,
            "ignoreCicada": true
          }
        }
    ]}"#;

        let res = JSONParser::parse(
            json,
            ParseOptions::default()
                .start_parse_at("/skills".to_string())
                .parse_array(false),
        )
        .unwrap()
        .to_owned();
        let (array, columns) = as_array(res).unwrap();
        let filter_column = columns
            .iter()
            .filter(|c| c.name.eq("/description"))
            .cloned()
            .collect::<Vec<Column>>();
        let replaced_values = replace_occurrences(
            &array,
            SearchReplaceResponse {
                search_criteria: "(.*)".to_string(),
                replace_value: Some("A$1".to_string()),
                selected_column: Some(filter_column),
                replace_mode: ReplaceMode::Regex,
            },
        );
        assert_eq!(
            replaced_values[0].0.value.as_ref().unwrap().as_str(),
            "ACart Termination"
        );
    }
}
