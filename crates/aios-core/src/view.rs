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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ViewNode {
    List(ListView),
    Grid(GridView),
    Detail(DetailView),
    Form(FormView),
    Empty(EmptyView),
    Error { title: String, message: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListView {
    pub navigation_title: Option<String>,
    pub search_placeholder: Option<String>,
    pub filtering: FilterMode,
    pub is_loading: bool,
    pub is_showing_detail: bool,
    pub sections: Vec<ListSection>,
    pub empty_view: Option<EmptyView>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListSection {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub items: Vec<ListItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListItem {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub icon: Option<String>,
    pub keywords: Vec<String>,
    pub detail_markdown: Option<String>,
    pub actions: ActionPanel,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridView {
    pub navigation_title: Option<String>,
    pub search_placeholder: Option<String>,
    pub filtering: FilterMode,
    pub is_loading: bool,
    pub columns: u8,
    pub empty_view: Option<EmptyView>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetailView {
    pub navigation_title: Option<String>,
    pub is_loading: bool,
    pub markdown: String,
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
        error: Option<String>,
    },
    PasswordField {
        id: String,
        label: String,
        value: String,
        error: Option<String>,
    },
    TextArea {
        id: String,
        label: String,
        placeholder: Option<String>,
        value: String,
        error: Option<String>,
    },
    Checkbox {
        id: String,
        label: String,
        title: String,
        checked: bool,
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
pub struct ActionPanel {
    pub actions: Vec<ActionItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionItem {
    pub id: String,
    pub title: String,
    pub shortcut: Option<String>,
    pub destructive: bool,
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
    pub title: String,
    pub message: String,
    pub primary_action: String,
    pub dismiss_action: String,
    pub destructive: bool,
}
