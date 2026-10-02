use super::Column;
use super::cell_lookup::find_cell;
use crate::parser::column_id;
use indexmap::{IndexMap, IndexSet};
use json_flat_parser::{FlatJsonValue, JsonArrayEntries, ValueType};
use rayon::prelude::*;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum ColumnFilter {
    /// Show only rows whose value is one of these.
    Include(HashSet<String>),
    /// Hide rows whose value is one of these. Null and missing values stay visible.
    Exclude(HashSet<String>),
    /// Hide rows whose value is null or missing.
    NonNull,
}

impl ColumnFilter {
    #[inline]
    fn matches(&self, entry: Option<&FlatJsonValue<String>>) -> bool {
        let value = entry.and_then(|entry| entry.value.as_ref());
        match self {
            Self::Include(values) => value.is_some_and(|value| values.contains(value)),
            Self::Exclude(values) => value.is_none_or(|value| !values.contains(value)),
            Self::NonNull => value.is_some(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

/// Value of a row cell in the sorted column. Variant order is the type rank.
#[derive(PartialEq, PartialOrd)]
enum SortKey<'a> {
    Bool(bool),
    Number(f64),
    Str(&'a str),
}

impl<'a> SortKey<'a> {
    /// `None` when null or missing.
    fn of(entry: Option<&'a FlatJsonValue<String>>) -> Option<Self> {
        let entry = entry?;
        let value = entry.value.as_deref()?;
        match entry.pointer.value_type {
            ValueType::Null => None,
            ValueType::Bool => Some(Self::Bool(value == "true")),
            ValueType::Number => Some(value.parse::<f64>().map_or(Self::Str(value), Self::Number)),
            _ => Some(Self::Str(value)),
        }
    }

    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => a.total_cmp(b),
            _ => self.partial_cmp(other).unwrap_or(Ordering::Equal),
        }
    }
}

/// Single owner of "which data rows are shown, in what order".
/// Data (`nodes`) is never reordered: the view only holds data indices.
pub struct RowView {
    visible: Vec<usize>,            // view index -> data index
    data_to_view: Option<Vec<u32>>, // lazily built inverse, invalidated on change
    // Keyed by column name, so a filter survives depth change and pin/unpin.
    // Revision is bumped on each change of the filter, it keys distinct values cache.
    filters: IndexMap<String, (ColumnFilter, u64)>,
    next_filter_revision: u64,
    distinct_values_cache: DistinctValuesCache,
    sort: Option<(String, SortDirection)>, // keyed by column name
}

// column name -> (key of other filters, distinct values)
type DistinctValuesCache = RefCell<HashMap<String, (u64, Arc<IndexSet<String>>)>>;

const HIDDEN: u32 = u32::MAX;

impl RowView {
    pub fn new(rows_count: usize) -> Self {
        Self {
            visible: (0..rows_count).collect(),
            data_to_view: None,
            filters: IndexMap::new(),
            next_filter_revision: 0,
            distinct_values_cache: Default::default(),
            sort: None,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.visible.len()
    }

    #[inline]
    pub fn view_to_data(&self, view_index: usize) -> usize {
        self.visible[view_index]
    }

    pub fn data_to_view(&mut self, data_index: usize) -> Option<usize> {
        let inverse = self.data_to_view.get_or_insert_with(|| {
            let len = self.visible.iter().max().map_or(0, |max| max + 1);
            let mut inverse = vec![HIDDEN; len];
            for (view_index, data_index) in self.visible.iter().enumerate() {
                inverse[*data_index] = view_index as u32;
            }
            inverse
        });
        inverse
            .get(data_index)
            .filter(|view_index| **view_index != HIDDEN)
            .map(|view_index| *view_index as usize)
    }

    /// A row was inserted in data at `data_index` and is shown at `view_index`.
    pub fn on_row_inserted(&mut self, view_index: usize, data_index: usize) {
        for visible_data_index in self.visible.iter_mut() {
            if *visible_data_index >= data_index {
                *visible_data_index += 1;
            }
        }
        self.visible.insert(view_index, data_index);
        self.data_to_view = None;
        self.rows_changed();
    }

    /// Any change of rows data (edit, insert, replace, depth change).
    pub fn rows_changed(&mut self) {
        self.distinct_values_cache.get_mut().clear();
    }

    pub fn active_filters(&self) -> impl Iterator<Item = &String> {
        self.filters.keys()
    }

    #[inline]
    pub fn filter(&self, column: &str) -> Option<&ColumnFilter> {
        self.filters.get(column).map(|(filter, _)| filter)
    }

    /// `None`, or an empty exclusion, removes the column filter.
    pub fn set_filter(&mut self, column: String, filter: Option<ColumnFilter>) {
        match filter {
            Some(ColumnFilter::Exclude(values)) if values.is_empty() => self.remove_filter(&column),
            None => self.remove_filter(&column),
            Some(filter) => {
                self.next_filter_revision += 1;
                self.filters
                    .insert(column, (filter, self.next_filter_revision));
            }
        }
    }

    pub fn toggle_non_null(&mut self, column: &str) {
        if matches!(self.filter(column), Some(ColumnFilter::NonNull)) {
            self.remove_filter(column);
        } else {
            self.set_filter(column.to_string(), Some(ColumnFilter::NonNull));
        }
    }

    pub fn remove_filter(&mut self, column: &str) {
        self.filters.shift_remove(column);
    }

    pub fn clear_filters(&mut self) {
        self.filters.clear();
    }

    #[inline]
    pub fn sort(&self) -> Option<(&str, SortDirection)> {
        self.sort
            .as_ref()
            .map(|(column, direction)| (column.as_str(), *direction))
    }

    /// `None` restores data order.
    pub fn set_sort(&mut self, sort: Option<(String, SortDirection)>) {
        self.sort = sort;
    }

    /// Filter then sort.
    pub fn recompute(&mut self, nodes: &[JsonArrayEntries<String>]) {
        #[cfg(debug_assertions)]
        let start = crate::compatibility::now();
        self.data_to_view = None;
        let filters = compiled_filters(&self.filters, None);
        self.visible = if filters.is_empty() {
            (0..nodes.len()).collect()
        } else {
            nodes
                .par_iter()
                .enumerate()
                .filter(|(_, row)| matches_all(row, &filters))
                .map(|(data_index, _)| data_index)
                .collect()
        };
        if let Some((column, direction)) = self.sort.as_ref() {
            let id = column_id(column);
            let mut keys = self
                .visible
                .par_iter()
                .map(|data_index| (SortKey::of(find_cell(&nodes[*data_index], id)), *data_index))
                .collect::<Vec<(Option<SortKey>, usize)>>();
            keys.par_sort_by(|(a, a_index), (b, b_index)| {
                let ordering = match (a, b) {
                    (Some(a), Some(b)) => match direction {
                        SortDirection::Asc => a.cmp(b),
                        SortDirection::Desc => b.cmp(a),
                    },
                    // Null and missing values go last in both directions
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => Ordering::Equal,
                };
                // Data index tie-breaker makes the sort stable
                ordering.then(a_index.cmp(b_index))
            });
            self.visible = keys.into_iter().map(|(_, data_index)| data_index).collect();
        }
        #[cfg(debug_assertions)]
        crate::log!(
            "RowView::recompute {} rows -> {} visible in {}ms",
            nodes.len(),
            self.visible.len(),
            start.elapsed().as_millis()
        );
    }

    /// Distinct values of `column` among rows passing every filter except the column's own.
    pub fn distinct_values(
        &self,
        nodes: &[JsonArrayEntries<String>],
        column: &Column,
    ) -> Arc<IndexSet<String>> {
        let mut hasher = DefaultHasher::new();
        for (name, (_, revision)) in self.filters.iter() {
            if name != column.name.as_ref() {
                name.hash(&mut hasher);
                revision.hash(&mut hasher);
            }
        }
        let key = hasher.finish();
        if let Some((cached_key, values)) = self
            .distinct_values_cache
            .borrow()
            .get(column.name.as_ref())
            && *cached_key == key
        {
            return values.clone();
        }

        let filters = compiled_filters(&self.filters, Some(column.name.as_ref()));
        let id = column_id(&column.name);
        let values = nodes
            .par_iter()
            .filter(|row| matches_all(row, &filters))
            .filter_map(|row| find_cell(row, id).and_then(|entry| entry.value.as_ref()))
            .collect::<Vec<&String>>();
        let mut unique_values = values.into_iter().collect::<IndexSet<&String>>();
        if matches!(column.value_type, ValueType::Number) {
            unique_values.sort_by(|a, b| {
                let num_a = a.parse::<f64>();
                let num_b = b.parse::<f64>();

                // Compare parsed numbers; handle parse errors by pushing them to the end
                match (num_a, num_b) {
                    (Ok(a), Ok(b)) => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal),
                    (Ok(_), Err(_)) => std::cmp::Ordering::Less, // Numbers are less than errors
                    (Err(_), Ok(_)) => std::cmp::Ordering::Greater, // Errors are greater than numbers
                    (Err(_), Err(_)) => std::cmp::Ordering::Equal,  // Treat errors equally
                }
            });
        } else {
            unique_values.sort();
        }
        let values = Arc::new(
            unique_values
                .into_iter()
                .cloned()
                .collect::<IndexSet<String>>(),
        );
        self.distinct_values_cache
            .borrow_mut()
            .insert(column.name.to_string(), (key, values.clone()));
        values
    }
}

fn compiled_filters<'a>(
    filters: &'a IndexMap<String, (ColumnFilter, u64)>,
    except_column: Option<&str>,
) -> Vec<(usize, &'a ColumnFilter)> {
    filters
        .iter()
        .filter(|(name, _)| except_column != Some(name.as_str()))
        .map(|(name, (filter, _))| (column_id(name), filter))
        .collect()
}

#[inline]
fn matches_all(row: &JsonArrayEntries<String>, filters: &[(usize, &ColumnFilter)]) -> bool {
    filters
        .iter()
        .all(|(column_id, filter)| filter.matches(find_cell(row, *column_id)))
}

#[cfg(test)]
mod tests {
    use super::{ColumnFilter, RowView, SortDirection};
    use crate::array_table::Column;
    use crate::parser::{as_array, change_depth_array};
    use json_flat_parser::{JSONParser, JsonArrayEntries, ParseOptions, ValueType};

