use crate::Path;

#[derive(Debug, PartialEq, Eq)]
pub struct Span {
    pub offset: usize,
    pub size: usize,
}

impl Span {
    pub fn new(offset: usize, size: usize) -> Self {
        Self { offset, size }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Node {
    pub field_span: Span,
    pub value_span: Span,
    pub indent: usize,
}

impl Node {
    pub fn replace_by(&self, input: &mut String, raw: &str) -> () {
        input.replace_range(self.value_span.offset..self.value_span.offset + self.value_span.size, raw);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Field {
    pub path: Path,
    pub node: Node,
}

pub type Document = Vec<Field>;
