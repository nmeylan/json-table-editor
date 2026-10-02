use std::borrow::Cow;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hasher};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::{fs, mem};

use crate::array_table::Column;
use crate::array_table::table_source::{CellKind, CompactCell, CompactRows, TableSource};
use crate::panels::{ReplaceMode, SearchReplaceResponse};
use json_flat_parser::{
    FlatJsonValue, GetBytes, JSONParser, JsonArrayEntries, ParseOptions, ParseResult, PointerKey,
    ValueType,
};
use std::fmt::Debug;
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use rayon::prelude::{IndexedParallelIterator, IntoParallelRefMutIterator, ParallelSlice, ParallelSliceMut};
use regex_lite::Regex;

#[macro_export]
macro_rules! concat_string {
    () => { String::with_capacity(0) };
    ($($s:expr_2021),+) => {{
        use std::ops::AddAssign;
        let mut len = 0;
        $(len.add_assign(AsRef::<str>::as_ref(&$s).len());)+
        let mut buf = String::with_capacity(len);
        $(buf.push_str($s.as_ref());)+
        buf
    }};
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FileFormat {
    Json,
    Jsonl,
}

/// Id stored in `PointerKey.column_id` for every entry of the column named `name`.
pub fn column_id(name: &str) -> usize {
    let mut hasher = DefaultHasher::new();
    hasher.write(name.as_bytes());
    hasher.finish() as usize
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
                            column.id = column_id(&column.name);
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
            let root_value = previous_parse_result.json
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
                            column.id = column_id(&column.name);
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

/// Same result as `as_array(JSONParser::parse_jsonl(json, options))`, with lines parsed in parallel.
/// Also returns the max json depth.
pub fn jsonl_as_array<'array>(
    json: &[u8],
    options: &ParseOptions,
) -> Result<(Vec<JsonArrayEntries<String>>, Vec<Column<'array>>, usize), String> {
    let lines: Vec<(usize, &[u8])> = JSONParser::jsonl_lines(json).collect();
    rows_as_array(
        &lines,
        "",
        |row_index, (line_number, line)| {
            JSONParser::parse_jsonl_line::<String>(line, *line_number, row_index, options)
                .map(|(entries, max_depth)| (row_index, entries, max_depth))
        },
        array_entries,
    )
}

/// Same result as `as_array(JSONParser::parse_bytes_owned(json, options).unwrap())`, with rows parsed in parallel.
/// Also returns the parse result without json.
pub fn json_array_as_array<'array>(
    json: &[u8],
    options: &ParseOptions,
) -> Result<(Vec<JsonArrayEntries<String>>, Vec<Column<'array>>, ParseResult<String>), String> {
    let json_rows = JsonRows::new(json, options)?;
    let (mut rows, columns, max_depth) = rows_as_array(
        json_rows.rows(),
        &json_rows.prefix,
        |_, row| json_rows.parse_row::<String>(row, options),
        array_entries,
    )?;

    // Positions as the sequential parse: shifted by the content of previous rows, not counted by the rows pass
    let mut offsets = Vec::with_capacity(rows.len());
    let mut offset = 0;
    for row in rows.iter() {
        offsets.push(offset);
        // Without row number and row entries
        offset += row.entries.len() - 2;
    }
    rows.par_iter_mut().zip(offsets).for_each(|(row, offset)| {
        for entry in row.entries.iter_mut() {
            entry.pointer.position += offset;
        }
    });

    Ok((rows, columns, json_rows.parse_result(options, max_depth)))
}

