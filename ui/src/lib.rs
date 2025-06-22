pub use bb::Annotation;
use std::path::PathBuf;

pub struct Line {
    pub segments: Vec<Segment>,
}

pub enum Segment {
    Text(String),
    Inline(Block),
}

pub struct Block {
    pub annotations: Vec<Annotation>,
    pub entities: Vec<Entity>,
}

pub enum Entity {
    Line(Line),
    Block(Block),
    Image(PathBuf),
    Pdf(PathBuf),
}
