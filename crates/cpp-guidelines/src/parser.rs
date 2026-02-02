/// Parser for CppCoreGuidelines.md.
///
/// The markdown has a deterministic structure:
/// - Category headers: `# <a name="..."></a>PREFIX: Category Name`
/// - Rule headers: `### <a name="ANCHOR"></a>RULE_ID: Title`
/// - Sub-sections within rules: `##### Heading`
/// - Rule ends at next `###`, `##`, or `#` header, or EOF
///
/// Parser approach: line-by-line state machine with regex for header detection.
use std::collections::HashMap;

use regex::Regex;
use tracing::warn;

use crate::model::{Category, Guideline, GuidelineSection};

/// Parse the CppCoreGuidelines.md content into a list of guidelines and a category map.
///
/// Returns `(guidelines, categories)` where:
/// - `guidelines`: all successfully parsed rules
/// - `categories`: map from category prefix to `Category`
///
/// Malformed rules are skipped with a warning log; the parser never panics.
pub fn parse_guidelines(content: &str) -> (Vec<Guideline>, HashMap<String, Category>) {
    let rule_header_re =
        Regex::new(r#"^### <a name="([^"]+)">\s*</a>\s*(.+)$"#).expect("valid regex");
    let category_header_re =
        Regex::new(r#"^# <a name="[^"]+">\s*</a>\s*(\S+):\s+(.+)$"#).expect("valid regex");
    let section_header_re = Regex::new(r"^##### (.+)$").expect("valid regex");
    let any_heading_re = Regex::new(r"^#{1,3} ").expect("valid regex");

    let lines: Vec<&str> = content.lines().collect();
    let mut guidelines: Vec<Guideline> = Vec::new();
    let mut category_names: HashMap<String, String> = HashMap::new();

    // First pass: extract category names from `# <a name=...` headers
    for line in &lines {
        if let Some(caps) = category_header_re.captures(line) {
            let prefix = caps[1].to_string();
            let name = caps[2].to_string();
            category_names.insert(prefix, name);
        }
    }

    // Second pass: parse rules
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];

        // Check for rule header: ### <a name="ANCHOR"></a>...
        if let Some(caps) = rule_header_re.captures(line) {
            let anchor = caps[1].to_string();
            let rest = caps[2].to_string();

            // Parse RULE_ID: Title from the rest
            // The rule_id is everything before the first `: `, title is after.
            // Some titles contain `:` so we only split on the first occurrence.
            let (rule_id, title) = match rest.find(": ") {
                Some(pos) => (rest[..pos].trim().to_string(), rest[pos + 2..].trim().to_string()),
                None => {
                    // Some rules may use just `:` without space, or have no title
                    match rest.find(':') {
                        Some(pos) => (
                            rest[..pos].trim().to_string(),
                            rest[pos + 1..].trim().to_string(),
                        ),
                        None => {
                            warn!(
                                line_number = i + 1,
                                content = line,
                                "rule header has no ':' separator, skipping"
                            );
                            i += 1;
                            continue;
                        }
                    }
                }
            };

            if rule_id.is_empty() {
                warn!(
                    line_number = i + 1,
                    content = line,
                    "empty rule ID, skipping"
                );
                i += 1;
                continue;
            }

            // Extract category prefix from rule_id.
            // For compound IDs like "SL.con.1", the category is "SL".
            // For simple IDs like "P.1", the category is "P".
            // For "In.0", the category is "In".
            let category = extract_category(&rule_id);

            // Collect all lines belonging to this rule (until next heading of level 1-3)
            let rule_start = i;
            i += 1;
            let mut sections: Vec<GuidelineSection> = Vec::new();
            let mut current_section_heading: Option<String> = None;
            let mut current_section_lines: Vec<&str> = Vec::new();

            while i < lines.len() {
                let current_line = lines[i];

                // Check if we've hit a new heading that ends this rule.
                // Rule ends at `###`, `##`, or `#` headers (but NOT `#####` which is a sub-section).
                if any_heading_re.is_match(current_line)
                    && !current_line.starts_with("##### ")
                    && !current_line.starts_with("###### ")
                {
                    break;
                }

                // Check for sub-section header: ##### Heading
                if let Some(caps) = section_header_re.captures(current_line) {
                    // Save the previous section if any
                    if let Some(heading) = current_section_heading.take() {
                        let content = join_section_lines(&current_section_lines);
                        if !content.is_empty() || !heading.is_empty() {
                            sections.push(GuidelineSection {
                                heading,
                                content,
                            });
                        }
                    }
                    current_section_heading = Some(caps[1].to_string());
                    current_section_lines.clear();
                } else {
                    current_section_lines.push(current_line);
                }

                i += 1;
            }

            // Save the last section
            if let Some(heading) = current_section_heading.take() {
                let content = join_section_lines(&current_section_lines);
                if !content.is_empty() || !heading.is_empty() {
                    sections.push(GuidelineSection {
                        heading,
                        content,
                    });
                }
            }

            // Build raw markdown from all lines of this rule
            let raw_markdown = lines[rule_start..i].join("\n");

            guidelines.push(Guideline {
                id: rule_id,
                anchor,
                title,
                category,
                sections,
                raw_markdown,
            });
        } else {
            i += 1;
        }
    }

    // Build category map
    let mut category_rule_counts: HashMap<String, usize> = HashMap::new();
    for g in &guidelines {
        *category_rule_counts.entry(g.category.clone()).or_insert(0) += 1;
    }

    let mut categories: HashMap<String, Category> = HashMap::new();
    for (prefix, count) in category_rule_counts {
        let name = category_names
            .get(&prefix)
            .cloned()
            .unwrap_or_else(|| prefix.clone());
        categories.insert(
            prefix.clone(),
            Category {
                prefix,
                name,
                rule_count: count,
            },
        );
    }

    (guidelines, categories)
}