/// Same rows and columns as `json_array_as_array`, kept as positions in json. Objects keep their raw data.
pub fn json_array_as_compact<'array>(
    json: String,
    options: &ParseOptions,
) -> Result<(CompactRows, Vec<Column<'array>>, ParseResult<String>), String> {
    let (rows, columns, parse_result) = {
        let json_rows = JsonRows::new(json.as_bytes(), options)?;
        let (rows, columns, max_depth) = rows_as_array(
            json_rows.rows(),
            &json_rows.prefix,
            |_, row| json_rows.parse_row::<&str>(row, options),
            |_, _, entries| compact_row(json.as_bytes(), entries),
        )?;
        (rows, columns, json_rows.parse_result(options, max_depth))
    };
    let column_ids = columns.iter().map(|column| column.id).collect();
    Ok((CompactRows::new(json, rows, column_ids), columns, parse_result))
}

/// Same rows and columns as `jsonl_as_array`, kept as positions in json. Objects keep their raw data.
pub fn jsonl_as_compact<'array>(
    json: String,
    options: &ParseOptions,
) -> Result<(CompactRows, Vec<Column<'array>>, usize), String> {
    let (rows, columns, max_depth) = {
        let lines: Vec<(usize, &[u8])> = JSONParser::jsonl_lines(json.as_bytes()).collect();
        rows_as_array(
            &lines,
            "",
            |row_index, (line_number, line)| {
                JSONParser::parse_jsonl_line::<&str>(line, *line_number, row_index, options)
                    .map(|(entries, max_depth)| (row_index, entries, max_depth))
            },
            |_, _, entries| compact_row(json.as_bytes(), entries),
        )?
    };
    let column_ids = columns.iter().map(|column| column.id).collect();
    Ok((CompactRows::new(json, rows, column_ids), columns, max_depth))
}

/// Row start in json and its cells with their column id
fn compact_row(json: &[u8], entries: Vec<FlatJsonValue<&str>>) -> (usize, Vec<(usize, CompactCell)>) {
    let position = |value: &str| value.as_ptr() as usize - json.as_ptr() as usize;
    let row_start = entries[0].value.map_or(0, position);
    let cells = entries
        .iter()
        .map(|entry| {
            let cell = CompactCell {
                column: 0,
                start: entry.value.map_or(0, |value| (position(value) - row_start) as u32),
                len: entry.value.map_or(0, |value| value.len() as u32),
                kind: CellKind::of(entry.pointer.value_type, entry.value.is_some()),
            };
            (entry.pointer.column_id, cell)
        })
        .collect();
    (row_start, cells)
}

/// Rows of a json array, unparsed with their raw data, from a fast sequential pass
struct JsonRows<'json> {
    rows_result: ParseResult<&'json str>,
    start_index: usize,
    end_index: usize,
    prefix: String,
    keep_row_raw_data: bool,
}

impl<'json> JsonRows<'json> {
    fn new(json: &'json [u8], options: &ParseOptions) -> Result<Self, String> {
        let rows_result = JSONParser::parse_bytes(json, options.clone().max_depth(1))?;
        let (start_index, mut end_index) = if rows_result.started_parsing_at.is_some() {
            (
                rows_result.started_parsing_at_index_start,
                rows_result.started_parsing_at_index_end,
            )
        } else {
            (0, 0)
        };
        if !rows_result.json.get(start_index).is_some_and(|root| matches!(root.pointer.value_type, ValueType::Array(_))) {
            return Err("Parsed json root is not an array".to_string());
        }
        if end_index == 0 {
            end_index = rows_result.json.len() - 1;
        }
        let prefix = rows_result
            .started_parsing_at
            .clone()
            .or(rows_result.parsing_prefix.clone())
            .unwrap_or_default();
        let keep_row_raw_data = (options.keep_object_raw_data && options.keep_object_raw_data_max_depth >= 1)
            || options.max_depth == 1;
        Ok(Self {
            rows_result,
            start_index,
            end_index,
            prefix,
            keep_row_raw_data,
        })
    }

    fn rows(&self) -> &[FlatJsonValue<&'json str>] {
        &self.rows_result.json[self.start_index + 1..=self.end_index]
    }

