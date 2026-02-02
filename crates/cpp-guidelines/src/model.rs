use serde::{Deserialize, Serialize};

/// A single C++ Core Guideline rule (e.g., "P.1: Express ideas directly in code").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guideline {
    /// Rule identifier, e.g. "P.1", "SL.con.1", "ES.20"
    pub id: String,
    /// HTML anchor from the source markdown, e.g. "rp-direct"
    pub anchor: String,
    /// Rule title, e.g. "Express ideas directly in code"
    pub title: String,
    /// Category prefix, e.g. "P", "SL", "ES"
    pub category: String,
    /// Sub-sections within the rule (Reason, Example, Enforcement, etc.)
    pub sections: Vec<GuidelineSection>,
    /// Full original markdown text of the rule
    pub raw_markdown: String,
}

/// A sub-section within a guideline (e.g., "Reason", "Example", "Enforcement").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidelineSection {
    /// Section heading, e.g. "Reason", "Example, bad", "Enforcement"
    pub heading: String,
    /// Section content (markdown)
    pub content: String,
}

/// A search result returned from vector similarity search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidelineResult {
    /// Rule identifier
    pub id: String,
    /// Rule title
    pub title: String,
    /// Category prefix
    pub category: String,
    /// Similarity score (lower distance = more similar in LanceDB)
    pub score: f32,
    /// Summary text (first portion of the rule content)
    pub summary: String,
}

/// A guideline category (e.g., "P: Philosophy", "R: Resource management").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    /// Category prefix, e.g. "P", "SL", "ES"
    pub prefix: String,
    /// Category name, e.g. "Philosophy", "Resource management"
    pub name: String,
    /// Number of rules in this category
    pub rule_count: usize,
}
