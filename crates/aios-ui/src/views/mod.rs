//! All 4 interactive declarative GPUI view renderers:
//! - `list`: `List` + split `List.Item.Detail`
//! - `grid`: multi-column aspect-locked `Grid`
//! - `detail`: Markdown + `240px` right `Metadata` sidebar
//! - `form`: single-column `Form` with persistent labels & inline validation

pub mod detail;
pub mod form;
pub mod grid;
pub mod list;
