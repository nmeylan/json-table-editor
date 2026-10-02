use super::{ArrayTable, Column};
use crate::concat_string;
use egui::TextBuffer;
use json_flat_parser::{FlatJsonValue, JsonArrayEntries, PointerKey, ValueType};
use std::ops::Deref;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Entry of `row` belonging to the column with id `column_id` (see `parser::column_id`).
#[inline]
pub(super) fn find_cell(
    row: &JsonArrayEntries<String>,
    column_id: usize,
) -> Option<&FlatJsonValue<String>> {
    row.entries
        .iter()
        .find(|entry| entry.pointer.column_id == column_id)
}

/// Parsed object at `index` in `row`, kept without its raw data, written from its content.
/// Values are written as stored, so the text is the object raw data without its whitespaces.
fn object_text(row: &JsonArrayEntries<String>, index: usize) -> String {
    let object = &row.entries[index];
    let prefix = concat_string!(object.pointer.pointer, "/");
    let mut content = row
        .entries
        .iter()
        .filter(|entry| entry.pointer.pointer.starts_with(&prefix))
        .collect::<Vec<&FlatJsonValue<String>>>();
    content.sort_by_key(|entry| entry.pointer.position);
    let mut text = String::new();
    write_object(&content, object, &mut text);
    text
}

fn write_object(content: &[&FlatJsonValue<String>], object: &FlatJsonValue<String>, text: &mut String) {
    let prefix = concat_string!(object.pointer.pointer, "/");
    text.push('{');
    let children = content.iter().filter(|entry| {
        entry.pointer.depth == object.pointer.depth + 1 && entry.pointer.pointer.starts_with(&prefix)
    });
    for (i, child) in children.enumerate() {
        if i > 0 {
            text.push(',');
        }
        text.push('"');
        text.push_str(&child.pointer.pointer[prefix.len()..]);
        text.push_str("\":");
        match (child.pointer.value_type, child.value.as_ref()) {
            (ValueType::String, Some(value)) => {
                text.push('"');
                text.push_str(value);
                text.push('"');
            }
            (_, Some(value)) => text.push_str(value),
            (ValueType::Object(true, _), None) => write_object(content, child, text),
            // Not kept as raw data
            (ValueType::Array(_), None) => text.push_str("[]"),
            (_, None) => text.push_str("null"),
        }
    }
    text.push('}');
}

/// Value of a cell, either the entry value or a value computed from the row
pub(super) enum CellValue<'a> {
    Borrowed(&'a str),
    Shared(Arc<str>),
}

impl Deref for CellValue<'_> {
    type Target = str;

    fn deref(&self) -> &str {
        match self {
            CellValue::Borrowed(value) => value,
            CellValue::Shared(value) => value,
        }
    }
}

#[derive(Default)]
pub(super) struct CacheObjectText {}

#[derive(Copy, Clone, Hash)]
pub(super) struct CacheObjectTextKey {
    row_index: usize,
    entry_index: usize,
}

impl crate::components::cache::ComputerMut<CacheObjectTextKey, &JsonArrayEntries<String>, Arc<str>>
    for CacheObjectText
{
    fn compute(&mut self, key: CacheObjectTextKey, row: &JsonArrayEntries<String>) -> Arc<str> {
        Arc::from(object_text(row, key.entry_index))
    }
}

#[derive(Default)]
pub(super) struct CacheGetPointer {}

#[derive(Copy, Clone)]
pub(super) struct CachePointerKey {
    pinned_column_table: bool,
    index: usize,
    row_index: usize,
}

impl Hash for CachePointerKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pinned_column_table.hash(state);
        self.index.hash(state);
        self.row_index.hash(state);
    }
}

impl<'array>
    crate::components::cache::ComputerMut<CachePointerKey, &ArrayTable<'array>, Option<usize>>
    for CacheGetPointer
{
    fn compute(
        &mut self,
        cache_pointer_key: CachePointerKey,
        table: &ArrayTable<'array>,
    ) -> Option<usize> {
        let columns = if cache_pointer_key.pinned_column_table {
            &table.column_pinned
        } else {
            &table.column_selected
        };
        ArrayTable::get_pointer_index(
            &table.parent_pointer,
            columns,
            &table.nodes()[cache_pointer_key.row_index].entries(),
            cache_pointer_key.index,
            cache_pointer_key.row_index,
        )
    }
}

