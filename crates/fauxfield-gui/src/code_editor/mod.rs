//! Reusable code editor (SPEC §20.18). Pure logic lives in the GPUI-free submodules; `entity`
//! is the GPUI component and `element` paints it.

pub mod actions;
pub mod buffer;
pub mod completion;
pub mod display_map;
pub mod element;
pub mod entity;
pub mod format;
pub mod highlight;
pub mod language_config;
pub mod ops;
pub mod settings;

pub use entity::{CodeEditor, EditorEvent, VarCheck};
pub use language_config::Language;
pub use settings::EditorSettings;

#[cfg(test)]
mod tests;
