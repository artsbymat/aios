//! Shared declarative Multi-View AST used by built-in plugins, WASM plugins, and GPUI renderers.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommandMode {
    View,
    NoView,
    MenuBar,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FilterMode {
    #[default]
    Host,
    Plugin,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticTone {
    #[default]
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ViewNode {
    List(ListView),
    Grid(GridView),
    Detail(DetailView),
    Form(FormView),
    Empty(EmptyView),
    Error(ErrorView),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineArgumentSpec {
    pub id: String,
    pub placeholder: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterOption {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterDropdown {
    pub tooltip: String,
    pub selected_id: String,
    pub options: Vec<FilterOption>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListView {
    pub navigation_title: Option<String>,
    pub search_placeholder: Option<String>,
    pub filtering: FilterMode,
    pub is_loading: bool,
    pub loading_message: Option<String>,
    pub is_showing_detail: bool,
    pub filter_dropdown: Option<FilterDropdown>,
    pub inline_arguments: Vec<InlineArgumentSpec>,
    pub sections: Vec<ListSection>,
    pub empty_view: Option<EmptyView>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListSection {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub items: Vec<ListItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Accessory {
    Text { value: String },
    Tag { label: String, tone: SemanticTone },
    Date { value: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemDetail {
    pub markdown: String,
    pub metadata: Vec<MetadataEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListItem {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub icon: Option<String>,
    /// Optional resolved filesystem path to a PNG/SVG icon (e.g. XDG app icon).
    pub icon_path: Option<String>,
    pub alias: Option<String>,
    pub keywords: Vec<String>,
    pub filter_group: Option<String>,
    pub accessories: Vec<Accessory>,
    pub detail: Option<ItemDetail>,
    pub actions: ActionPanel,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GridAspectRatio {
    #[default]
    Square,
    Wide16x9,
    Portrait3x4,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum GridItemContent {
    Symbol { glyph: String },
    ColorSwatch { hex: String },
    Monogram { text: String, accent: bool },
}

impl Default for GridItemContent {
    fn default() -> Self {
        Self::Monogram {
            text: "AI".to_string(),
            accent: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridItem {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub content: GridItemContent,
    pub keywords: Vec<String>,
    pub filter_group: Option<String>,
    pub actions: ActionPanel,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridSection {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub items: Vec<GridItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridView {
    pub navigation_title: Option<String>,
    pub search_placeholder: Option<String>,
    pub filtering: FilterMode,
    pub is_loading: bool,
    pub loading_message: Option<String>,
    pub columns: u8,
    pub aspect_ratio: GridAspectRatio,
    pub filter_dropdown: Option<FilterDropdown>,
    pub sections: Vec<GridSection>,
    pub empty_view: Option<EmptyView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum MetadataEntry {
    Label {
        title: String,
        text: String,
    },
    Link {
        title: String,
        text: String,
        target: String,
    },
    TagList {
        title: String,
        tags: Vec<(String, SemanticTone)>,
    },
    Separator,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetailView {
    pub navigation_title: Option<String>,
    pub is_loading: bool,
    pub loading_message: Option<String>,
    pub markdown: String,
    pub metadata: Vec<MetadataEntry>,
    pub actions: ActionPanel,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormView {
    pub navigation_title: Option<String>,
    pub is_loading: bool,
    pub enable_drafts: bool,
    pub fields: Vec<FormField>,
    pub actions: ActionPanel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum FormField {
    TextField {
        id: String,
        label: String,
        placeholder: Option<String>,
        value: String,
        required: bool,
        error: Option<String>,
    },
    PasswordField {
        id: String,
        label: String,
        placeholder: Option<String>,
        value: String,
        required: bool,
        error: Option<String>,
    },
    TextArea {
        id: String,
        label: String,
        placeholder: Option<String>,
        value: String,
        required: bool,
        error: Option<String>,
    },
    Checkbox {
        id: String,
        label: String,
        title: String,
        checked: bool,
        error: Option<String>,
    },
    Dropdown {
        id: String,
        label: String,
        options: Vec<FilterOption>,
        selected_id: String,
        error: Option<String>,
    },
    DatePicker {
        id: String,
        label: String,
        value: String,
        error: Option<String>,
    },
    Description {
        title: String,
        text: String,
    },
    Separator,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmptyView {
    pub title: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub actions: ActionPanel,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorView {
    pub title: String,
    pub message: String,
    pub detail: Option<String>,
    pub actions: ActionPanel,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionSection {
    pub title: Option<String>,
    pub actions: Vec<ActionItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionPanel {
    pub sections: Vec<ActionSection>,
}

impl ActionPanel {
    #[must_use]
    pub fn from_actions(actions: Vec<ActionItem>) -> Self {
        if actions.is_empty() {
            Self {
                sections: Vec::new(),
            }
        } else {
            Self {
                sections: vec![ActionSection {
                    title: None,
                    actions,
                }],
            }
        }
    }

    pub fn flat_actions(&self) -> impl Iterator<Item = &ActionItem> {
        self.sections.iter().flat_map(|s| s.actions.iter())
    }

    #[must_use]
    pub fn primary_action(&self) -> Option<&ActionItem> {
        self.flat_actions().next()
    }

    #[must_use]
    pub fn secondary_action(&self) -> Option<&ActionItem> {
        self.flat_actions().nth(1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionItem {
    pub id: String,
    pub title: String,
    pub shortcut: Option<String>,
    pub destructive: bool,
    pub submenu: Vec<ActionItem>,
}

impl ActionItem {
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            shortcut: None,
            destructive: false,
            submenu: Vec::new(),
        }
    }

    #[must_use]
    pub fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    #[must_use]
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    #[must_use]
    pub fn with_submenu(mut self, submenu: Vec<ActionItem>) -> Self {
        self.submenu = submenu;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToastStyle {
    Animated,
    Success,
    Failure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToastMessage {
    pub style: ToastStyle,
    pub title: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HudMessage {
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmAlert {
    pub id: String,
    pub title: String,
    pub message: String,
    pub primary_action: String,
    pub dismiss_action: String,
    pub destructive: bool,
}
