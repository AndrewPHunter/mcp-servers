use std::collections::HashMap;
use std::path::Path;

use regex::Regex;

use crate::error::AppError;
use crate::model::{Category, Guideline};

const CATEGORY_FILES: &[&str] = &[
    "src/naming.md",
    "src/interoperability.md",
    "src/macros.md",
    "src/documentation.md",
    "src/predictability.md",
    "src/flexibility.md",
    "src/type-safety.md",
    "src/dependability.md",
    "src/debuggability.md",
    "src/future-proofing.md",
    "src/necessities.md",
];

pub fn parse_guidelines_repo(
    repo_path: &Path,
) -> Result<(Vec<Guideline>, HashMap<String, Category>), AppError> {
    let mut guidelines = Vec::new();
    let mut category_map: HashMap<String, Category> = HashMap::new();

    for rel_path in CATEGORY_FILES {
        let path = repo_path.join(rel_path);
        let content = std::fs::read_to_string(&path).map_err(|e| {
            AppError::Config(format!("failed to read {}: {e}", path.display()))
        })?;

        let (category_name, mut chapter_guidelines) =
            parse_category_file(&content, rel_path).map_err(|e| {
                AppError::Parse {
                    line: e.line,
                    message: format!("{} in {}", e.message, rel_path),
                }
            })?;

        let count = chapter_guidelines.len();
        category_map.insert(
            category_name.clone(),
            Category {
                key: category_name,
                guideline_count: count,
            },
        );
        guidelines.append(&mut chapter_guidelines);
    }

    guidelines.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((guidelines, category_map))
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

#[derive(Debug)]
struct ParseError {
    line: usize,
    message: String,
}

fn parse_category_file(content: &str, source_file: &str) -> Result<(String, Vec<Guideline>), ParseError> {
    let heading_re = Regex::new(r"^##\s+(.+?)\s+\((C-[A-Z0-9-]+)\)\s*$").expect("valid regex");
    let anchor_re = Regex::new(r#"^<a id="([^"]+)"></a>\s*$"#).expect("valid regex");

    let lines: Vec<&str> = content.lines().collect();
    let category = lines
        .iter()
        .find_map(|line| line.strip_prefix("# "))
        .map(|s| s.trim().to_string())
        .ok_or_else(|| ParseError {
            line: 1,
            message: "missing category heading".to_string(),
        })?;

    let mut guidelines = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let mut anchor: Option<String> = None;
        let header_idx;

        if let Some(caps) = anchor_re.captures(lines[i]) {
            anchor = Some(caps[1].to_string());
            if i + 1 < lines.len() && heading_re.is_match(lines[i + 1]) {
                header_idx = i + 1;
            } else {
                i += 1;
                continue;
            }
        } else if heading_re.is_match(lines[i]) {
            header_idx = i;
        } else {
            i += 1;
            continue;
        }

        let caps = heading_re.captures(lines[header_idx]).ok_or_else(|| ParseError {
            line: header_idx + 1,
            message: "invalid guideline heading".to_string(),
        })?;
        let title = caps[1].trim().to_string();
        let id = caps[2].trim().to_string();
        let anchor = anchor.unwrap_or_else(|| id.to_lowercase());

        let start = if header_idx > 0 && anchor_re.is_match(lines[header_idx - 1]) {
            header_idx - 1
        } else {
            header_idx
        };

        let mut end = header_idx + 1;
        while end < lines.len() {
            if heading_re.is_match(lines[end]) {
                break;
            }
            if anchor_re.is_match(lines[end])
                && end + 1 < lines.len()
                && heading_re.is_match(lines[end + 1])
            {
                break;
            }
            end += 1;
        }

        let raw_markdown = lines[start..end].join("\n").trim().to_string();
        guidelines.push(Guideline {
            id,
            anchor,
            title,
            category: category.clone(),
            source_file: source_file.to_string(),
            raw_markdown,
        });

        i = end;
    }

    Ok((category, guidelines))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_category_file() {
        let content = r#"# Naming

<a id="c-case"></a>
## Casing conforms to RFC 430 (C-CASE)

Use Rust conventions.

<a id="c-conv"></a>
## Ad-hoc conversions follow conventions (C-CONV)

Use as_/to_/into_.
"#;

        let (category, guidelines) = parse_category_file(content, "src/naming.md").unwrap();
        assert_eq!(category, "Naming");
        assert_eq!(guidelines.len(), 2);
        assert_eq!(guidelines[0].id, "C-CASE");
        assert_eq!(guidelines[0].anchor, "c-case");
        assert_eq!(guidelines[1].id, "C-CONV");
    }

    #[test]
    fn parse_real_repo() {
        let path = std::env::var("RUST_API_GUIDELINES_REPO_PATH")
            .unwrap_or_else(|_| "./data/rust-api-guidelines".to_string());
        let repo_path = Path::new(&path);

        if !repo_path.exists() {
            eprintln!("skipping parse_real_repo: {} not found", repo_path.display());
            return;
        }

        let (guidelines, categories) = parse_guidelines_repo(repo_path).expect("parse should succeed");

        assert!(guidelines.len() > 30, "expected >30 guidelines");
        assert!(categories.len() >= 10, "expected >=10 categories");
        assert!(guidelines.iter().any(|g| g.id == "C-CASE"));
        assert!(guidelines.iter().any(|g| g.id == "C-DEBUG"));
        assert!(categories.contains_key("Naming"));
        assert!(categories.contains_key("Documentation"));
    }
}
