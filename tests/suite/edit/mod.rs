pub mod color;
pub mod command;
pub mod execute;
pub mod form;
pub mod model;
pub mod plan;
pub mod rendering;
pub mod ui;

use labeldeck::labels::{Label, LabelColor};

pub fn label(name: &str) -> Label {
    Label {
        name: name.into(),
        color: LabelColor::parse("ededed").unwrap(),
        description: String::new(),
    }
}
