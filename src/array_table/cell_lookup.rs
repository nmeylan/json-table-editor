use super::{ArrayTable, Column};
use crate::concat_string;
use egui::TextBuffer;
use json_flat_parser::{FlatJsonValue, JsonArrayEntries, PointerKey};
use std::hash::{Hash, Hasher};

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
