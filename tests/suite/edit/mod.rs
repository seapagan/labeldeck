pub mod color;
pub mod command;
pub mod execute;
pub mod form;
pub mod input;
pub mod model;
pub mod plan;
pub mod polish;
pub mod rendering;
pub mod rendering_cleanup;
pub mod ui;

use labeldeck::labels::{Label, LabelColor};

pub fn label(name: &str) -> Label {
    Label {
        name: name.into(),
        color: LabelColor::parse("ededed").unwrap(),
        description: String::new(),
    }
}

pub mod export_interactive;
pub mod interactive;
pub mod reconcile_interactive;
pub mod terminal_interactive;