    fn sort(view: &mut RowView, column: &str, direction: SortDirection) {
        view.set_sort(Some((column.to_string(), direction)));
    }
    use std::collections::HashSet;

    pub(crate) fn nodes(json: &str) -> Vec<JsonArrayEntries<String>> {
        let result = JSONParser::parse(json, ParseOptions::default().parse_array(false))
            .unwrap()
            .to_owned();
        as_array(result).unwrap().0
    }

    fn visible(view: &RowView) -> Vec<usize> {
        (0..view.len()).map(|i| view.view_to_data(i)).collect()
    }

    fn set(values: &[&str]) -> HashSet<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn include(view: &mut RowView, column: &str, values: &[&str]) {
        view.set_filter(column.to_string(), Some(ColumnFilter::Include(set(values))));
    }

    fn exclude(view: &mut RowView, column: &str, values: &[&str]) {
        view.set_filter(column.to_string(), Some(ColumnFilter::Exclude(set(values))));
    }

    fn distinct(view: &RowView, nodes: &[JsonArrayEntries<String>], column: &str) -> Vec<String> {
        let column = Column::new(column.to_string(), ValueType::String);
        view.distinct_values(nodes, &column)
            .iter()
            .cloned()
            .collect()
    }

    const ROWS: &str = r#"[
        {"name": "a", "kind": "x"},
        {"name": "b", "kind": "y"},
        {"name": "c", "kind": null},
        {"name": "d"},
        {"name": "e", "kind": "x"}
    ]"#;