    /// Parse row content as the sequential parse would have done it, positions are local to the row
    fn parse_row<V: Debug + Clone + AsRef<str> + GetBytes + From<&'json str>>(
        &self,
        row: &FlatJsonValue<&'json str>,
        options: &ParseOptions,
    ) -> Result<(usize, Vec<FlatJsonValue<V>>, usize), String> {
        let index = row.pointer.pointer[self.prefix.len() + 1..]
            .parse::<usize>()
            .map_err(|e| format!("Unexpected row pointer {}: {}", row.pointer.pointer, e))?;
        if let (true, ValueType::Object(false, _), Some(raw_data)) =
            (options.max_depth > 1, row.pointer.value_type, row.value)
        {
            let (content, elements_count) = JSONParser::parse_object_raw_data::<V>(
                raw_data,
                &row.pointer,
                self.rows_result.depth_after_start_at,
                options,
            )?;
            let mut entries = Vec::with_capacity(content.json.len() + 1);
            entries.push(FlatJsonValue {
                pointer: PointerKey {
                    value_type: ValueType::Object(true, elements_count),
                    ..row.pointer.clone()
                },
                value: self.keep_row_raw_data.then(|| V::from(raw_data)),
            });
            // Content positions start at 1 after the row position
            entries.extend(content.json.into_iter().map(|mut entry| {
                entry.pointer.position += row.pointer.position;
                entry
            }));
            Ok((index, entries, content.max_json_depth))
        } else {
            let entry = FlatJsonValue {
                pointer: row.pointer.clone(),
                value: row.value.map(V::from),
            };
            Ok((index, vec![entry], 0))
        }
    }

    fn parse_result(&self, options: &ParseOptions, rows_max_depth: usize) -> ParseResult<String> {
        let mut parse_result = self.rows_result.clone_except_json().to_owned();
        parse_result.parsing_max_depth = options.max_depth;
        parse_result.max_json_depth = rows_max_depth.max(self.rows_result.max_json_depth);
        parse_result
    }
}

