use std::collections::HashMap;
use std::path::Path;

use regex::Regex;

use crate::error::AppError;
use crate::model::{Category, Guideline};

pub fn parse_guidelines_repo(
    repo_path: &Path,
) -> Result<(Vec<Guideline>, HashMap<String, Category>), AppError> {
    let readme =
        std::env::var("NODEJS_GUIDELINES_README").unwrap_or_else(|_| "README.md".to_string());
    let mut path = repo_path.join(&readme);
    if !path.exists() {
        let nested = repo_path.join("nodebestpractices").join(&readme);
        if nested.exists() {
            path = nested;
        }
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| AppError::Config(format!("failed to read {}: {e}", path.display())))?;
    Ok(parse_guidelines(&content, &readme))
}

pub fn parse_guidelines(
    content: &str,
    source_file: &str,
) -> (Vec<Guideline>, HashMap<String, Category>) {
    let category_re =
        Regex::new(r#"^#\s+`?(\d+)\.\s+(.+?)`?\s*$"#).expect("valid regex");
    let guideline_re =
        Regex::new(r#"^##\s+!\[✔\]\s+(\d+(?:\.\d+)+)\s+(.+?)\s*$"#).expect("valid regex");

    let mut guidelines = Vec::new();
    let mut categories: HashMap<String, Category> = HashMap::new();

    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    let mut current_category_key: Option<String> = None;
    let mut current_category_name: Option<String> = None;

    while i < lines.len() {
        let line = lines[i];

        if let Some(caps) = category_re.captures(line) {
            let key = caps[1].to_string();
            let name = caps[2].trim().to_string();
            current_category_key = Some(key.clone());
            current_category_name = Some(name.clone());
            categories.entry(key.clone()).or_insert(Category {
                key,
                display_name: name,
                guideline_count: 0,
            });
            i += 1;
            continue;
        }

        if let Some(caps) = guideline_re.captures(line) {
            let id = caps[1].trim().to_string();
            let title = caps[2].trim().to_string();

            let category = current_category_key
                .clone()
                .or_else(|| id.split('.').next().map(|s| s.to_string()))
                .unwrap_or_else(|| "unknown".to_string());

            if let Some(category_name) = current_category_name.as_ref() {
                categories.entry(category.clone()).or_insert(Category {
                    key: category.clone(),
                    display_name: category_name.clone(),
                    guideline_count: 0,
                });
            } else {
                categories.entry(category.clone()).or_insert(Category {
                    key: category.clone(),
                    display_name: category.clone(),
                    guideline_count: 0,
                });
            }

            let start = i;
            let mut end = i + 1;
            while end < lines.len() {
                if guideline_re.is_match(lines[end]) || category_re.is_match(lines[end]) {
                    break;
                }
                end += 1;
            }

            let raw_markdown = lines[start..end].join("\n").trim().to_string();
            let anchor = guideline_anchor(&id, &title);

            guidelines.push(Guideline {
                id,
                anchor,
                title,
                category: category.clone(),
                source_file: source_file.to_string(),
                raw_markdown,
            });

            if let Some(cat) = categories.get_mut(&category) {
                cat.guideline_count += 1;
            }

            i = end;
            continue;
        }

        i += 1;
    }

    guidelines.sort_by(|a, b| a.id.cmp(&b.id));
    (guidelines, categories)
}

pub fn compose_embedding_text(guideline: &Guideline) -> String {
    let text = format!(
        "{}: {}. Category: {}. {}",
        guideline.id, guideline.title, guideline.category, guideline.raw_markdown
    );
    if text.chars().count() > 3000 {
        text.chars().take(3000).collect()
    } else {
        text
    }
}

fn guideline_anchor(id: &str, title: &str) -> String {
    let id_flat: String = id.chars().filter(|c| c.is_ascii_digit()).collect();
    format!("-{}-{}", id_flat, slugify(title))
}

fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_dash = false;
    for ch in s.chars() {
        let lc = ch.to_ascii_lowercase();
        if lc.is_ascii_alphanumeric() {
            out.push(lc);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal() {
        let content = r#"# `1. Project Architecture Practices`

## ![✔] 1.1 Structure your solution by business components

TL;DR text.

## ![✔] 1.2 Layer your components

More text.
"#;

        let (guidelines, categories) = parse_guidelines(content, "README.md");
        assert_eq!(guidelines.len(), 2);
        assert!(categories.contains_key("1"));
        assert_eq!(guidelines[0].id, "1.1");
        assert_eq!(guidelines[0].category, "1");
        assert_eq!(guidelines[0].anchor, "-11-structure-your-solution-by-business-components");
    }

    #[test]
    fn parse_real_repo() {
        let path = std::env::var("NODEJS_GUIDELINES_REPO_PATH")
            .unwrap_or_else(|_| "./data/nodejs-guidelines".to_string());
        let repo_path = Path::new(&path);
        if !repo_path.exists() {
            eprintln!("skipping parse_real_repo: {} not found", repo_path.display());
            return;
        }
        let (guidelines, categories) = parse_guidelines_repo(repo_path).expect("parse should succeed");
        assert!(guidelines.len() > 50, "expected >50 guidelines");
        assert!(categories.len() >= 5, "expected multiple categories");
        assert!(guidelines.iter().any(|g| g.id == "1.1"));
    }
}
