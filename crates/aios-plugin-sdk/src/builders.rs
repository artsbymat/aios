//! Ergonomic fluent builders for constructing declarative AIOS multi-view ASTs in Pure Rust.

use aios_core::view::{
    Accessory, ActionItem, ActionPanel, ActionSection, DetailView, EmptyView, ErrorView,
    FilterDropdown, FilterMode, FilterOption, FormField, FormView, GridAspectRatio, GridItem,
    GridItemContent, GridSection, GridView, InlineArgumentSpec, ItemDetail, ListItem, ListSection,
    ListView, MetadataEntry, SemanticTone, ViewNode,
};

/// Fluent builder for [`ActionPanel`].
#[derive(Debug, Default, Clone)]
pub struct ActionPanelBuilder {
    sections: Vec<ActionSection>,
}

impl ActionPanelBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn section(mut self, title: Option<&str>, actions: Vec<ActionItem>) -> Self {
        if !actions.is_empty() {
            self.sections.push(ActionSection {
                title: title.map(ToOwned::to_owned),
                actions,
            });
        }
        self
    }

    #[must_use]
    pub fn action(mut self, item: ActionItem) -> Self {
        if let Some(last) = self.sections.last_mut()
            && last.title.is_none()
        {
            last.actions.push(item);
        } else {
            self.sections.push(ActionSection {
                title: None,
                actions: vec![item],
            });
        }
        self
    }

    #[must_use]
    pub fn build(self) -> ActionPanel {
        ActionPanel {
            sections: self.sections,
        }
    }
}

/// Fluent builder for [`ListItem`].
#[derive(Debug, Clone)]
pub struct ListItemBuilder {
    item: ListItem,
}

impl ListItemBuilder {
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            item: ListItem {
                id: id.into(),
                title: title.into(),
                ..ListItem::default()
            },
        }
    }

    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.item.subtitle = Some(subtitle.into());
        self
    }

    #[must_use]
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.item.icon = Some(icon.into());
        self
    }

    #[must_use]
    pub fn icon_path(mut self, path: impl Into<String>) -> Self {
        self.item.icon_path = Some(path.into());
        self
    }

    #[must_use]
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.item.alias = Some(alias.into());
        self
    }

    #[must_use]
    pub fn keyword(mut self, keyword: impl Into<String>) -> Self {
        self.item.keywords.push(keyword.into());
        self
    }

    #[must_use]
    pub fn filter_group(mut self, group: impl Into<String>) -> Self {
        self.item.filter_group = Some(group.into());
        self
    }

    #[must_use]
    pub fn accessory_text(mut self, value: impl Into<String>) -> Self {
        self.item.accessories.push(Accessory::Text {
            value: value.into(),
        });
        self
    }

    #[must_use]
    pub fn accessory_tag(mut self, label: impl Into<String>, tone: SemanticTone) -> Self {
        self.item.accessories.push(Accessory::Tag {
            label: label.into(),
            tone,
        });
        self
    }

    #[must_use]
    pub fn detail(mut self, markdown: impl Into<String>, metadata: Vec<MetadataEntry>) -> Self {
        self.item.detail = Some(ItemDetail {
            markdown: markdown.into(),
            metadata,
        });
        self
    }

    #[must_use]
    pub fn actions(mut self, panel: ActionPanel) -> Self {
        self.item.actions = panel;
        self
    }

    #[must_use]
    pub fn build(self) -> ListItem {
        self.item
    }
}

/// Fluent builder for [`ListView`].
#[derive(Debug, Default, Clone)]
pub struct ListViewBuilder {
    view: ListView,
}

impl ListViewBuilder {
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            view: ListView {
                navigation_title: Some(title.into()),
                ..ListView::default()
            },
        }
    }

    #[must_use]
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.view.search_placeholder = Some(text.into());
        self
    }

    #[must_use]
    pub fn filtering(mut self, mode: FilterMode) -> Self {
        self.view.filtering = mode;
        self
    }

    #[must_use]
    pub fn showing_detail(mut self, enabled: bool) -> Self {
        self.view.is_showing_detail = enabled;
        self
    }

    #[must_use]
    pub fn loading(mut self, is_loading: bool, message: Option<String>) -> Self {
        self.view.is_loading = is_loading;
        self.view.loading_message = message;
        self
    }

    #[must_use]
    pub fn filter_dropdown(
        mut self,
        tooltip: impl Into<String>,
        selected_id: impl Into<String>,
        options: Vec<FilterOption>,
    ) -> Self {
        self.view.filter_dropdown = Some(FilterDropdown {
            tooltip: tooltip.into(),
            selected_id: selected_id.into(),
            options,
        });
        self
    }

    #[must_use]
    pub fn inline_argument(
        mut self,
        id: impl Into<String>,
        placeholder: impl Into<String>,
        required: bool,
    ) -> Self {
        self.view.inline_arguments.push(InlineArgumentSpec {
            id: id.into(),
            placeholder: placeholder.into(),
            required,
        });
        self
    }

    #[must_use]
    pub fn section(
        mut self,
        title: Option<&str>,
        subtitle: Option<&str>,
        items: Vec<ListItem>,
    ) -> Self {
        self.view.sections.push(ListSection {
            title: title.map(ToOwned::to_owned),
            subtitle: subtitle.map(ToOwned::to_owned),
            items,
        });
        self
    }

    #[must_use]
    pub fn empty_view(mut self, empty: EmptyView) -> Self {
        self.view.empty_view = Some(empty);
        self
    }

    #[must_use]
    pub fn build(self) -> ViewNode {
        ViewNode::List(self.view)
    }
}

