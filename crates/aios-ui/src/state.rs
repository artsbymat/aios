//! Pure, headless-testable launcher state, navigation stack, host fuzzy filtering,
//! form validation, and keyboard interaction controller.

use aios_core::view::{
    ActionItem, ActionPanel, ConfirmAlert, FilterMode, FormField, GridItem, HudMessage, ListItem,
    ToastMessage, ToastStyle, ViewNode,
};
use aios_fuzzy::{CaseMatching, FrecencyConfig, MatchResult, QueryParser, SearchItem, score_item};

/// Which input control currently receives typed characters inside the header or form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderFocus {
    Search,
    InlineArg(usize),
    FilterDropdown,
}

/// Active overlay popover or modal sitting on Plane 2 above the main canvas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveOverlay {
    ActionPanel {
        filter_query: String,
        selected_index: usize,
        submenu_parent: Option<String>,
    },
    FilterDropdown {
        selected_index: usize,
    },
    ConfirmAlert(ConfirmAlert),
}

/// A single filtered list row paired with its original indices and fuzzy highlight offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilteredListEntry {
    pub section_index: usize,
    pub item_index: usize,
    pub match_result: MatchResult,
}

/// A single filtered grid tile paired with its original indices and fuzzy highlight offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilteredGridEntry {
    pub section_index: usize,
    pub item_index: usize,
    pub match_result: MatchResult,
}

/// One frame in the navigation stack (`Push`, `Pop`, `PopToRoot`).
#[derive(Debug, Clone)]
pub struct NavFrame {
    pub id: String,
    pub view: ViewNode,
    pub search_query: String,
    pub inline_arg_values: Vec<String>,
    pub header_focus: HeaderFocus,
    pub selected_index: usize,
    pub focused_form_field: usize,
    /// Optional `(plugin_id, command_id)` owning this view frame for interactive WASM events.
    pub plugin_session: Option<(String, String)>,
}

impl NavFrame {
    #[must_use]
    pub fn new(id: impl Into<String>, view: ViewNode) -> Self {
        let arg_count = match &view {
            ViewNode::List(list) => list.inline_arguments.len(),
            _ => 0,
        };
        let focused_form_field = first_interactive_form_field(&view).unwrap_or(0);
        Self {
            id: id.into(),
            view,
            search_query: String::new(),
            inline_arg_values: vec![String::new(); arg_count],
            header_focus: HeaderFocus::Search,
            selected_index: 0,
            focused_form_field,
            plugin_session: None,
        }
    }

    #[must_use]
    pub fn with_plugin_session(
        mut self,
        plugin_id: impl Into<String>,
        command_id: impl Into<String>,
    ) -> Self {
        self.plugin_session = Some((plugin_id.into(), command_id.into()));
        self
    }

    /// Returns the ID of the currently selected list row or grid tile, if any.
    #[must_use]
    pub fn selected_item_id(&self) -> Option<String> {
        match &self.view {
            ViewNode::List(list) => {
                let entries = self.filtered_list_entries();
                entries.get(self.selected_index).map(|entry| {
                    list.sections[entry.section_index].items[entry.item_index]
                        .id
                        .clone()
                })
            }
            ViewNode::Grid(grid) => {
                let entries = self.filtered_grid_entries();
                entries.get(self.selected_index).map(|entry| {
                    grid.sections[entry.section_index].items[entry.item_index]
                        .id
                        .clone()
                })
            }
            _ => None,
        }
    }

    #[must_use]
    pub fn title(&self) -> &str {
        match &self.view {
            ViewNode::List(v) => v.navigation_title.as_deref().unwrap_or("AIOS"),
            ViewNode::Grid(v) => v.navigation_title.as_deref().unwrap_or("Grid"),
            ViewNode::Detail(v) => v.navigation_title.as_deref().unwrap_or("Detail"),
            ViewNode::Form(v) => v.navigation_title.as_deref().unwrap_or("Form"),
            ViewNode::Empty(v) => v.title.as_str(),
            ViewNode::Error(v) => v.title.as_str(),
        }
    }

    #[must_use]
    pub fn is_loading(&self) -> bool {
        match &self.view {
            ViewNode::List(v) => v.is_loading,
            ViewNode::Grid(v) => v.is_loading,
            ViewNode::Detail(v) => v.is_loading,
            ViewNode::Form(v) => v.is_loading,
            ViewNode::Empty(_) | ViewNode::Error(_) => false,
        }
    }