    #[test]
    fn mapping_both_ways() {
        let nodes = nodes(ROWS);
        let mut view = RowView::new(nodes.len());
        include(&mut view, "/kind", &["x"]);
        view.recompute(&nodes);
        assert_eq!(visible(&view), vec![0, 4]);
        assert_eq!(view.data_to_view(0), Some(0));
        assert_eq!(view.data_to_view(4), Some(1));
        assert_eq!(view.data_to_view(1), None);
        assert_eq!(view.data_to_view(42), None);
    }

    #[test]
    fn include_ors_within_a_column() {
        let nodes = nodes(ROWS);
        let mut view = RowView::new(nodes.len());
        include(&mut view, "/kind", &["x", "y"]);
        view.recompute(&nodes);
        // null value and missing entry are not included
        assert_eq!(visible(&view), vec![0, 1, 4]);
    }

    #[test]
    fn empty_include_hides_every_row() {
        let nodes = nodes(ROWS);
        let mut view = RowView::new(nodes.len());
        include(&mut view, "/kind", &[]);
        view.recompute(&nodes);
        assert_eq!(visible(&view), Vec::<usize>::new());
    }

    #[test]
    fn exclude_hides_matching_value() {
        let nodes = nodes(ROWS);
        let mut view = RowView::new(nodes.len());
        exclude(&mut view, "/kind", &["x"]);
        view.recompute(&nodes);
        // null value and missing entry stay visible
        assert_eq!(visible(&view), vec![1, 2, 3]);
    }

    #[test]
    fn empty_exclude_removes_the_filter() {
        let mut view = RowView::new(0);
        exclude(&mut view, "/kind", &["x"]);
        exclude(&mut view, "/kind", &[]);
        assert_eq!(view.active_filters().count(), 0);
    }

