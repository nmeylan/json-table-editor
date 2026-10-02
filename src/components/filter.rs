use crate::components::popover::{FilterMode, PopupMenu};
use eframe::egui::{Align, Button, Id, Layout, Response, Sense, TextBuffer, Ui, WidgetText};
use egui::text::CharIndex;
use std::collections::HashSet;
use std::hash::Hash;

/// Checkbox list of values in a popover, with text search.
pub struct Filter {
    id_source: Id,
    inverse_filter: bool, // When true, Checkbox checked for not filtered items
}

impl Filter {
    pub fn new(id_source: Id) -> Self {
        Self {
            id_source,
            inverse_filter: false,
        }
    }

    pub fn inverse_filter(mut self) -> Self {
        self.inverse_filter = true;
        self
    }

    /// `before_items` is rendered above the value list.
    pub fn show_ui<
        I1,
        I2,
        IT1: PartialEq + Eq + Hash + Clone + AsRef<str> + Into<WidgetText> + std::fmt::Debug,
        IT2: PartialEq + Clone,
    >(
        self,
        ui: &mut Ui,
        button_fn: impl FnOnce(&mut Ui) -> Response,
        before_items: impl FnOnce(&mut Ui),
        selected_items: impl Fn() -> I1,
        selectable_items: impl Fn() -> I2,
        item_resolver: impl Fn(&IT2) -> IT1,
    ) -> FilterResponse<IT1>
    where
        I1: Iterator<Item = IT1>,
        I2: Iterator<Item = IT2>,
    {
        let mut response = FilterResponse::default();

        let popup_menu = PopupMenu::new(self.id_source).height(300.0).with_filter();
        popup_menu.show_ui(
            ui,
            |ui| button_fn(ui),
            |ui, state| {
                before_items(ui);
                ui.spacing_mut().item_spacing.x = 0.0;
                let mut selected_items = selected_items().collect::<HashSet<IT1>>();
                if self.inverse_filter {
                    if state.clicked_select_all {
                        response.selected_items = Some(HashSet::default());
                    }
                    if state.clicked_unselect_all {
                        response.selected_items = Some(
                            selectable_items()
                                .map(|i| item_resolver(&i))
                                .collect::<HashSet<IT1>>(),
                        );
                    }
                } else {
                    if state.clicked_select_all {
                        response.selected_items = Some(
                            selectable_items()
                                .map(|i| item_resolver(&i))
                                .collect::<HashSet<IT1>>(),
                        );
                    }
                    if state.clicked_unselect_all {
                        response.selected_items = Some(HashSet::default());
                    }
                }

                let mut matching = vec![];

                // Always use virtual scrolling for performance
                let all_items: Vec<_> = selectable_items().collect();

                // Pre-filter items if filter or show_only_selected is active
                let filtered_items: Vec<_> = if !state.filter.is_empty() || state.show_only_selected
                {
                    all_items
                        .iter()
                        .filter_map(|value| {
                            let item = item_resolver(value);
                            let item_selected = selected_items.contains(&item);

                            let mut not_match = false;
                            if !state.filter.is_empty() {
                                not_match = match state.filter_mode {
                                    FilterMode::Contains => !Self::contains_case_insensitive(
                                        item.as_ref(),
                                        state.filter.as_str(),
                                    ),
                                    FilterMode::StartsWith => !Self::starts_with_case_insensitive(
                                        item.as_ref(),
                                        state.filter.as_str(),
                                    ),
                                    FilterMode::Equals => {
                                        !item.as_ref().eq_ignore_ascii_case(state.filter.as_str())
                                    }
                                };
                            }
                            let is_selected = if self.inverse_filter {
                                !item_selected
                            } else {
                                item_selected
                            };
                            if not_match || (state.show_only_selected && !is_selected) {
                                return None;
                            }

                            Some((value, item))
                        })
                        .collect()
                } else {
                    all_items
                        .iter()
                        .map(|value| {
                            let item = item_resolver(value);
                            (value, item)
                        })
                        .collect()
                };

                // Collect matching items for select/unselect all matching operations
                if !state.filter.is_empty()
                    && (state.clicked_select_all_matching
                        || state.clicked_unselect_all_matching
                        || state.clicked_replace_with_matching
                        || state.clicked_replace_without_matching)
                {
                    matching = filtered_items
                        .iter()
                        .map(|(_, item)| item.clone())
                        .collect();
                }

                // Virtual scrolling: estimate visible range
                let row_height = ui.spacing().interact_size.y;
                let clip_rect = ui.clip_rect();
                let available_height = clip_rect.height();

                // During sizing pass (first frame only), render ALL items to get proper width
                // After sizing pass, use virtual scrolling
                let (start_idx, end_idx) = if state.is_sizing_pass {
                    // Render ALL items during sizing to get accurate width (happens once on popup open)
                    (0, filtered_items.len())
                } else {
                    // Normal virtual scrolling after sizing
                    let cursor_y = ui.cursor().min.y;
                    let scroll_offset = (clip_rect.min.y - cursor_y).max(0.0);

                    let start_idx = (scroll_offset / row_height).floor().max(0.0) as usize;
                    let start_idx = start_idx.min(filtered_items.len());
                    let visible_count = ((available_height / row_height).ceil() as usize + 5)
                        .min(filtered_items.len());
                    let end_idx = (start_idx + visible_count).min(filtered_items.len());
                    (start_idx, end_idx)
                };

                // Add spacer for items before visible range (only when not sizing)
                if !state.is_sizing_pass && start_idx > 0 {
                    ui.add_space(start_idx as f32 * row_height);
                }

                // Only process visible items
                for (_value, item) in &filtered_items[start_idx..end_idx] {
                    let item_selected = selected_items.contains(item);

                    ui.horizontal(|ui| {
                        let rect = ui.available_rect_before_wrap();
                        ui.set_width(ui.available_width());
                        let mut chcked = item_selected;
                        if state.is_sizing_pass || ui.is_rect_visible(rect) {
                            if self.inverse_filter {
                                chcked = !chcked;
                                if ui.checkbox(&mut chcked, item.as_ref()).clicked() {
                                    if !chcked {
                                        selected_items.insert(item.clone());
                                    } else {
                                        selected_items.retain(|i| !i.eq(item));
                                    }
                                    response.selected_items = Some(selected_items.clone());
                                }
                            } else if ui.checkbox(&mut chcked, item.as_ref()).clicked() {
                                if !chcked {
                                    selected_items.retain(|i| !i.eq(item));
                                } else {
                                    selected_items.insert(item.clone());
                                }
                                response.selected_items = Some(selected_items.clone());
                            }
                        } else {
                            ui.label("");
                        }

                        if ui.is_rect_visible(rect) || state.is_sizing_pass {
                            let response_interact =
                                ui.interact(rect, Id::new(item), Sense::hover());
                            if response_interact.contains_pointer() {
                                ui.add_space(4.0);
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.add(Button::new("only").small()).clicked() {
                                        if self.inverse_filter {
                                            selected_items = all_items
                                                .iter()
                                                .map(&item_resolver)
                                                .filter(|i| !i.eq(item))
                                                .collect::<HashSet<IT1>>();
                                        } else {
                                            selected_items.clear();
                                            selected_items.insert(item.clone());
                                        }
                                        response.selected_items = Some(selected_items.clone());
                                    }
                                });
                            } else if state.is_sizing_pass {
                                ui.add_space(4.0);
                                ui.add(Button::new("only").small());
                            }
                        }
                    });
                }

                // Add spacer for items after visible range (only when not sizing)
                if !state.is_sizing_pass {
                    let remaining = filtered_items.len().saturating_sub(end_idx);
                    if remaining > 0 {
                        ui.add_space(remaining as f32 * row_height);
                    }
                }

                if self.inverse_filter {
                    if state.clicked_replace_with_matching {
                        selected_items = all_items
                            .iter()
                            .map(&item_resolver)
                            .filter(|i| !matching.contains(i))
                            .collect::<HashSet<IT1>>();
                        response.selected_items = Some(selected_items.clone());
                    } else if state.clicked_replace_without_matching {
                        selected_items = matching.iter().cloned().collect::<HashSet<IT1>>();
                        response.selected_items = Some(selected_items.clone());
                    } else if state.clicked_select_all_matching {
                        selected_items.retain(|i| !matching.contains(i));
                        response.selected_items = Some(selected_items.clone());
                    } else if state.clicked_unselect_all_matching {
                        selected_items.extend(matching);
                        response.selected_items = Some(selected_items.clone());
                    }
                } else if state.clicked_replace_with_matching {
                    selected_items.clear();
                    selected_items.extend(matching);
                    response.selected_items = Some(selected_items.clone());
                } else if state.clicked_replace_without_matching {
                    selected_items = all_items
                        .iter()
                        .map(&item_resolver)
                        .filter(|i| !matching.contains(i))
                        .collect::<HashSet<IT1>>();
                    response.selected_items = Some(selected_items.clone());
                } else if state.clicked_select_all_matching {
                    selected_items.extend(matching);
                    response.selected_items = Some(selected_items.clone());
                } else if state.clicked_unselect_all_matching {
                    selected_items.retain(|i| !matching.contains(i));
                    response.selected_items = Some(selected_items.clone());
                }
            },
        );

        response
    }

    fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        if needle.len() > haystack.len() {
            return false;
        }

        'outer: for i in 0..=haystack.len() - needle.len() {
            if i >= haystack.len() {
                break 'outer;
            }
            let haystack_slice = &haystack.char_range(CharIndex(i)..CharIndex(haystack.len()));
            let mut h_iter = haystack_slice.chars();
            let mut n_iter = needle.chars();

            loop {
                match (n_iter.next(), h_iter.next()) {
                    (None, _) => return true, // Found a match
                    (Some(n), Some(h)) => {
                        if !n.eq_ignore_ascii_case(&h) {
                            continue 'outer; // Character mismatch, try next position
                        }
                    }
                    _ => continue 'outer, // Not enough characters left in haystack
                }
            }
        }

        false
    }
    fn starts_with_case_insensitive(haystack: &str, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        if needle.len() > haystack.len() {
            return false;
        }

        let mut h_iter = haystack.chars();
        let mut n_iter = needle.chars();

        loop {
            match (n_iter.next(), h_iter.next()) {
                (None, _) => return true, // Found a match - all needle chars matched
                (Some(n), Some(h)) => {
                    if !n.eq_ignore_ascii_case(&h) {
                        return false; // Character mismatch at start
                    }
                }
                _ => return false, // Not enough characters in haystack
            }
        }
    }
}

pub struct FilterResponse<IT> {
    selected_items: Option<HashSet<IT>>,
}

impl<IT> FilterResponse<IT> {
    pub fn selected_items(&mut self) -> &mut Option<HashSet<IT>> {
        &mut self.selected_items
    }
}

impl<IT> Default for FilterResponse<IT> {
    fn default() -> Self {
        Self {
            selected_items: None,
        }
    }
}