    /// Computes filtered list items using `aios-fuzzy` when `filtering == FilterMode::Host`.
    #[must_use]
    pub fn filtered_list_entries(&self) -> Vec<FilteredListEntry> {
        let ViewNode::List(list) = &self.view else {
            return Vec::new();
        };

        let active_filter = list
            .filter_dropdown
            .as_ref()
            .map(|d| d.selected_id.as_str())
            .filter(|id| !id.is_empty() && *id != "all");

        let query = self.search_query.trim();
        let engine = QueryParser::parse(query, CaseMatching::Smart);
        let config = FrecencyConfig::default();

        let mut entries = Vec::new();
        for (s_idx, section) in list.sections.iter().enumerate() {
            for (i_idx, item) in section.items.iter().enumerate() {
                if let Some(filter_id) = active_filter
                    && item.filter_group.as_deref() != Some(filter_id)
                {
                    continue;
                }

                if list.filtering == FilterMode::Plugin {
                    let match_result = if query.is_empty() {
                        MatchResult {
                            score: 0,
                            primary_indices: Vec::new(),
                            secondary_indices: Vec::new(),
                        }
                    } else {
                        let adapter = ListItemSearchAdapter(item);
                        score_item(&adapter, query, engine.as_ref(), None, 0, &config).unwrap_or(
                            MatchResult {
                                score: 0,
                                primary_indices: Vec::new(),
                                secondary_indices: Vec::new(),
                            },
                        )
                    };
                    entries.push(FilteredListEntry {
                        section_index: s_idx,
                        item_index: i_idx,
                        match_result,
                    });
                } else if query.is_empty() {
                    entries.push(FilteredListEntry {
                        section_index: s_idx,
                        item_index: i_idx,
                        match_result: MatchResult {
                            score: 0,
                            primary_indices: Vec::new(),
                            secondary_indices: Vec::new(),
                        },
                    });
                } else {
                    let adapter = ListItemSearchAdapter(item);
                    if let Some(res) =
                        score_item(&adapter, query, engine.as_ref(), None, 0, &config)
                    {
                        entries.push(FilteredListEntry {
                            section_index: s_idx,
                            item_index: i_idx,
                            match_result: res,
                        });
                    }
                }
            }
        }

        if list.filtering == FilterMode::Host && !query.is_empty() {
            entries.sort_by(|a, b| {
                b.match_result
                    .score
                    .cmp(&a.match_result.score)
                    .then_with(|| a.section_index.cmp(&b.section_index))
                    .then_with(|| a.item_index.cmp(&b.item_index))
            });
        }

        entries
    }

    /// Computes filtered grid items using `aios-fuzzy` when `filtering == FilterMode::Host`.
    #[must_use]
    pub fn filtered_grid_entries(&self) -> Vec<FilteredGridEntry> {
        let ViewNode::Grid(grid) = &self.view else {
            return Vec::new();
        };

        let active_filter = grid
            .filter_dropdown
            .as_ref()
            .map(|d| d.selected_id.as_str())
            .filter(|id| !id.is_empty() && *id != "all");

        let query = self.search_query.trim();
        let engine = QueryParser::parse(query, CaseMatching::Smart);
        let config = FrecencyConfig::default();

        let mut entries = Vec::new();
        for (s_idx, section) in grid.sections.iter().enumerate() {
            for (i_idx, item) in section.items.iter().enumerate() {
                if let Some(filter_id) = active_filter
                    && item.filter_group.as_deref() != Some(filter_id)
                {
                    continue;
                }

                if grid.filtering == FilterMode::Plugin || query.is_empty() {
                    entries.push(FilteredGridEntry {
                        section_index: s_idx,
                        item_index: i_idx,
                        match_result: MatchResult {
                            score: 0,
                            primary_indices: Vec::new(),
                            secondary_indices: Vec::new(),
                        },
                    });
                } else {
                    let adapter = GridItemSearchAdapter(item);
                    if let Some(res) =
                        score_item(&adapter, query, engine.as_ref(), None, 0, &config)
                    {
                        entries.push(FilteredGridEntry {
                            section_index: s_idx,
                            item_index: i_idx,
                            match_result: res,
                        });
                    }
                }
            }
        }

        if grid.filtering == FilterMode::Host && !query.is_empty() {
            entries.sort_by(|a, b| {
                b.match_result
                    .score
                    .cmp(&a.match_result.score)
                    .then_with(|| a.section_index.cmp(&b.section_index))
                    .then_with(|| a.item_index.cmp(&b.item_index))
            });
        }

        entries
    }

