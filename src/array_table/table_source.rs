use super::cell_lookup::find_cell;
use json_flat_parser::{JsonArrayEntries, ValueType};
use nohash_hasher::IntMap;

/// Rows of a table, read only.
pub trait TableSource: Sync {
    fn rows_count(&self) -> usize;

    /// Index of the row in the json array, at `data_index` in the source
    fn row_index(&self, data_index: usize) -> usize;

    /// Cell of the column with id `column_id` (see `parser::column_id`), `None` when the row has no such cell
    fn cell(&self, data_index: usize, column_id: usize) -> Option<Cell<'_>>;

    /// Whether `predicate` is true for one of the row string values
    fn any_string(&self, data_index: usize, predicate: &mut dyn FnMut(&str) -> bool) -> bool;
}

pub struct Cell<'a> {
    pub value_type: ValueType,
    pub value: Option<&'a str>,
}

impl TableSource for Vec<JsonArrayEntries<String>> {
    fn rows_count(&self) -> usize {
        self.len()
    }

    fn row_index(&self, data_index: usize) -> usize {
        self[data_index].index
    }

    fn cell(&self, data_index: usize, column_id: usize) -> Option<Cell<'_>> {
        find_cell(&self[data_index], column_id).map(|entry| Cell {
            value_type: entry.pointer.value_type,
            value: entry.value.as_deref(),
        })
    }

    fn any_string(&self, data_index: usize, predicate: &mut dyn FnMut(&str) -> bool) -> bool {
        self[data_index].entries.iter().any(|entry| {
            matches!(entry.pointer.value_type, ValueType::String)
                && entry.value.as_deref().is_some_and(&mut *predicate)
        })
    }
}

/// Cell value kept as a position in the json, 16 bytes.
#[derive(Clone, Copy)]
pub struct CompactCell {
    /// Index in `CompactRows::column_ids`
    pub column: u32,
    /// Start of the value, relative to the start of its row in the json
    pub start: u32,
    pub len: u32,
    pub kind: CellKind,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CellKind {
    String,
    Number,
    Bool,
    Null,
    Object,
    Array,
    /// Value not kept, e.g empty array
    NoValue(ValueKind),
}

/// Value type of a cell without value
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ValueKind {
    Object,
    Array,
}

impl CellKind {
    pub fn of(value_type: ValueType, has_value: bool) -> Self {
        match (value_type, has_value) {
            (ValueType::String, _) => Self::String,
            (ValueType::Number, _) => Self::Number,
            (ValueType::Bool, _) => Self::Bool,
            (ValueType::Object(..), true) => Self::Object,
            (ValueType::Array(_), true) => Self::Array,
            (ValueType::Object(..), false) => Self::NoValue(ValueKind::Object),
            (ValueType::Array(_), false) => Self::NoValue(ValueKind::Array),
            (ValueType::Null, _) | (ValueType::None, _) => Self::Null,
        }
    }

    fn value_type(self) -> ValueType {
        match self {
            Self::String => ValueType::String,
            Self::Number => ValueType::Number,
            Self::Bool => ValueType::Bool,
            Self::Null => ValueType::Null,
            Self::Object | Self::NoValue(ValueKind::Object) => ValueType::Object(true, 0),
            Self::Array | Self::NoValue(ValueKind::Array) => ValueType::Array(0),
        }
    }

    fn has_value(self) -> bool {
        !matches!(self, Self::Null | Self::NoValue(_))
    }
}

/// Rows of a json array as positions of their values in the json, for read only tables:
/// values are not copied, json is kept as is.
pub struct CompactRows {
    json: String,
    cells: Vec<CompactCell>,
    /// For each row: start of the row in json and index of its first cell. Rows are in json array order.
    rows: Vec<(usize, usize)>,
    column_ids: Vec<usize>,
    column_index_by_id: IntMap<usize, u32>,
}

impl CompactRows {
    /// `rows`: for each row, start of the row in json and its cells with their column id.
    /// `column_ids`: id of each column, cells refer to their column by its index in it.
    pub fn new(json: String, rows: Vec<(usize, Vec<(usize, CompactCell)>)>, column_ids: Vec<usize>) -> Self {
        let column_index_by_id: IntMap<usize, u32> = column_ids
            .iter()
            .enumerate()
            .map(|(index, id)| (*id, index as u32))
            .collect();
        let mut cells = Vec::with_capacity(rows.iter().map(|(_, cells)| cells.len()).sum());
        let mut row_starts = Vec::with_capacity(rows.len());
        for (row_start, row_cells) in rows {
            row_starts.push((row_start, cells.len()));
            cells.extend(row_cells.into_iter().map(|(column_id, cell)| CompactCell {
                column: column_index_by_id[&column_id],
                ..cell
            }));
        }
        Self {
            json,
            cells,
            rows: row_starts,
            column_ids,
            column_index_by_id,
        }
    }

    fn row_cells(&self, data_index: usize) -> &[CompactCell] {
        let start = self.rows[data_index].1;
        let end = self
            .rows
            .get(data_index + 1)
            .map_or(self.cells.len(), |row| row.1);
        &self.cells[start..end]
    }

    fn value(&self, data_index: usize, cell: &CompactCell) -> Option<&str> {
        if !cell.kind.has_value() {
            return None;
        }
        let start = self.rows[data_index].0 + cell.start as usize;
        Some(&self.json[start..start + cell.len as usize])
    }

    pub fn column_ids(&self) -> &[usize] {
        &self.column_ids
    }

    /// Memory used, json included
    pub fn size(&self) -> usize {
        self.json.capacity()
            + self.cells.capacity() * size_of::<CompactCell>()
            + self.rows.capacity() * size_of::<(usize, usize)>()
    }
}

impl TableSource for CompactRows {
    fn rows_count(&self) -> usize {
        self.rows.len()
    }

    fn row_index(&self, data_index: usize) -> usize {
        data_index
    }

    fn cell(&self, data_index: usize, column_id: usize) -> Option<Cell<'_>> {
        let column = *self.column_index_by_id.get(&column_id)?;
        let cell = self
            .row_cells(data_index)
            .iter()
            .find(|cell| cell.column == column)?;
        Some(Cell {
            value_type: cell.kind.value_type(),
            value: self.value(data_index, cell),
        })
    }

    fn any_string(&self, data_index: usize, predicate: &mut dyn FnMut(&str) -> bool) -> bool {
        self.row_cells(data_index).iter().any(|cell| {
            cell.kind == CellKind::String
                && self.value(data_index, cell).is_some_and(&mut *predicate)
        })
    }
}
