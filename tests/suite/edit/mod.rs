pub mod model;
pub mod plan;

use labeldeck::labels::{Label, LabelColor};

pub fn label(name: &str) -> Label {
    Label {
        name: name.into(),
        color: LabelColor::parse("ededed").unwrap(),
        description: String::new(),
    }
}
