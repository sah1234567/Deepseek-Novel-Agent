//! Render spec_template / path_template with loop cursor.

use crate::types::LoopCursor;
use std::collections::HashMap;

pub fn render_template(
    template: &str,
    cursor: &LoopCursor,
    extras: &HashMap<String, String>,
) -> String {
    let mut out = template.to_string();
    let chapter = cursor.chapter.to_string();
    let volume = cursor.volume.to_string();
    let round = cursor.round.to_string();
    let pad3 = format!("{:03}", cursor.chapter);
    let replacements = [
        ("{{cursor.chapter}}", chapter.as_str()),
        ("{{cursor.volume}}", volume.as_str()),
        ("{{cursor.round}}", round.as_str()),
        ("{{cursor.chapter | pad3}}", pad3.as_str()),
    ];
    for (k, v) in replacements {
        out = out.replace(k, v);
    }
    for (k, v) in extras {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_chapter_pad() {
        let c = LoopCursor {
            chapter: 7,
            volume: 1,
            fine_outline_through: 0,
            round: 7,
        };
        let s = render_template(
            "chapters/chapter-{{cursor.chapter | pad3}}.md",
            &c,
            &HashMap::new(),
        );
        assert_eq!(s, "chapters/chapter-007.md");
    }
}