/// Rows parsed in parallel by chunks, then built with columns, same result as as_array when built as array entries.
/// `parse_row` gives for a source, at its index in `sources`, the row index, its entries (row entry first then its content,
/// in parse order) and its max json depth.
/// `build_row` gets the row index, its pointer and its entries, with their column id.
fn rows_as_array<'array, S: Sync, V: Debug + Clone + AsRef<str> + GetBytes + Send, R: Send>(
    sources: &[S],
    prefix: &str,
    parse_row: impl Fn(usize, &S) -> Result<(usize, Vec<FlatJsonValue<V>>, usize), String> + Sync,
    build_row: impl Fn(usize, &str, Vec<FlatJsonValue<V>>) -> R + Sync,
) -> Result<(Vec<R>, Vec<Column<'array>>, usize), String> {
    let chunk_size = (sources.len() / (rayon::current_num_threads() * 4)).max(1);
    let chunks = sources
        .par_chunks(chunk_size)
        .enumerate()
        .map(|(chunk_index, chunk)| {
            let mut rows = Vec::with_capacity(chunk.len());
            let mut max_depth = 0;
            for (i, source) in chunk.iter().enumerate() {
                let (index, entries, row_max_depth) = parse_row(chunk_index * chunk_size + i, source)?;
                max_depth = max_depth.max(row_max_depth);
                rows.push((index, entries));
            }
            // Columns are collected in the order as_array sees them: rows and their entries from last to first
            let mut columns: Vec<Column> = Vec::with_capacity(16);
            let mut column_index_by_name: HashMap<String, usize> = HashMap::with_capacity(16);
            let mut res: Vec<R> = Vec::with_capacity(rows.len());
            for (index, mut entries) in rows.into_iter().rev() {
                let row_prefix = concat_string!(prefix, "/", index.to_string());
                for entry in entries.iter_mut().rev() {
                    let key = &entry.pointer.pointer[row_prefix.len()..];
                    if let Some(column_index) = column_index_by_name.get(key) {
                        let column = &mut columns[*column_index];
                        column.seen_count += 1;
                        if column.value_type.eq(&ValueType::Null) {
                            column.value_type = entry.pointer.value_type;
                        }
                        entry.pointer.column_id = column.id;
                    } else {
                        let id = column_id(key);
                        entry.pointer.column_id = id;
                        column_index_by_name.insert(key.to_string(), columns.len());
                        columns.push(Column {
                            name: Cow::from(key.to_string()),
                            depth: entry.pointer.depth,
                            value_type: entry.pointer.value_type,
                            seen_count: 1,
                            order: columns.len(),
                            id,
                        });
                    }
                }
                res.push(build_row(index, &row_prefix, entries));
            }
            res.reverse();
            Ok((res, columns, max_depth))
        })
        .collect::<Result<Vec<_>, String>>()?;

    let mut rows: Vec<R> = Vec::with_capacity(sources.len());
    let mut unique_keys: Vec<Column> = Vec::with_capacity(16);
    let mut max_depth = 0;
    let mut chunks_columns = Vec::with_capacity(chunks.len());
    for (chunk_rows, chunk_columns, chunk_max_depth) in chunks {
        rows.extend(chunk_rows);
        chunks_columns.push(chunk_columns);
        max_depth = max_depth.max(chunk_max_depth);
    }
    // Last chunk first, to keep as_array columns order
    for chunk_columns in chunks_columns.into_iter().rev() {
        for chunk_column in chunk_columns {
            if let Some(column) = unique_keys.iter_mut().find(|c| c.eq(&&chunk_column)) {
                column.seen_count += chunk_column.seen_count;
                if column.value_type.eq(&ValueType::Null) {
                    column.value_type = chunk_column.value_type;
                }
            } else {
                let order = unique_keys.len();
                unique_keys.push(Column {
                    order,
                    ..chunk_column
                });
            }
        }
    }
    unique_keys.sort();
    Ok((rows, unique_keys, max_depth))
}

/// Row as array entries, the same as as_array
fn array_entries(index: usize, row_prefix: &str, entries: Vec<FlatJsonValue<String>>) -> JsonArrayEntries<String> {
    let position = entries.last().unwrap().pointer.position;
    let mut row_entries = Vec::with_capacity(entries.len() + 1);
    row_entries.push(row_number_entry(index, position, row_prefix));
    row_entries.extend(entries.into_iter().rev());
    JsonArrayEntries::<String> {
        entries: row_entries,
        index,
    }
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
    format: FileFormat,
    buffer: &mut T,
) -> std::io::Result<()> {
    if format == FileFormat::Jsonl {
        for entry in array.iter() {
            if let Some(serialized_entry) = entry.entries.last() {
                // Edited rows are serialized on multiple lines: as a json string can't contain a raw new line,
                // new lines and the indentation following them are only formatting.
                for line in serialized_entry.value.as_ref().unwrap().split('\n') {
                    buffer.write_all(line.trim_start().as_bytes())?;
                }
                buffer.write_all(b"\n")?;
            }
        }
        buffer.flush()?;
        return Ok(());
    }
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
    format: FileFormat,
    file_path: &Path,
) -> std::io::Result<()> {
    // let start = crate::compatibility::now();
    let file = fs::File::create(file_path)?;
    let mut file = BufWriter::new(file);
    save_to_buffer(parent_pointer, array, format, &mut file)?;
    // println!("serialize and save file took {}ms", start.elapsed().as_millis());
    Ok(())
}

pub fn search_occurrences(source: &dyn TableSource, term: &str) -> Vec<usize> {
    (0..source.rows_count())
        .filter(|data_index| {
            source.any_string(*data_index, &mut |value| value.to_lowercase().contains(term))
        })
        .map(|data_index| source.row_index(data_index))
        .collect()
}