/// Fluent builder for [`GridItem`].
#[derive(Debug, Clone)]
pub struct GridItemBuilder {
    item: GridItem,
}

impl GridItemBuilder {
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>, content: GridItemContent) -> Self {
        Self {
            item: GridItem {
                id: id.into(),
                title: title.into(),
                content,
                ..GridItem::default()
            },
        }
    }

    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.item.subtitle = Some(subtitle.into());
        self
    }

    #[must_use]
    pub fn keyword(mut self, keyword: impl Into<String>) -> Self {
        self.item.keywords.push(keyword.into());
        self
    }

    #[must_use]
    pub fn filter_group(mut self, group: impl Into<String>) -> Self {
        self.item.filter_group = Some(group.into());
        self
    }

    #[must_use]
    pub fn actions(mut self, panel: ActionPanel) -> Self {
        self.item.actions = panel;
        self
    }

    #[must_use]
    pub fn build(self) -> GridItem {
        self.item
    }
}

/// Fluent builder for [`GridView`].
#[derive(Debug, Clone)]
pub struct GridViewBuilder {
    view: GridView,
}

impl GridViewBuilder {
    #[must_use]
    pub fn new(title: impl Into<String>, columns: u8) -> Self {
        Self {
            view: GridView {
                navigation_title: Some(title.into()),
                columns: columns.clamp(1, 8),
                ..GridView::default()
            },
        }
    }

    #[must_use]
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.view.search_placeholder = Some(text.into());
        self
    }

    #[must_use]
    pub fn aspect_ratio(mut self, ratio: GridAspectRatio) -> Self {
        self.view.aspect_ratio = ratio;
        self
    }

    #[must_use]
    pub fn section(
        mut self,
        title: Option<&str>,
        subtitle: Option<&str>,
        items: Vec<GridItem>,
    ) -> Self {
        self.view.sections.push(GridSection {
            title: title.map(ToOwned::to_owned),
            subtitle: subtitle.map(ToOwned::to_owned),
            items,
        });
        self
    }

    #[must_use]
    pub fn empty_view(mut self, empty: EmptyView) -> Self {
        self.view.empty_view = Some(empty);
        self
    }

    #[must_use]
    pub fn build(self) -> ViewNode {
        ViewNode::Grid(self.view)
    }
}

/// Fluent builder for [`DetailView`].
#[derive(Debug, Default, Clone)]
pub struct DetailViewBuilder {
    view: DetailView,
}

impl DetailViewBuilder {
    #[must_use]
    pub fn new(title: impl Into<String>, markdown: impl Into<String>) -> Self {
        Self {
            view: DetailView {
                navigation_title: Some(title.into()),
                markdown: markdown.into(),
                ..DetailView::default()
            },
        }
    }

    #[must_use]
    pub fn metadata(mut self, entries: Vec<MetadataEntry>) -> Self {
        self.view.metadata = entries;
        self
    }

    #[must_use]
    pub fn actions(mut self, panel: ActionPanel) -> Self {
        self.view.actions = panel;
        self
    }

    #[must_use]
    pub fn build(self) -> ViewNode {
        ViewNode::Detail(self.view)
    }
}

/// Fluent builder for [`FormView`].
#[derive(Debug, Default, Clone)]
pub struct FormViewBuilder {
    view: FormView,
}

impl FormViewBuilder {
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            view: FormView {
                navigation_title: Some(title.into()),
                ..FormView::default()
            },
        }
    }

    #[must_use]
    pub fn enable_drafts(mut self, enable: bool) -> Self {
        self.view.enable_drafts = enable;
        self
    }

    #[must_use]
    pub fn field(mut self, field: FormField) -> Self {
        self.view.fields.push(field);
        self
    }

    #[must_use]
    pub fn actions(mut self, panel: ActionPanel) -> Self {
        self.view.actions = panel;
        self
    }

    #[must_use]
    pub fn build(self) -> ViewNode {
        ViewNode::Form(self.view)
    }
}

/// Helper to construct a standard isolated [`ViewNode::Error`] view.
#[must_use]
pub fn error_view(
    title: impl Into<String>,
    message: impl Into<String>,
    detail: Option<String>,
    actions: ActionPanel,
) -> ViewNode {
    ViewNode::Error(ErrorView {
        title: title.into(),
        message: message.into(),
        detail,
        actions,
    })
}
