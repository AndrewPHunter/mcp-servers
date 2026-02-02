use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guideline {
    pub id: String,
    pub anchor: String,
    pub title: String,
    pub category: String,
    pub source_file: String,
    pub raw_markdown: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub key: String,
    pub display_name: String,
    pub guideline_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidelineResult {
    pub id: String,
    pub title: String,
    pub category: String,
    pub score: f32,
    pub summary: String,
}