pub fn replace_occurrences(
    previous_parse_result: &Vec<JsonArrayEntries<String>>,
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
                            } else if (search_replace_response.search_criteria.is_empty()
                                && value.is_empty())
                                || (!search_replace_response
                                    .search_criteria
                                    .is_empty()
                                    && value.contains(
                                        search_replace_response.search_criteria.as_str(),
                                    ))
                            {
                                None
                            } else {
                                Some(value.clone())
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
    value: &String,
    re: Regex,
) -> Option<String> {
    let new_value = if let Some(ref replace_value) = search_replace_response.replace_value {
        Some(re.replace_all(value, replace_value.as_str()).to_string())
    } else if (search_replace_response.search_criteria.is_empty() && value.is_empty())
        || (!search_replace_response.search_criteria.is_empty() && re.is_match(value))
    {
        None
    } else {
        Some(value.clone())
    };
    new_value
}

#[cfg(test)]
mod tests {
    use crate::array_table::Column;
    use crate::panels::{ReplaceMode, SearchReplaceResponse};
    use crate::parser::{
        FileFormat, as_array, json_array_as_array, json_array_as_compact, jsonl_as_array,
        jsonl_as_compact, replace_occurrences, save_to_buffer,
    };
    use crate::array_table::table_source::{CellKind, TableSource};
    use json_flat_parser::JsonArrayEntries;
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

    #[test]
    fn jsonl_as_array_same_as_as_array() {
        // Enough rows to be split in several chunks, columns with different schemas, null first, nested
        let mut jsonl = String::new();
        for i in 0..500 {
            match i % 4 {
                0 => jsonl.push_str(&format!("{{\"id\": {i}, \"a\": null, \"n\": {{\"x\": {i}, \"y\": {{\"z\": [1, 2]}}}}}}\n")),
                1 => jsonl.push_str(&format!("  {{\"id\": {i}, \"a\": \"v{i}\", \"b\": true}}  \r\n\n")),
                2 => jsonl.push_str(&format!("{{\"b\": false, \"n\": {{\"x\": null}}, \"c{}\": 1}}\n", i % 7)),
                _ => jsonl.push_str(&format!("{{\"id\": {i}, \"a\": 1.5}}\n")),
            }
        }
        for max_depth in [1, 2, 3, u8::MAX] {
            let options = ParseOptions::default().parse_array(false).max_depth(max_depth);
            let (expected_rows, expected_columns) =
                as_array(JSONParser::parse_jsonl(jsonl.as_bytes(), options.clone()).unwrap()).unwrap();
            let (rows, columns, max_json_depth) = jsonl_as_array(jsonl.as_bytes(), &options).unwrap();

            assert_same_array(&rows, &columns, &expected_rows, &expected_columns);
            assert_eq!(
                max_json_depth,
                JSONParser::parse_jsonl(jsonl.as_bytes(), options).unwrap().max_json_depth
            );
        }
    }

    #[test]
    fn save_jsonl() {
        let jsonl = "{\"id\": 1, \"a\": \"x\\ny\"}\n\n{\"id\": 2, \"n\": {\"x\": 1}}\n";
        let options = ParseOptions::default().parse_array(false).max_depth(u8::MAX);
        let (mut rows, _, _) = jsonl_as_array(jsonl.as_bytes(), &options).unwrap();
        // An edited row is serialized on multiple lines
        rows[1].entries.last_mut().unwrap().value =
            Some("{\n  \"id\": 3,\n  \"n\": {\n    \"x\": \"a  b\"\n  }\n}".to_string());
        let mut buffer = vec![];
        save_to_buffer("", &rows, FileFormat::Jsonl, &mut buffer).unwrap();
        assert_eq!(
            String::from_utf8(buffer).unwrap(),
            "{\"id\": 1, \"a\": \"x\\ny\"}\n{\"id\": 3,\"n\": {\"x\": \"a  b\"}}\n"
        );
    }

