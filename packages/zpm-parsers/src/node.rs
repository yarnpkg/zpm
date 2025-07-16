use crate::Path;

#[derive(Debug, PartialEq, Eq)]
pub struct Node {
    pub offset: usize,
    pub size: usize,

    pub indent: usize,
    pub column: usize,
    pub lines: usize,
}

impl Node {
    pub fn replace_by(&self, input: &mut String, raw: &str) -> () {
        input.replace_range(self.offset..self.offset + self.size, raw);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Field {
    pub path: Path,
    pub node: Node,
}

pub type Document = Vec<Field>;
