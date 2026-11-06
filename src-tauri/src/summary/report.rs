// A finished summary as Markdown: what Copy puts on the clipboard and Save writes next
// to the original (readable as plain text too).

use serde_json::Value;

fn text(value: &Value) -> Option<&str> {
    value.as_str().map(str::trim).filter(|s| !s.is_empty())
}

fn items(value: &Value) -> Vec<&Value> {
    value.as_array().map(|a| a.iter().collect()).unwrap_or_default()
}

/// `name` is the original file's name; `summary` holds the fields the model wrote
/// (type, title, tldr, points, facts, actions, keywords), any of which may be missing;
/// `answers` the questions asked about the file, with their answers.
pub fn markdown(name: &str, summary: &Value, answers: &[(String, String)]) -> String {
    let mut out = String::new();
    let title = text(&summary["title"]).unwrap_or(name);
    out.push_str(&format!("# {title}\n\n"));
    match text(&summary["type"]) {
        Some(kind) => out.push_str(&format!("*{kind} · summary of {name}*\n\n")),
        None => out.push_str(&format!("*Summary of {name}*\n\n")),
    }
    if let Some(tldr) = text(&summary["tldr"]) {
        out.push_str(&format!("> {tldr}\n\n"));
    }

    let points: Vec<&str> = items(&summary["points"]).into_iter().filter_map(text).collect();
    if !points.is_empty() {
        out.push_str("## Key points\n\n");
        for point in points {
            out.push_str(&format!("- {point}\n"));
        }
        out.push('\n');
    }

    let facts: Vec<(&str, &str)> = items(&summary["facts"])
        .into_iter()
        .filter_map(|fact| Some((text(&fact["label"]).unwrap_or(""), text(&fact["value"])?)))
        .collect();
    if !facts.is_empty() {
        out.push_str("## Key figures\n\n");
        for (label, value) in facts {
            if label.is_empty() {
                out.push_str(&format!("- **{value}**\n"));
            } else {
                out.push_str(&format!("- {label}: **{value}**\n"));
            }
        }
        out.push('\n');
    }

    let actions: Vec<&str> = items(&summary["actions"]).into_iter().filter_map(text).collect();
    if !actions.is_empty() {
        out.push_str("## To do\n\n");
        for action in actions {
            out.push_str(&format!("- [ ] {action}\n"));
        }
        out.push('\n');
    }

    let keywords: Vec<&str> = items(&summary["keywords"]).into_iter().filter_map(text).collect();
    if !keywords.is_empty() {
        out.push_str(&format!("**Topics:** {}\n\n", keywords.join(" · ")));
    }

    let answered: Vec<_> = answers.iter().filter(|(_, answer)| !answer.trim().is_empty()).collect();
    if !answered.is_empty() {
        out.push_str("## Questions\n\n");
        for (question, answer) in answered {
            out.push_str(&format!("**{}**\n{}\n\n", question.trim(), answer.trim()));
        }
    }
    out.push_str("---\n*Summarized on this Mac by Apple Intelligence in flipfile.*\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn complete_summary() {
        let summary = json!({
            "type": "Meeting notes",
            "title": "Bandung office renovation on track",
            "tldr": "Handover stays on 15 December.",
            "points": ["Work is 40% done.", "  "],
            "facts": [{"label": "Budget", "value": "Rp 3,2 miliar"}, {"label": "", "value": "42 staff"}],
            "actions": ["Andi: revised budget by 11 November"],
            "keywords": ["renovation", "budget"]
        });
        let answers = [("When is the handover?".to_string(), "On 15 December 2026.".to_string())];
        let md = markdown("notulen.pdf", &summary, &answers);
        assert!(md.starts_with("# Bandung office renovation on track\n\n*Meeting notes · summary of notulen.pdf*\n\n> Handover stays on 15 December.\n"));
        assert!(md.contains("## Key points\n\n- Work is 40% done.\n\n"), "blank points are dropped: {md}");
        assert!(md.contains("- Budget: **Rp 3,2 miliar**\n- **42 staff**\n"));
        assert!(md.contains("- [ ] Andi: revised budget by 11 November\n"));
        assert!(md.contains("**Topics:** renovation · budget\n"));
        assert!(md.contains("## Questions\n\n**When is the handover?**\nOn 15 December 2026.\n"));
    }

    #[test]
    fn partial_summary_uses_the_file_name() {
        let md = markdown("scan.jpg", &json!({"points": ["Only this."]}), &[]);
        assert!(md.starts_with("# scan.jpg\n\n*Summary of scan.jpg*\n\n## Key points\n\n- Only this.\n"));
        assert!(!md.contains("## To do"));
    }
}
