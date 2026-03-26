use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub type EdgeIndex = usize;
pub type Timestamp = u64;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct FileId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum Language {
    Rust,
    Markdown,
    Json,
    Toml,
    Yaml,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self {
            start: start as u32,
            end: end as u32,
        }
    }

    pub fn line(line: usize) -> Self {
        let offset = line.saturating_sub(1);
        Self::new(offset, offset)
    }

    pub fn whole_file(byte_len: usize) -> Self {
        Self::new(0, byte_len)
    }

    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}