    /// Returns the active [`ActionPanel`] for the currently selected item or view.
    #[must_use]
    pub fn active_action_panel(&self) -> ActionPanel {
        match &self.view {
            ViewNode::List(list) => {
                let entries = self.filtered_list_entries();
                if let Some(entry) = entries.get(self.selected_index) {
                    return list.sections[entry.section_index].items[entry.item_index]
                        .actions
                        .clone();
                }
                list.empty_view
                    .as_ref()
                    .map(|e| e.actions.clone())
                    .unwrap_or_default()
            }
            ViewNode::Grid(grid) => {
                let entries = self.filtered_grid_entries();
                if let Some(entry) = entries.get(self.selected_index) {
                    return grid.sections[entry.section_index].items[entry.item_index]
                        .actions
                        .clone();
                }
                grid.empty_view
                    .as_ref()
                    .map(|e| e.actions.clone())
                    .unwrap_or_default()
            }
            ViewNode::Detail(detail) => detail.actions.clone(),
            ViewNode::Form(form) => form.actions.clone(),
            ViewNode::Empty(empty) => empty.actions.clone(),
            ViewNode::Error(error) => error.actions.clone(),
        }
    }

    /// Clamps `selected_index` safely after a query or filter change.
    pub fn clamp_selection(&mut self) {
        let len = match &self.view {
            ViewNode::List(_) => self.filtered_list_entries().len(),
            ViewNode::Grid(_) => self.filtered_grid_entries().len(),
            _ => 0,
        };
        if len == 0 {
            self.selected_index = 0;
        } else if self.selected_index >= len {
            self.selected_index = len - 1;
        }
    }
}

struct ListItemSearchAdapter<'a>(&'a ListItem);

impl SearchItem for ListItemSearchAdapter<'_> {
    fn id(&self) -> &str {
        &self.0.id
    }
    fn primary_text(&self) -> &str {
        &self.0.title
    }
    fn secondary_text(&self) -> Option<&str> {
        self.0.subtitle.as_deref()
    }
    fn keywords(&self) -> &[String] {
        &self.0.keywords
    }
    fn alias(&self) -> Option<&str> {
        self.0.alias.as_deref()
    }
}

struct GridItemSearchAdapter<'a>(&'a GridItem);

impl SearchItem for GridItemSearchAdapter<'_> {
    fn id(&self) -> &str {
        &self.0.id
    }
    fn primary_text(&self) -> &str {
        &self.0.title
    }
    fn secondary_text(&self) -> Option<&str> {
        self.0.subtitle.as_deref()
    }
    fn keywords(&self) -> &[String] {
        &self.0.keywords
    }
}

/// High-level side-effect emitted by [`LauncherState`] to the GPUI shell host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEffect {
    None,
    HideWindow,
    CycleTheme,
    SetTheme(aios_core::ThemeMode),
    ShowHud(HudMessage),
    ActionTriggered(String),
}

/// Complete navigation & interaction state for the AIOS launcher window.
#[derive(Debug, Clone)]
pub struct LauncherState {
    pub stack: Vec<NavFrame>,
    pub overlay: Option<ActiveOverlay>,
    pub toast: Option<ToastMessage>,
}

impl LauncherState {
    #[must_use]
    pub fn new(root: NavFrame) -> Self {
        Self {
            stack: vec![root],
            overlay: None,
            toast: None,
        }
    }

    #[must_use]
    pub fn current(&self) -> &NavFrame {
        self.stack.last().expect("nav stack always has root frame")
    }

    pub fn current_mut(&mut self) -> &mut NavFrame {
        self.stack
            .last_mut()
            .expect("nav stack always has root frame")
    }

    #[must_use]
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    pub fn push(&mut self, frame: NavFrame) {
        self.overlay = None;
        self.toast = None;
        self.stack.push(frame);
    }

    pub fn pop(&mut self) -> bool {
        if self.stack.len() > 1 {
            self.overlay = None;
            self.toast = None;
            self.stack.pop();
            true
        } else {
            false
        }
    }