    fn assert_same_array(
        rows: &[JsonArrayEntries<String>],
        columns: &[Column],
        expected_rows: &[JsonArrayEntries<String>],
        expected_columns: &[Column],
    ) {
        assert_eq!(rows.len(), expected_rows.len());
        for (row, expected_row) in rows.iter().zip(expected_rows.iter()) {
            assert_eq!(row.index, expected_row.index);
            assert_eq!(row.entries.len(), expected_row.entries.len(), "row {}", row.index);
            for (entry, expected_entry) in row.entries.iter().zip(expected_row.entries.iter()) {
                assert_eq!(entry.pointer.pointer, expected_entry.pointer.pointer);
                assert_eq!(entry.pointer.value_type, expected_entry.pointer.value_type, "{}", entry.pointer.pointer);
                assert_eq!(entry.pointer.depth, expected_entry.pointer.depth, "{}", entry.pointer.pointer);
                assert_eq!(entry.pointer.position, expected_entry.pointer.position, "{}", entry.pointer.pointer);
                assert_eq!(entry.pointer.column_id, expected_entry.pointer.column_id, "{}", entry.pointer.pointer);
                assert_eq!(entry.value, expected_entry.value, "{}", entry.pointer.pointer);
            }
        }
        let describe = |columns: &[Column]| {
            columns
                .iter()
                .map(|c| (c.name.to_string(), c.depth, c.value_type, c.seen_count, c.order, c.id))
                .collect::<Vec<_>>()
        };
        assert_eq!(describe(columns), describe(expected_columns));
    }

    #[test]
    fn json_array_as_array_same_as_as_array() {
        let mut rows = vec![];
        for i in 0..500 {
            rows.push(match i % 7 {
                0 => format!("{{\"id\": {i}, \"a\": null, \"n\": {{\"x\": {i}, \"y\": {{\"z\": [1, {{\"w\": 2}}]}}}}}}"),
                1 => format!("{{\"id\": {i}, \"a\": \"v{i} }}]\", \"b\": true}}"),
                2 => format!("{{\"b\": false, \"n\": {{\"x\": null}}, \"c{}\": 1}}", i % 5),
                3 => "{}".to_string(),
                4 => format!("{i}"),
                5 => "null".to_string(),
                _ => format!("[{i}, {{\"a\": 1}}]"),
            });
        }
        let array = format!("[\n  {}\n]", rows.join(",\n  "));
        let root_json = array.clone();
        let nested_json = format!("{{\"before\": {{\"deep\": {{\"deeper\": {{\"x\": 1}}}}}}, \"a\": {{\"b\": {{\"items\": {array}}}}}, \"after\": {{\"x\": {{\"y\": 1}}}}}}");
        for (json, start_parse_at) in [(&root_json, None), (&nested_json, Some("/a/b/items"))] {
            for max_depth in [1, 2, 3, u8::MAX] {
                for (keep_object_raw_data, keep_object_raw_data_max_depth) in [(true, u8::MAX), (true, 1), (false, u8::MAX)] {
                    let mut options = ParseOptions::default()
                        .parse_array(false)
                        .max_depth(max_depth)
                        .keep_object_raw_data(keep_object_raw_data)
                        .keep_object_raw_data_max_depth(keep_object_raw_data_max_depth);
                    if let Some(start_parse_at) = start_parse_at {
                        options = options.start_parse_at(start_parse_at.to_string());
                    }
                    let expected = JSONParser::parse_bytes_owned(json.as_bytes(), options.clone()).unwrap();
                    let expected_meta = expected.clone_except_json();
                    let (expected_rows, expected_columns) = as_array(expected).unwrap();
                    let (rows, columns, meta) = json_array_as_array(json.as_bytes(), &options).unwrap();

                    assert_eq!(rows.len(), 500);
                    assert_same_array(&rows, &columns, &expected_rows, &expected_columns);
                    assert_eq!(meta.parsing_max_depth, expected_meta.parsing_max_depth);
                    assert_eq!(meta.started_parsing_at, expected_meta.started_parsing_at);
                    assert_eq!(meta.parsing_prefix, expected_meta.parsing_prefix);
                    assert_eq!(meta.depth_after_start_at, expected_meta.depth_after_start_at);
                }
            }
        }
        assert!(json_array_as_array(b"{\"a\": 1}", &ParseOptions::default().parse_array(false)).is_err());
    }