impl<'array> ArrayTable<'array> {
    /// Value of entry at `index` in `row`. Parsed objects may be kept without raw data, their value is then computed from their content.
    pub(super) fn cell_value<'a>(
        &self,
        row: &'a JsonArrayEntries<String>,
        index: usize,
    ) -> Option<CellValue<'a>> {
        let entry = &row.entries[index];
        if let Some(value) = entry.value.as_ref() {
            return Some(CellValue::Borrowed(value.as_str()));
        }
        if !matches!(entry.pointer.value_type, ValueType::Object(true, _)) {
            return None;
        }
        let mut cache_ref_mut = self.cache.borrow_mut();
        let cache =
            cache_ref_mut.cache::<crate::components::cache::FrameCache<Arc<str>, CacheObjectText>>();
        let key = CacheObjectTextKey {
            row_index: row.index(),
            entry_index: index,
        };
        Some(CellValue::Shared(cache.get(key, row)))
    }

    pub(super) fn get_pointer_index_from_cache(
        &self,
        pinned_column_table: bool,
        row_data: &&JsonArrayEntries<String>,
        col_index: usize,
    ) -> Option<usize> {
        let index = {
            let mut cache_ref_mut = self.cache.borrow_mut();
            let cache = cache_ref_mut
                .cache::<crate::components::cache::FrameCache<Option<usize>, CacheGetPointer>>();
            let key = CachePointerKey {
                pinned_column_table,
                index: col_index,
                row_index: row_data.index(),
            };
            cache.get(key, self)
        };
        index
    }

    #[inline]
    pub(super) fn get_pointer_index(
        parent_pointer: &PointerKey,
        columns: &Vec<Column>,
        data: &&Vec<FlatJsonValue<String>>,
        index: usize,
        row_index: usize,
    ) -> Option<usize> {
        if let Some(column) = columns.get(index) {
            let key = column.name.as_str();
            let key = Self::pointer_key(&parent_pointer.pointer, row_index, key);
            return data.iter().position(|entry| entry.pointer.pointer.eq(&key));
        }
        None
    }
    #[inline]
    pub(super) fn get_pointer<'a>(
        &self,
        columns: &Vec<Column>,
        data: &&'a Vec<FlatJsonValue<String>>,
        index: usize,
        row_index: usize,
    ) -> Option<&'a FlatJsonValue<String>> {
        if let Some(column) = columns.get(index) {
            return Self::get_pointer_for_column(
                &self.parent_pointer.pointer,
                data,
                row_index,
                column,
            );
        }
        None
    }

    #[inline]
    pub(super) fn get_pointer_for_column<'a>(
        parent_pointer: &String,
        data: &&'a Vec<FlatJsonValue<String>>,
        row_index: usize,
        column: &Column,
    ) -> Option<&'a FlatJsonValue<String>> {
        let key = column.name.as_str();
        let key = Self::pointer_key(parent_pointer, row_index, key);
        data.iter().find(|entry| entry.pointer.pointer.eq(&key))
    }

    #[inline]
    pub(super) fn pointer_key(parent_pointer: &String, row_index: usize, key: &str) -> String {
        concat_string!(parent_pointer, "/", row_index.to_string(), key)
    }
}

#[cfg(test)]
mod tests {
    use super::object_text;
    use crate::parser::json_array_as_array;
    use json_flat_parser::{ParseOptions, ValueType};

    #[test]
    fn object_text_same_as_raw_data() {
        let json = r#"{"items": [
            {"id": 1, "n": {"x": 1.5, "s": "a \"q\" }", "e": {}, "y": {"z": [1, {"w": null}], "b": true}}},
            {"id": 2, "n": {"x": null, "y": {"z": []}}, "m": {"k": "v"}}
        ]}"#;
        for start_parse_at in [None, Some("/items")] {
            let json = if start_parse_at.is_some() { json.to_string() } else { json[10..json.len() - 1].to_string() };
            let mut options = ParseOptions::default().parse_array(false).max_depth(u8::MAX);
            if let Some(start_parse_at) = start_parse_at {
                options = options.start_parse_at(start_parse_at.to_string());
            }
            let (rows_with_raw_data, _, _) = json_array_as_array(json.as_bytes(), &options).unwrap();
            let (rows, _, _) = json_array_as_array(json.as_bytes(), &options.clone().keep_object_raw_data_max_depth(1)).unwrap();
            let mut checked = 0;
            for (row, row_with_raw_data) in rows.iter().zip(rows_with_raw_data.iter()) {
                for (index, entry) in row.entries.iter().enumerate() {
                    if matches!(entry.pointer.value_type, ValueType::Object(true, _)) && entry.value.is_none() {
                        let raw_data = row_with_raw_data.entries[index].value.as_ref().unwrap();
                        let expected: serde_json::Value = serde_json::from_str(raw_data).unwrap();
                        let actual: serde_json::Value = serde_json::from_str(&object_text(row, index)).unwrap();
                        assert_eq!(actual, expected, "{}", entry.pointer.pointer);
                        checked += 1;
                    }
                }
            }
            assert_eq!(checked, 6);
        }
    }
}