    #[test]
    fn non_null_hides_null_and_missing() {
        let nodes = nodes(ROWS);
        let mut view = RowView::new(nodes.len());
        view.toggle_non_null("/kind");
        view.recompute(&nodes);
        assert_eq!(visible(&view), vec![0, 1, 4]);
        view.toggle_non_null("/kind");
        assert_eq!(view.filter("/kind"), None);
    }

    #[test]
    fn filters_are_anded_across_columns() {
        let nodes = nodes(ROWS);
        let mut view = RowView::new(nodes.len());
        include(&mut view, "/kind", &["x"]);
        include(&mut view, "/name", &["e", "b"]);
        view.recompute(&nodes);
        assert_eq!(visible(&view), vec![4]);
        exclude(&mut view, "/name", &["e"]);
        view.recompute(&nodes);
        assert_eq!(visible(&view), vec![0]);
    }

    #[test]
    fn clear_filters_restores_all_rows() {
        let nodes = nodes(ROWS);
        let mut view = RowView::new(nodes.len());
        include(&mut view, "/kind", &["x"]);
        view.toggle_non_null("/name");
        view.recompute(&nodes);
        assert_eq!(view.len(), 2);
        view.clear_filters();
        view.recompute(&nodes);
        assert_eq!(view.active_filters().count(), 0);
        assert_eq!(visible(&view), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn filter_on_a_missing_column_stays_active() {
        let nodes = nodes(ROWS);
        let mut view = RowView::new(nodes.len());
        include(&mut view, "/not_a_column", &["x"]);
        view.recompute(&nodes);
        assert_eq!(
            view.active_filters().collect::<Vec<_>>(),
            vec!["/not_a_column"]
        );
        assert_eq!(view.len(), 0);
    }

    #[test]
    fn distinct_values_cascade_from_other_filters_only() {
        let nodes = nodes(ROWS);
        let mut view = RowView::new(nodes.len());
        assert_eq!(distinct(&view, &nodes, "/kind"), vec!["x", "y"]);
        include(&mut view, "/kind", &["x"]);
        // Own filter is ignored
        assert_eq!(distinct(&view, &nodes, "/kind"), vec!["x", "y"]);
        // Other columns only list values of rows passing the other filters
        assert_eq!(distinct(&view, &nodes, "/name"), vec!["a", "e"]);
        view.remove_filter("/kind");
        assert_eq!(
            distinct(&view, &nodes, "/name"),
            vec!["a", "b", "c", "d", "e"]
        );
    }

    #[test]
    fn distinct_values_cache_is_invalidated_by_row_changes() {
        let mut nodes = nodes(ROWS);
        let mut view = RowView::new(nodes.len());
        assert_eq!(distinct(&view, &nodes, "/kind"), vec!["x", "y"]);
        let entry = nodes[1]
            .entries
            .iter_mut()
            .find(|e| e.pointer.pointer == "/1/kind")
            .unwrap();
        entry.value = Some("z".to_string());
        // Not told about the change: cached
        assert_eq!(distinct(&view, &nodes, "/kind"), vec!["x", "y"]);
        view.rows_changed();
        assert_eq!(distinct(&view, &nodes, "/kind"), vec!["x", "z"]);
    }

    #[test]
    fn distinct_numbers_are_sorted_numerically() {
        let nodes = nodes(r#"[{"n": 10}, {"n": 9}, {"n": 100}, {"n": 9}]"#);
        let view = RowView::new(nodes.len());
        let column = Column::new("/n".to_string(), ValueType::Number);
        let values = view.distinct_values(&nodes, &column);
        assert_eq!(values.iter().collect::<Vec<_>>(), vec!["9", "10", "100"]);
    }

    #[test]
    fn insert_shifting() {
        let mut view = RowView::new(5);
        view.on_row_inserted(2, 2);
        assert_eq!(visible(&view), vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(view.data_to_view(5), Some(5));
    }

    #[test]
    fn insert_shifts_every_data_index_after_insertion_point() {
        // Not data order: view shows 4, 0, 3
        let mut view = RowView::new(0);
        view.visible = vec![4, 0, 3];
        // Insert a row in data at 3, shown right after data row 4
        view.on_row_inserted(1, 3);
        assert_eq!(visible(&view), vec![5, 3, 0, 4]);
        assert_eq!(view.data_to_view(3), Some(1));
        assert_eq!(view.data_to_view(5), Some(0));
    }

    const MIXED: &str = r#"[
        {"v": "b"},
        {"v": 10},
        {"v": null},
        {"v": true},
        {},
        {"v": 9},
        {"v": "a"},
        {"v": false}
    ]"#;

    #[test]
    fn sort_by_type_rank_then_value_with_nulls_last() {
        let nodes = nodes(MIXED);
        let mut view = RowView::new(nodes.len());
        sort(&mut view, "/v", SortDirection::Asc);
        view.recompute(&nodes);
        // false, true, 9, 10, "a", "b", then null and missing in data order
        assert_eq!(visible(&view), vec![7, 3, 5, 1, 6, 0, 2, 4]);
        sort(&mut view, "/v", SortDirection::Desc);
        view.recompute(&nodes);
        assert_eq!(visible(&view), vec![0, 6, 1, 5, 3, 7, 2, 4]);
    }

    #[test]
    fn numbers_are_compared_numerically() {
        let nodes = nodes(r#"[{"n": 10}, {"n": 9}, {"n": -1.5}, {"n": 100}]"#);
        let mut view = RowView::new(nodes.len());
        sort(&mut view, "/n", SortDirection::Asc);
        view.recompute(&nodes);
        assert_eq!(visible(&view), vec![2, 1, 0, 3]);
    }

    #[test]
    fn sort_is_stable() {
        let nodes = nodes(r#"[{"k": "b"}, {"k": "a"}, {"k": "b"}, {"k": "a"}, {"k": "b"}]"#);
        let mut view = RowView::new(nodes.len());
        sort(&mut view, "/k", SortDirection::Asc);
        view.recompute(&nodes);
        assert_eq!(visible(&view), vec![1, 3, 0, 2, 4]);
        sort(&mut view, "/k", SortDirection::Desc);
        view.recompute(&nodes);
        assert_eq!(visible(&view), vec![0, 2, 4, 1, 3]);
    }

    #[test]
    fn clearing_sort_restores_data_order() {
        let nodes = nodes(ROWS);
        let mut view = RowView::new(nodes.len());
        sort(&mut view, "/name", SortDirection::Desc);
        view.recompute(&nodes);
        assert_eq!(visible(&view), vec![4, 3, 2, 1, 0]);
        view.set_sort(None);
        view.recompute(&nodes);
        assert_eq!(visible(&view), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn sort_and_filter_combined() {
        let nodes = nodes(ROWS);
        let mut view = RowView::new(nodes.len());
        include(&mut view, "/kind", &["x", "y"]);
        sort(&mut view, "/name", SortDirection::Desc);
        view.recompute(&nodes);
        assert_eq!(visible(&view), vec![4, 1, 0]);
        // Filter change keeps the sort
        exclude(&mut view, "/kind", &["y"]);
        view.recompute(&nodes);
        assert_eq!(visible(&view), vec![4, 3, 2, 0]);
    }

    #[test]
    fn sort_and_insert() {
        let nodes = nodes(ROWS);
        let mut view = RowView::new(nodes.len());
        sort(&mut view, "/name", SortDirection::Desc);
        view.recompute(&nodes);
        assert_eq!(visible(&view), vec![4, 3, 2, 1, 0]);
        // Insert in data below data row 1 (view row 3): new row is data 2, shown at view row 4
        view.on_row_inserted(4, 2);
        assert_eq!(visible(&view), vec![5, 4, 3, 1, 2, 0]);
        assert_eq!(view.data_to_view(2), Some(4));
    }

    #[test]
    fn sort_survives_depth_change() {
        let json = r#"[{"n": 2, "o": {"a": 1}}, {"n": 1, "o": {"a": 2}}, {"n": 3, "o": {"a": 3}}]"#;
        let result = JSONParser::parse(
            json,
            ParseOptions::default().parse_array(false).max_depth(2),
        )
        .unwrap()
        .to_owned();
        let parse_result = result.clone_except_json();
        let nodes = as_array(result).unwrap().0;
        let mut view = RowView::new(nodes.len());
        sort(&mut view, "/n", SortDirection::Asc);
        view.recompute(&nodes);
        assert_eq!(visible(&view), vec![1, 0, 2]);

        let (deeper_nodes, _, _) = change_depth_array(parse_result, nodes, 3).unwrap();
        view.rows_changed();
        view.recompute(&deeper_nodes);
        assert_eq!(visible(&view), vec![1, 0, 2]);
        // Columns that only exist at the new depth are sortable
        sort(&mut view, "/o/a", SortDirection::Desc);
        view.recompute(&deeper_nodes);
        assert_eq!(visible(&view), vec![2, 1, 0]);
    }
}