    pub fn pop_to_root(&mut self) {
        self.overlay = None;
        self.toast = None;
        self.stack.truncate(1);
        if let Some(root) = self.stack.first_mut() {
            root.search_query.clear();
            root.header_focus = HeaderFocus::Search;
            root.selected_index = 0;
        }
    }

    pub fn show_toast(
        &mut self,
        style: ToastStyle,
        title: impl Into<String>,
        message: Option<String>,
    ) {
        self.toast = Some(ToastMessage {
            style,
            title: title.into(),
            message,
        });
    }

    pub fn clear_toast(&mut self) {
        self.toast = None;
    }

    pub fn open_confirm_alert(&mut self, alert: ConfirmAlert) {
        self.overlay = Some(ActiveOverlay::ConfirmAlert(alert));
    }

    pub fn toggle_action_panel(&mut self) {
        match &self.overlay {
            Some(ActiveOverlay::ActionPanel { .. }) => self.overlay = None,
            _ => {
                let panel = self.current().active_action_panel();
                if panel.primary_action().is_some() {
                    self.overlay = Some(ActiveOverlay::ActionPanel {
                        filter_query: String::new(),
                        selected_index: 0,
                        submenu_parent: None,
                    });
                }
            }
        }
    }

    pub fn toggle_filter_dropdown(&mut self) {
        match &self.overlay {
            Some(ActiveOverlay::FilterDropdown { .. }) => {
                self.overlay = None;
                self.current_mut().header_focus = HeaderFocus::Search;
            }
            _ => {
                let selected_idx = match &self.current().view {
                    ViewNode::List(l) => l
                        .filter_dropdown
                        .as_ref()
                        .and_then(|d| d.options.iter().position(|o| o.id == d.selected_id)),
                    ViewNode::Grid(g) => g
                        .filter_dropdown
                        .as_ref()
                        .and_then(|d| d.options.iter().position(|o| o.id == d.selected_id)),
                    _ => None,
                };
                if let Some(idx) = selected_idx {
                    self.current_mut().header_focus = HeaderFocus::FilterDropdown;
                    self.overlay = Some(ActiveOverlay::FilterDropdown {
                        selected_index: idx,
                    });
                }
            }
        }
    }

    /// Returns the flat list of actions visible inside an open `ActionPanel` popover.
    #[must_use]
    pub fn visible_action_panel_items(&self) -> Vec<ActionItem> {
        let Some(ActiveOverlay::ActionPanel {
            filter_query,
            submenu_parent,
            ..
        }) = &self.overlay
        else {
            return Vec::new();
        };

        let panel = self.current().active_action_panel();
        let source: Vec<ActionItem> = if let Some(parent_id) = submenu_parent {
            panel
                .flat_actions()
                .find(|a| &a.id == parent_id)
                .map(|a| a.submenu.clone())
                .unwrap_or_default()
        } else {
            panel.flat_actions().cloned().collect()
        };

        let q = filter_query.trim();
        if q.is_empty() {
            return source;
        }
        let engine = QueryParser::parse(q, CaseMatching::Smart);
        source
            .into_iter()
            .filter(|a| engine.match_text(&a.title).is_some())
            .collect()
    }

    /// Layered `Esc` behavior strictly matching `DESIGN.md`:
    /// ConfirmAlert -> ActionPanel submenu -> ActionPanel -> FilterDropdown -> clear query -> pop view -> hide window.
    pub fn handle_escape(&mut self) -> ShellEffect {
        if let Some(overlay) = self.overlay.clone() {
            match overlay {
                ActiveOverlay::ActionPanel {
                    submenu_parent: Some(_),
                    ..
                } => {
                    self.overlay = Some(ActiveOverlay::ActionPanel {
                        filter_query: String::new(),
                        selected_index: 0,
                        submenu_parent: None,
                    });
                    return ShellEffect::None;
                }
                ActiveOverlay::ActionPanel { .. }
                | ActiveOverlay::FilterDropdown { .. }
                | ActiveOverlay::ConfirmAlert(_) => {
                    self.overlay = None;
                    self.current_mut().header_focus = HeaderFocus::Search;
                    return ShellEffect::None;
                }
            }
        }

        if !self.current().search_query.is_empty() {
            let frame = self.current_mut();
            frame.search_query.clear();
            frame.selected_index = 0;
            return ShellEffect::None;
        }

        if self.pop() {
            return ShellEffect::None;
        }

        ShellEffect::HideWindow
    }