/// Extract the top-level category prefix from a rule ID.
///
/// Examples:
/// - "P.1" → "P"
/// - "SL.con.1" → "SL"
/// - "ES.20" → "ES"
/// - "In.0" → "In"
/// - "C.20" → "C"
fn extract_category(rule_id: &str) -> String {
    // The category is everything before the first `.`
    match rule_id.find('.') {
        Some(pos) => rule_id[..pos].to_string(),
        None => rule_id.to_string(),
    }
}

/// Join section content lines, trimming leading/trailing blank lines.
fn join_section_lines(lines: &[&str]) -> String {
    let joined = lines.join("\n");
    let trimmed = joined.trim();
    trimmed.to_string()
}

/// Compose the embedding text for a guideline.
///
/// Concatenates the title, reason section, and first example section for
/// maximum semantic relevance. Truncated to a reasonable length.
pub fn compose_embedding_text(guideline: &Guideline) -> String {
    let mut parts = vec![guideline.title.clone()];

    // Add the Reason section if present
    for section in &guideline.sections {
        if section.heading == "Reason" {
            parts.push(section.content.clone());
            break;
        }
    }

    // Add the first Example section if present
    for section in &guideline.sections {
        if section.heading.starts_with("Example") {
            parts.push(section.content.clone());
            break;
        }
    }

    let text = parts.join(". ");

    // Truncate to ~2000 chars to keep embedding input reasonable
    if text.len() > 2000 {
        text[..2000].to_string()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_category() {
        assert_eq!(extract_category("P.1"), "P");
        assert_eq!(extract_category("SL.con.1"), "SL");
        assert_eq!(extract_category("ES.20"), "ES");
        assert_eq!(extract_category("In.0"), "In");
        assert_eq!(extract_category("C.20"), "C");
        assert_eq!(extract_category("NR.1"), "NR");
    }

    #[test]
    fn test_parse_single_rule() {
        let content = r#"# <a name="s-philosophy"></a>P: Philosophy

### <a name="rp-direct"></a>P.1: Express ideas directly in code

##### Reason

Compilers don't read comments.

##### Example

    class Date {};

##### Enforcement

Very hard in general.
"#;
        let (guidelines, categories) = parse_guidelines(content);
        assert_eq!(guidelines.len(), 1);

        let g = &guidelines[0];
        assert_eq!(g.id, "P.1");
        assert_eq!(g.anchor, "rp-direct");
        assert_eq!(g.title, "Express ideas directly in code");
        assert_eq!(g.category, "P");
        assert_eq!(g.sections.len(), 3);
        assert_eq!(g.sections[0].heading, "Reason");
        assert_eq!(g.sections[1].heading, "Example");
        assert_eq!(g.sections[2].heading, "Enforcement");

        assert_eq!(categories.len(), 1);
        let cat = &categories["P"];
        assert_eq!(cat.prefix, "P");
        assert_eq!(cat.name, "Philosophy");
        assert_eq!(cat.rule_count, 1);
    }

    #[test]
    fn test_parse_compound_id() {
        let content = r#"### <a name="rsl-arrays"></a>SL.con.1: Prefer using STL `array` or `vector` instead of a C array

##### Reason

C arrays are less safe.
"#;
        let (guidelines, _) = parse_guidelines(content);
        assert_eq!(guidelines.len(), 1);
        assert_eq!(guidelines[0].id, "SL.con.1");
        assert_eq!(guidelines[0].category, "SL");
    }

    #[test]
    fn test_parse_backtick_in_title() {
        let content =
            r#"### <a name="ri-global"></a>I.2: Avoid non-`const` global variables

##### Reason

Non-const global variables are bad.
"#;
        let (guidelines, _) = parse_guidelines(content);
        assert_eq!(guidelines.len(), 1);
        assert_eq!(guidelines[0].id, "I.2");
        assert_eq!(
            guidelines[0].title,
            "Avoid non-`const` global variables"
        );
    }

    #[test]
    fn test_compose_embedding_text() {
        let g = Guideline {
            id: "P.1".to_string(),
            anchor: "rp-direct".to_string(),
            title: "Express ideas directly in code".to_string(),
            category: "P".to_string(),
            sections: vec![
                GuidelineSection {
                    heading: "Reason".to_string(),
                    content: "Compilers don't read comments.".to_string(),
                },
                GuidelineSection {
                    heading: "Example".to_string(),
                    content: "class Date {};".to_string(),
                },
            ],
            raw_markdown: String::new(),
        };
        let text = compose_embedding_text(&g);
        assert!(text.starts_with("Express ideas directly in code"));
        assert!(text.contains("Compilers don't read comments."));
        assert!(text.contains("class Date {};"));
    }

    /// Integration test: parse the real CppCoreGuidelines.md and verify structure.
    ///
    /// This test requires the data file to exist at the expected path (set via env var
    /// or the default dev location).
    #[test]
    fn test_parse_real_guidelines() {
        let path = std::env::var("CPP_GUIDELINES_REPO_PATH")
            .unwrap_or_else(|_| "./data/cpp-guidelines".to_string());
        let file_path = std::path::Path::new(&path).join("CppCoreGuidelines.md");

        // Try the workspace-root relative path too
        let file_path = if file_path.exists() {
            file_path
        } else {
            std::path::PathBuf::from("../../data/cpp-guidelines/CppCoreGuidelines.md")
        };

        if !file_path.exists() {
            eprintln!(
                "skipping test_parse_real_guidelines: {} not found",
                file_path.display()
            );
            return;
        }

        let content = std::fs::read_to_string(&file_path).expect("read guidelines file");
        let (guidelines, categories) = parse_guidelines(&content);

        // Expect approximately 513 rules (exact count may vary with guideline updates)
        assert!(
            guidelines.len() > 400,
            "expected >400 guidelines, got {}",
            guidelines.len()
        );
        assert!(
            guidelines.len() < 600,
            "expected <600 guidelines, got {}",
            guidelines.len()
        );

        // Verify some known rules exist
        let ids: Vec<&str> = guidelines.iter().map(|g| g.id.as_str()).collect();
        assert!(ids.contains(&"P.1"), "expected P.1 to be parsed");
        assert!(ids.contains(&"ES.20"), "expected ES.20 to be parsed");
        assert!(ids.contains(&"SL.con.1"), "expected SL.con.1 to be parsed");
        assert!(ids.contains(&"R.1"), "expected R.1 to be parsed");
        assert!(ids.contains(&"In.0"), "expected In.0 to be parsed");

        // Verify P.1 has sections
        let p1 = guidelines.iter().find(|g| g.id == "P.1").unwrap();
        assert_eq!(p1.title, "Express ideas directly in code");
        assert_eq!(p1.category, "P");
        assert!(!p1.sections.is_empty(), "P.1 should have sections");
        assert!(
            p1.sections.iter().any(|s| s.heading == "Reason"),
            "P.1 should have a Reason section"
        );

        // Verify categories include expected prefixes
        assert!(categories.contains_key("P"), "expected P category");
        assert!(categories.contains_key("R"), "expected R category");
        assert!(categories.contains_key("ES"), "expected ES category");
        assert!(categories.contains_key("SL"), "expected SL category");

        eprintln!(
            "Parsed {} guidelines across {} categories",
            guidelines.len(),
            categories.len()
        );
    }
}
