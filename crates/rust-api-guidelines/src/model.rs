use serde::{Deserialize, Serialize};

/// A single Rust API guideline item (e.g., "C-CASE: Casing conforms to RFC 430").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guideline {
    /// Guideline identifier, e.g. "C-CASE", "C-CONV", "C-DEBUG"
    pub id: String,
    /// HTML anchor in source markdown, e.g. "c-case"
    pub anchor: String,
    /// Guideline title from the H2 heading
    pub title: String,
    /// Category from chapter title, e.g. "Naming", "Interoperability"
    pub category: String,
    /// Relative markdown file path, e.g. "src/naming.md"
    pub source_file: String,
    /// Full original markdown for this guideline
    pub raw_markdown: String,
}

/// A search result returned from vector similarity search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidelineResult {
    /// Guideline identifier
    pub id: String,
    /// Guideline title
    pub title: String,
    /// Category
    pub category: String,
    /// Similarity score (higher is better)
    pub score: f32,
    /// Summary text snippet
    pub summary: String,
}

/// A guideline category (chapter in the book).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    /// Category key and display value, e.g. "Naming"
    pub key: String,
    /// Number of guidelines in this category
    pub guideline_count: usize,
}