    /// Moves selection up or down in the active overlay, list, or grid.
    pub fn move_vertical(&mut self, delta: isize) {
        if let Some(ActiveOverlay::ActionPanel { selected_index, .. }) = &self.overlay {
            let count = self.visible_action_panel_items().len();
            if count > 0 {
                let next = wrap_index(*selected_index, delta, count);
                if let Some(ActiveOverlay::ActionPanel { selected_index, .. }) = &mut self.overlay {
                    *selected_index = next;
                }
            }
            return;
        }

        if let Some(ActiveOverlay::FilterDropdown { selected_index }) = &self.overlay {
            let count = self.current_filter_options_len();
            if count > 0 {
                let next = wrap_index(*selected_index, delta, count);
                if let Some(ActiveOverlay::FilterDropdown { selected_index }) = &mut self.overlay {
                    *selected_index = next;
                }
            }
            return;
        }

        match &self.current().view {
            ViewNode::List(_) => {
                let count = self.current().filtered_list_entries().len();
                if count > 0 {
                    let cur = self.current().selected_index;
                    self.current_mut().selected_index = wrap_index(cur, delta, count);
                }
            }
            ViewNode::Grid(grid) => {
                let count = self.current().filtered_grid_entries().len();
                if count > 0 {
                    let cols = usize::from(grid.columns.clamp(1, 8)) as isize;
                    let cur = self.current().selected_index as isize;
                    let next = (cur + delta * cols).clamp(0, (count - 1) as isize) as usize;
                    self.current_mut().selected_index = next;
                }
            }
            ViewNode::Form(_) => {
                self.cycle_form_field(delta > 0);
            }
            _ => {}
        }
    }

    /// Moves selection left or right (Grid tiles, ActionPanel submenu, Form dropdowns).
    pub fn move_horizontal(&mut self, delta: isize) {
        if let Some(ActiveOverlay::ActionPanel {
            selected_index,
            submenu_parent,
            ..
        }) = &self.overlay.clone()
        {
            if delta > 0 && submenu_parent.is_none() {
                let items = self.visible_action_panel_items();
                if let Some(item) = items.get(*selected_index)
                    && !item.submenu.is_empty()
                {
                    self.overlay = Some(ActiveOverlay::ActionPanel {
                        filter_query: String::new(),
                        selected_index: 0,
                        submenu_parent: Some(item.id.clone()),
                    });
                }
            } else if delta < 0 && submenu_parent.is_some() {
                self.overlay = Some(ActiveOverlay::ActionPanel {
                    filter_query: String::new(),
                    selected_index: 0,
                    submenu_parent: None,
                });
            }
            return;
        }

        match &self.current().view {
            ViewNode::Grid(_) => {
                let count = self.current().filtered_grid_entries().len();
                if count > 0 {
                    let cur = self.current().selected_index;
                    self.current_mut().selected_index = wrap_index(cur, delta, count);
                }
            }
            ViewNode::Form(_) => {
                self.cycle_focused_form_dropdown(delta);
            }
            _ => {}
        }
    }

    /// Handles `Tab` / `Shift+Tab` focus order across form fields or header inline arguments.
    pub fn handle_tab(&mut self, reverse: bool) {
        if self.overlay.is_some() {
            return;
        }

        if matches!(self.current().view, ViewNode::Form(_)) {
            self.cycle_form_field(!reverse);
            return;
        }

        let arg_len = self.current().inline_arg_values.len();
        let has_dropdown = self.current_filter_options_len() > 0;
        let mut stops = vec![HeaderFocus::Search];
        for i in 0..arg_len {
            stops.push(HeaderFocus::InlineArg(i));
        }
        if has_dropdown {
            stops.push(HeaderFocus::FilterDropdown);
        }
        if stops.len() <= 1 {
            return;
        }

        let cur = self.current().header_focus;
        let cur_pos = stops.iter().position(|s| *s == cur).unwrap_or(0);
        let next_pos = wrap_index(cur_pos, if reverse { -1 } else { 1 }, stops.len());
        self.current_mut().header_focus = stops[next_pos];
    }

