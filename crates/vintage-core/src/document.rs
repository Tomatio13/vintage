//! Bounded, non-executing document presentation for the native file viewer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Heading(u8),
    Paragraph,
    List,
    Quote,
    Code,
    Rule,
}
#[derive(Clone, Debug)]
pub struct Block {
    pub kind: BlockKind,
    pub text: String,
}
#[derive(Clone, Debug)]
pub struct Document {
    pub lines: Vec<String>,
    pub blocks: Vec<Block>,
    pub shortened: bool,
}

pub fn prepare(text: &str) -> Document {
    let mut shortened = false;
    let mut lines = Vec::new();
    for (index, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        if index == 8000 {
            shortened = true;
            break;
        }
        let mut chars = line.chars();
        let mut line: String = chars.by_ref().take(4096).collect();
        if chars.next().is_some() {
            line.push('…');
            shortened = true;
        }
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    let mut blocks: Vec<Block> = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut paragraph_break = true;
    for line in &lines {
        if blocks.len() == 1000 {
            shortened = true;
            break;
        }
        let trimmed = line.trim();
        let marker = trimmed.chars().next().unwrap_or(' ');
        let marker_count = trimmed.chars().take_while(|c| *c == marker).count();
        if let Some((opening, length)) = fence {
            if marker == opening && marker_count >= length && trimmed.chars().all(|c| c == marker) {
                fence = None;
                paragraph_break = true;
            } else {
                blocks.push(Block {
                    kind: BlockKind::Code,
                    text: line.clone(),
                });
            }
            continue;
        }
        if matches!(marker, '`' | '~') && marker_count >= 3 {
            fence = Some((marker, marker_count));
            paragraph_break = true;
            continue;
        }
        if trimmed.is_empty() {
            paragraph_break = true;
            continue;
        }
        let (kind, text) = if marker == '#'
            && (1..=6).contains(&marker_count)
            && trimmed.as_bytes().get(marker_count) == Some(&b' ')
        {
            (
                BlockKind::Heading(marker_count as u8),
                trimmed[marker_count + 1..].trim().to_owned(),
            )
        } else if matches!(trimmed, "---" | "***" | "___") {
            (BlockKind::Rule, String::new())
        } else if let Some(text) = trimmed.strip_prefix("> ") {
            (BlockKind::Quote, text.to_owned())
        } else if let Some(text) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            (BlockKind::List, format!("• {text}"))
        } else {
            (BlockKind::Paragraph, trimmed.to_owned())
        };
        if kind == BlockKind::Paragraph && !paragraph_break {
            if let Some(last) = blocks
                .last_mut()
                .filter(|b| b.kind == BlockKind::Paragraph && b.text.len() + text.len() < 8192)
            {
                last.text.push(' ');
                last.text.push_str(&text);
                continue;
            }
        }
        paragraph_break = kind != BlockKind::Paragraph;
        blocks.push(Block { kind, text });
    }
    Document {
        lines,
        blocks,
        shortened,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn headings_lists_quotes_and_fences_preserve_source() {
        let text = "# Heading\n\nfirst\nsecond\n- item\n> quote\n````rust\n# code\n```\n````\n---";
        let doc = prepare(text);
        assert_eq!(
            doc.blocks
                .iter()
                .map(|b| b.kind.clone())
                .collect::<Vec<_>>(),
            vec![
                BlockKind::Heading(1),
                BlockKind::Paragraph,
                BlockKind::List,
                BlockKind::Quote,
                BlockKind::Code,
                BlockKind::Code,
                BlockKind::Rule
            ]
        );
        assert_eq!(doc.blocks[1].text, "first second");
        assert_eq!(doc.lines.join("\n"), text);
    }
    #[test]
    fn oversized_and_unicode_documents_are_bounded() {
        let doc = prepare(&"日".repeat(5000));
        assert!(doc.shortened);
        assert_eq!(doc.lines[0].chars().count(), 4097);
        let doc = prepare(&"line\n".repeat(9000));
        assert!(doc.shortened);
        assert_eq!(doc.lines.len(), 8000);
        assert_eq!(prepare("").lines, vec![String::new()]);
    }
}