    fn assert_same_source(source: &dyn TableSource, expected: &dyn TableSource, columns: &[Column]) {
        assert_eq!(source.rows_count(), expected.rows_count());
        for data_index in 0..source.rows_count() {
            assert_eq!(source.row_index(data_index), expected.row_index(data_index));
            for column in columns {
                let cell = source.cell(data_index, column.id);
                let expected_cell = expected.cell(data_index, column.id);
                assert_eq!(cell.is_some(), expected_cell.is_some(), "row {} {}", data_index, column.name);
                if let (Some(cell), Some(expected_cell)) = (cell, expected_cell) {
                    assert_eq!(
                        CellKind::of(cell.value_type, cell.value.is_some()),
                        CellKind::of(expected_cell.value_type, expected_cell.value.is_some()),
                        "row {} {}", data_index, column.name
                    );
                    assert_eq!(cell.value, expected_cell.value, "row {} {}", data_index, column.name);
                }
            }
        }
    }

    #[test]
    fn compact_same_as_array() {
        let mut rows = vec![];
        let mut lines = String::new();
        for i in 0..300 {
            let row = match i % 6 {
                0 => format!("{{\"id\": {i}, \"a\": null, \"s\": \"é \\\"q\\\"\", \"n\": {{\"x\": {i}, \"y\": {{\"z\": [1, {{\"w\": 2}}], \"e\": []}}}}}}"),
                1 => format!("{{\"id\": {i}, \"a\": \"v{i} }}]\", \"b\": true}}"),
                2 => format!("{{\"b\": false, \"n\": {{\"x\": null}}, \"c{}\": 1}}", i % 5),
                3 => "{}".to_string(),
                4 => format!("{{\"id\": {i}, \"arr\": [{i}, {{\"a\": 1}}]}}"),
                _ => format!("{{\"id\": {i}, \"o\": {{}}}}"),
            };
            lines.push_str(&row);
            lines.push('\n');
            rows.push(row);
            if i % 6 == 3 {
                rows.push(format!("{i}"));
                rows.push("null".to_string());
            }
        }
        let array = format!("[{}]", rows.join(",\n"));
        let nested_json = format!("{{\"before\": {{\"x\": 1}}, \"a\": {{\"items\": {array}}}}}");
        let options = ParseOptions::default().parse_array(false).max_depth(u8::MAX);
        for (json, start_parse_at) in [(&array, None), (&nested_json, Some("/a/items"))] {
            let mut options = options.clone();
            if let Some(start_parse_at) = start_parse_at {
                options = options.start_parse_at(start_parse_at.to_string());
            }
            let (expected_rows, expected_columns, expected_meta) = json_array_as_array(json.as_bytes(), &options).unwrap();
            let (rows, columns, meta) = json_array_as_compact(json.to_string(), &options).unwrap();
            assert_same_array(&[], &columns, &[], &expected_columns);
            assert_same_source(&rows, &expected_rows, &columns);
            assert_eq!(meta.max_json_depth, expected_meta.max_json_depth);
        }
        let (expected_rows, expected_columns, expected_max_depth) = jsonl_as_array(lines.as_bytes(), &options).unwrap();
        let (rows, columns, max_depth) = jsonl_as_compact(lines, &options).unwrap();
        assert_same_array(&[], &columns, &[], &expected_columns);
        assert_same_source(&rows, &expected_rows, &columns);
        assert_eq!(max_depth, expected_max_depth);
    }
}