    /// Inserts typed text into the currently focused control (ActionPanel filter, Form field,
    /// Inline Argument pill, or main Search Bar).
    pub fn insert_text(&mut self, text: &str) {
        if let Some(ActiveOverlay::ActionPanel {
            filter_query,
            selected_index,
            ..
        }) = &mut self.overlay
        {
            filter_query.push_str(text);
            *selected_index = 0;
            return;
        }
        if self.overlay.is_some() {
            return;
        }

        let frame = self.current_mut();
        match &mut frame.view {
            ViewNode::Form(form) => {
                if let Some(field) = form.fields.get_mut(frame.focused_form_field) {
                    match field {
                        FormField::TextField { value, error, .. }
                        | FormField::PasswordField { value, error, .. }
                        | FormField::TextArea { value, error, .. }
                        | FormField::DatePicker { value, error, .. } => {
                            value.push_str(text);
                            *error = None;
                        }
                        FormField::Checkbox { checked, .. } if text == " " => {
                            *checked = !*checked;
                        }
                        FormField::Dropdown {
                            options,
                            selected_id,
                            error,
                            ..
                        } if text == " " && !options.is_empty() => {
                            let cur = options
                                .iter()
                                .position(|o| o.id == *selected_id)
                                .unwrap_or(0);
                            *selected_id = options[(cur + 1) % options.len()].id.clone();
                            *error = None;
                        }
                        _ => {}
                    }
                }
            }
            ViewNode::List(_) | ViewNode::Grid(_) => match frame.header_focus {
                HeaderFocus::Search => {
                    frame.search_query.push_str(text);
                    frame.selected_index = 0;
                    frame.clamp_selection();
                }
                HeaderFocus::InlineArg(idx) => {
                    if let Some(slot) = frame.inline_arg_values.get_mut(idx) {
                        slot.push_str(text);
                    }
                }
                HeaderFocus::FilterDropdown => {
                    if text == " " {
                        self.toggle_filter_dropdown();
                    }
                }
            },
            _ => {}
        }
    }

    /// Handles `Backspace` in the currently focused input (or pops view when search is empty).
    pub fn handle_backspace(&mut self) {
        if let Some(ActiveOverlay::ActionPanel {
            filter_query,
            selected_index,
            submenu_parent,
        }) = &mut self.overlay
        {
            if !filter_query.is_empty() {
                filter_query.pop();
                *selected_index = 0;
            } else if submenu_parent.is_some() {
                *submenu_parent = None;
                *selected_index = 0;
            }
            return;
        }
        if self.overlay.is_some() {
            return;
        }

        let frame = self.current_mut();
        match &mut frame.view {
            ViewNode::Form(form) => {
                if let Some(
                    FormField::TextField { value, error, .. }
                    | FormField::PasswordField { value, error, .. }
                    | FormField::TextArea { value, error, .. }
                    | FormField::DatePicker { value, error, .. },
                ) = form.fields.get_mut(frame.focused_form_field)
                {
                    value.pop();
                    *error = None;
                }
            }
            ViewNode::List(_) | ViewNode::Grid(_) => match frame.header_focus {
                HeaderFocus::Search => {
                    if frame.search_query.is_empty() {
                        self.pop();
                    } else {
                        frame.search_query.pop();
                        frame.selected_index = 0;
                        frame.clamp_selection();
                    }
                }
                HeaderFocus::InlineArg(idx) => {
                    if let Some(slot) = frame.inline_arg_values.get_mut(idx) {
                        slot.pop();
                    }
                }
                HeaderFocus::FilterDropdown => {
                    frame.header_focus = HeaderFocus::Search;
                }
            },
            _ => {
                self.pop();
            }
        }
    }

    /// Selects a filter dropdown option by index and closes the dropdown overlay.
    pub fn apply_filter_option(&mut self, index: usize) {
        let frame = self.current_mut();
        match &mut frame.view {
            ViewNode::List(list) => {
                if let Some(dropdown) = &mut list.filter_dropdown
                    && let Some(opt) = dropdown.options.get(index)
                {
                    dropdown.selected_id = opt.id.clone();
                }
            }
            ViewNode::Grid(grid) => {
                if let Some(dropdown) = &mut grid.filter_dropdown
                    && let Some(opt) = dropdown.options.get(index)
                {
                    dropdown.selected_id = opt.id.clone();
                }
            }
            _ => {}
        }
        frame.selected_index = 0;
        frame.header_focus = HeaderFocus::Search;
        frame.clamp_selection();
        self.overlay = None;
    }

    /// Validates all required fields in the active `FormView`.
    /// Returns `Ok(summary)` if valid, or sets inline field errors and returns `Err(count)`.
    pub fn validate_active_form(&mut self) -> Result<Vec<(String, String)>, usize> {
        let ViewNode::Form(form) = &mut self.current_mut().view else {
            return Ok(Vec::new());
        };

        let mut invalid_count = 0usize;
        let mut values = Vec::new();
        let mut first_error_idx = None;

        for (idx, field) in form.fields.iter_mut().enumerate() {
            match field {
                FormField::TextField {
                    id,
                    label,
                    value,
                    required,
                    error,
                    ..
                }
                | FormField::PasswordField {
                    id,
                    label,
                    value,
                    required,
                    error,
                    ..
                }
                | FormField::TextArea {
                    id,
                    label,
                    value,
                    required,
                    error,
                    ..
                } => {
                    if *required && value.trim().is_empty() {
                        *error = Some(format!("{label} is required before saving."));
                        invalid_count += 1;
                        if first_error_idx.is_none() {
                            first_error_idx = Some(idx);
                        }
                    } else {
                        *error = None;
                        values.push((id.clone(), value.clone()));
                    }
                }
                FormField::Checkbox { id, checked, .. } => {
                    values.push((id.clone(), checked.to_string()));
                }
                FormField::Dropdown {
                    id, selected_id, ..
                } => {
                    values.push((id.clone(), selected_id.clone()));
                }
                FormField::DatePicker {
                    id, value, error, ..
                } => {
                    if !value.trim().is_empty() && value.trim().len() != 10 {
                        *error = Some("Use YYYY-MM-DD format (example: 2026-11-25).".to_string());
                        invalid_count += 1;
                        if first_error_idx.is_none() {
                            first_error_idx = Some(idx);
                        }
                    } else {
                        *error = None;
                        values.push((id.clone(), value.clone()));
                    }
                }
                FormField::Description { .. } | FormField::Separator => {}
            }
        }

        if let Some(err_idx) = first_error_idx {
            self.current_mut().focused_form_field = err_idx;
        }

        if invalid_count > 0 {
            Err(invalid_count)
        } else {
            Ok(values)
        }
    }

    fn current_filter_options_len(&self) -> usize {
        match &self.current().view {
            ViewNode::List(l) => l.filter_dropdown.as_ref().map_or(0, |d| d.options.len()),
            ViewNode::Grid(g) => g.filter_dropdown.as_ref().map_or(0, |d| d.options.len()),
            _ => 0,
        }
    }

    fn cycle_form_field(&mut self, forward: bool) {
        let indices: Vec<usize> = match &self.current().view {
            ViewNode::Form(form) => form
                .fields
                .iter()
                .enumerate()
                .filter_map(|(i, f)| is_interactive_form_field(f).then_some(i))
                .collect(),
            _ => Vec::new(),
        };
        if indices.is_empty() {
            return;
        }
        let cur = self.current().focused_form_field;
        let pos = indices.iter().position(|&i| i == cur).unwrap_or(0);
        let next = wrap_index(pos, if forward { 1 } else { -1 }, indices.len());
        self.current_mut().focused_form_field = indices[next];
    }

    fn cycle_focused_form_dropdown(&mut self, delta: isize) {
        let frame = self.current_mut();
        if let ViewNode::Form(form) = &mut frame.view
            && let Some(FormField::Dropdown {
                options,
                selected_id,
                error,
                ..
            }) = form.fields.get_mut(frame.focused_form_field)
            && !options.is_empty()
        {
            let cur = options
                .iter()
                .position(|o| o.id == *selected_id)
                .unwrap_or(0);
            let next = wrap_index(cur, delta, options.len());
            *selected_id = options[next].id.clone();
            *error = None;
        }
    }
}

fn is_interactive_form_field(field: &FormField) -> bool {
    !matches!(field, FormField::Description { .. } | FormField::Separator)
}

fn first_interactive_form_field(view: &ViewNode) -> Option<usize> {
    match view {
        ViewNode::Form(form) => form.fields.iter().position(is_interactive_form_field),
        _ => None,
    }
}

fn wrap_index(current: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let len_i = len as isize;
    (((current as isize + delta) % len_i) + len_i) as usize % len
}
