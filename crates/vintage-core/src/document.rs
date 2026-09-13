//! Bounded, non-executing document presentation for the native file viewer.
//! Markdown parsing never resolves paths or touches the filesystem; image
//! targets stay as opaque strings until the viewer resolves them through the
//! registered workspace file service.
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::collections::BTreeSet;

/// Maximum number of top-level blocks kept for display.
const MAX_BLOCKS: usize = 1000;

/// One styled run of inline text; adjacent runs with equal styling merge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub link: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    Text(Span),
    Image { target: String, alt: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellAlign {
    Default,
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug)]
pub struct ListItem {
    /// Task-list state; `None` for regular bullet items.
    pub checked: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug)]
pub enum Block {
    Heading {
        level: u8,
        inlines: Vec<Inline>,
    },
    Paragraph {
        inlines: Vec<Inline>,
    },
    Code {
        text: String,
        language: Option<String>,
    },
    Quote {
        blocks: Vec<Block>,
    },
    List {
        ordered: bool,
        start: u64,
        items: Vec<ListItem>,
    },
    Table {
        aligns: Vec<CellAlign>,
        head: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    Rule,
}

#[derive(Clone, Debug)]
pub struct Document {
    pub lines: Vec<String>,
    pub blocks: Vec<Block>,
    pub shortened: bool,
}

/// Active inline formatting while events stream in.
#[derive(Clone)]
struct Active {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    link: Option<String>,
}
impl Active {
    fn current(stack: &[Active]) -> Self {
        stack.last().cloned().unwrap_or(Self::plain())
    }
    fn plain() -> Self {
        Self {
            bold: false,
            italic: false,
            strike: false,
            code: false,
            link: None,
        }
    }
}

/// One open container while events stream in.
enum Frame {
    Quote(Vec<Block>),
    List {
        ordered: bool,
        start: u64,
        items: Vec<ListItem>,
    },
    Item {
        checked: Option<bool>,
        /// Inline runs of a tight item, which arrive without paragraph tags
        /// and flush into a paragraph before the item's next block.
        tight: Vec<Inline>,
        blocks: Vec<Block>,
    },
    Inline {
        heading: Option<u8>,
        inlines: Vec<Inline>,
    },
    Cell(Vec<Inline>),
    Row {
        cells: Vec<Vec<Inline>>,
    },
    Table {
        aligns: Vec<CellAlign>,
        head: Option<Vec<Vec<Inline>>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    Code {
        language: Option<String>,
        text: String,
    },
    /// Image alt text accumulates between the image tags.
    Image {
        target: String,
        alt: String,
    },
}

fn span(text: String, active: &Active) -> Inline {
    Inline::Text(Span {
        text,
        bold: active.bold,
        italic: active.italic,
        strike: active.strike,
        code: active.code,
        link: active.link.clone(),
    })
}

/// Append an inline run to the open inline container, merging adjacent runs
/// with equal styling. Tight list items collect runs without paragraph tags.
fn push_inline(frame: &mut Frame, inline: Inline) {
    let inlines = match frame {
        Frame::Inline { inlines, .. } | Frame::Cell(inlines) => inlines,
        Frame::Item { tight, .. } => tight,
        _ => return,
    };
    if let (Some(Inline::Text(last)), Inline::Text(next)) = (inlines.last_mut(), &inline) {
        if last.bold == next.bold
            && last.italic == next.italic
            && last.strike == next.strike
            && last.code == next.code
            && last.link == next.link
        {
            last.text.push_str(&next.text);
            return;
        }
    }
    inlines.push(inline);
}

/// Append a block to the innermost block container, else to the document.
fn push_block(stack: &mut [Frame], blocks: &mut Vec<Block>, block: Block) {
    match stack.last_mut() {
        Some(Frame::Quote(children)) => children.push(block),
        Some(Frame::Item {
            tight,
            blocks: children,
            ..
        }) => {
            flush_tight(tight, children);
            children.push(block);
        }
        _ => blocks.push(block),
    }
}

/// Move pending tight-list inline runs into a paragraph block.
fn flush_tight(tight: &mut Vec<Inline>, blocks: &mut Vec<Block>) {
    if !tight.is_empty() {
        blocks.push(Block::Paragraph {
            inlines: std::mem::take(tight),
        });
    }
}

/// Lift `<img src alt>` targets out of ignored HTML. Attribute values are
/// taken verbatim; unsafe targets are rejected later by the image resolver.
fn html_images(html: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for piece in html.split("<img").skip(1) {
        let tag = &piece[..piece.find('>').unwrap_or(piece.len())];
        let Some(target) = html_attr(tag, "src") else {
            continue;
        };
        out.push((target, html_attr(tag, "alt").unwrap_or_default()));
    }
    out
}

/// Read one double- or single-quoted attribute from an HTML tag body.
fn html_attr(tag: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let needle = format!("{name}={quote}");
        if let Some(start) = tag.find(&needle) {
            let rest = &tag[start + needle.len()..];
            let end = rest.find(quote)?;
            return Some(rest[..end].to_string());
        }
    }
    None
}

/// Prepare bounded display data. The markdown IR is parsed only when
/// `markdown` is true; plain text previews rely on `lines` alone.
pub fn prepare(text: &str, markdown: bool) -> Document {
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
    if !markdown {
        return Document {
            lines,
            blocks: Vec::new(),
            shortened,
        };
    }

    let mut blocks = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    let mut styles: Vec<Active> = Vec::new();
    let parser = Parser::new_ext(
        text.trim_start_matches('\u{feff}'),
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH,
    );
    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => stack.push(Frame::Inline {
                    heading: None,
                    inlines: Vec::new(),
                }),
                Tag::Heading { level, .. } => stack.push(Frame::Inline {
                    heading: Some(level as u8),
                    inlines: Vec::new(),
                }),
                Tag::BlockQuote(_) => stack.push(Frame::Quote(Vec::new())),
                Tag::CodeBlock(kind) => stack.push(Frame::Code {
                    language: match kind {
                        CodeBlockKind::Fenced(info) => info
                            .split([',', ' '])
                            .next()
                            .filter(|word| !word.is_empty())
                            .map(str::to_owned),
                        CodeBlockKind::Indented => None,
                    },
                    text: String::new(),
                }),
                Tag::List(start) => stack.push(Frame::List {
                    ordered: start.is_some(),
                    start: start.unwrap_or(1),
                    items: Vec::new(),
                }),
                Tag::Item => stack.push(Frame::Item {
                    checked: None,
                    tight: Vec::new(),
                    blocks: Vec::new(),
                }),
                Tag::Table(aligns) => stack.push(Frame::Table {
                    aligns: aligns
                        .into_iter()
                        .map(|align| match align {
                            pulldown_cmark::Alignment::Left => CellAlign::Left,
                            pulldown_cmark::Alignment::Center => CellAlign::Center,
                            pulldown_cmark::Alignment::Right => CellAlign::Right,
                            pulldown_cmark::Alignment::None => CellAlign::Default,
                        })
                        .collect(),
                    head: None,
                    rows: Vec::new(),
                }),
                Tag::TableHead => stack.push(Frame::Row { cells: Vec::new() }),
                Tag::TableRow => stack.push(Frame::Row { cells: Vec::new() }),
                Tag::TableCell => stack.push(Frame::Cell(Vec::new())),
                Tag::Emphasis => styles.push(Active {
                    italic: true,
                    ..Active::current(&styles)
                }),
                Tag::Strong => styles.push(Active {
                    bold: true,
                    ..Active::current(&styles)
                }),
                Tag::Strikethrough => styles.push(Active {
                    strike: true,
                    ..Active::current(&styles)
                }),
                Tag::Link { dest_url, .. } => styles.push(Active {
                    link: Some(dest_url.to_string()),
                    ..Active::current(&styles)
                }),
                // The alt text arrives as Text events between the image tags.
                Tag::Image { dest_url, .. } => stack.push(Frame::Image {
                    target: dest_url.to_string(),
                    alt: String::new(),
                }),
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Paragraph | TagEnd::Heading(_) => {
                    let Some(Frame::Inline { heading, inlines }) = stack.pop() else {
                        continue;
                    };
                    if inlines.is_empty() {
                        continue;
                    }
                    let block = match heading {
                        Some(level) => Block::Heading { level, inlines },
                        None => Block::Paragraph { inlines },
                    };
                    push_block(&mut stack, &mut blocks, block);
                }
                TagEnd::BlockQuote(_) => {
                    if let Some(Frame::Quote(children)) = stack.pop() {
                        push_block(&mut stack, &mut blocks, Block::Quote { blocks: children });
                    }
                }
                TagEnd::CodeBlock => {
                    if let Some(Frame::Code { language, text }) = stack.pop() {
                        push_block(&mut stack, &mut blocks, Block::Code { text, language });
                    }
                }
                TagEnd::List(_) => {
                    if let Some(Frame::List {
                        ordered,
                        start,
                        items,
                    }) = stack.pop()
                    {
                        if !items.is_empty() {
                            push_block(
                                &mut stack,
                                &mut blocks,
                                Block::List {
                                    ordered,
                                    start,
                                    items,
                                },
                            );
                        }
                    }
                }
                TagEnd::Item => {
                    if let Some(Frame::Item {
                        checked,
                        mut tight,
                        blocks: item_blocks,
                    }) = stack.pop()
                    {
                        let mut blocks = item_blocks;
                        flush_tight(&mut tight, &mut blocks);
                        if let Some(Frame::List { items, .. }) = stack.last_mut() {
                            items.push(ListItem { checked, blocks });
                        }
                    }
                }
                TagEnd::Table => {
                    if let Some(Frame::Table { aligns, head, rows }) = stack.pop() {
                        push_block(
                            &mut stack,
                            &mut blocks,
                            Block::Table {
                                aligns,
                                head: head.unwrap_or_default(),
                                rows,
                            },
                        );
                    }
                }
                TagEnd::TableHead => {
                    if let Some(Frame::Row { cells, .. }) = stack.pop() {
                        if let Some(Frame::Table { head, .. }) = stack.last_mut() {
                            *head = Some(cells);
                        }
                    }
                }
                TagEnd::TableRow => {
                    if let Some(Frame::Row { cells, .. }) = stack.pop() {
                        if let Some(Frame::Table { rows, .. }) = stack.last_mut() {
                            rows.push(cells);
                        }
                    }
                }
                TagEnd::TableCell => {
                    if let Some(Frame::Cell(inlines)) = stack.pop() {
                        if let Some(Frame::Row { cells, .. }) = stack.last_mut() {
                            cells.push(inlines);
                        }
                    }
                }
                TagEnd::Image => {
                    if let Some(Frame::Image { target, alt }) = stack.pop() {
                        let inline = Inline::Image { target, alt };
                        match stack.last_mut() {
                            Some(frame @ (Frame::Inline { .. } | Frame::Cell(_))) => {
                                push_inline(frame, inline)
                            }
                            _ => push_block(
                                &mut stack,
                                &mut blocks,
                                Block::Paragraph {
                                    inlines: vec![inline],
                                },
                            ),
                        }
                    }
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                    styles.pop();
                }
                _ => {}
            },
            Event::Text(text) => match stack.last_mut() {
                Some(Frame::Code { text: buffer, .. }) => buffer.push_str(&text),
                Some(Frame::Image { alt, .. }) => alt.push_str(&text),
                _ => {
                    if let Some(frame) = stack.last_mut() {
                        push_inline(frame, span(text.to_string(), &Active::current(&styles)));
                    }
                }
            },
            Event::Code(text) => {
                if let Some(frame) = stack.last_mut() {
                    push_inline(
                        frame,
                        span(
                            text.to_string(),
                            &Active {
                                code: true,
                                ..Active::current(&styles)
                            },
                        ),
                    );
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                // Breaks render as plain wrapping spaces; an element-level
                // line break cannot sit inside a wrapped inline row.
                if let Some(frame) = stack.last_mut() {
                    push_inline(frame, span(" ".into(), &Active::current(&styles)));
                }
            }
            Event::Rule => push_block(&mut stack, &mut blocks, Block::Rule),
            Event::TaskListMarker(checked) => {
                if let Some(Frame::Item { checked: slot, .. }) = stack.last_mut() {
                    *slot = Some(checked);
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                // The HTML itself is never interpreted; only `<img>` targets
                // are lifted out so workspace images still render. `<br>`
                // becomes the wrapping space the element would have caused.
                let images = html_images(&html);
                if html.contains("<br") {
                    if let Some(frame) = stack.last_mut() {
                        push_inline(frame, span(" ".into(), &Active::current(&styles)));
                    }
                }
                for (target, alt) in images {
                    let inline = Inline::Image { target, alt };
                    match stack.last_mut() {
                        Some(frame @ (Frame::Inline { .. } | Frame::Cell(_))) => {
                            push_inline(frame, inline)
                        }
                        _ => push_block(
                            &mut stack,
                            &mut blocks,
                            Block::Paragraph {
                                inlines: vec![inline],
                            },
                        ),
                    }
                }
            }
            Event::FootnoteReference(_) | Event::InlineMath(_) | Event::DisplayMath(_) => {}
        }
    }

    if blocks.len() > MAX_BLOCKS {
        blocks.truncate(MAX_BLOCKS);
        shortened = true;
    }
    Document {
        lines,
        blocks,
        shortened,
    }
}

/// Distinct image targets in display order.
pub fn image_targets(doc: &Document) -> Vec<String> {
    fn collect_inlines(blocks: &[Block], out: &mut Vec<Inline>) {
        for block in blocks {
            match block {
                Block::Heading { inlines, .. } | Block::Paragraph { inlines } => {
                    out.extend(inlines.iter().cloned())
                }
                Block::Quote { blocks: nested } => collect_inlines(nested, out),
                Block::List { items, .. } => {
                    for item in items {
                        collect_inlines(&item.blocks, out);
                    }
                }
                Block::Table { head, rows, .. } => {
                    for cell in head {
                        out.extend(cell.iter().cloned());
                    }
                    for row in rows {
                        for cell in row {
                            out.extend(cell.iter().cloned());
                        }
                    }
                }
                Block::Code { .. } | Block::Rule => {}
            }
        }
    }
    let mut inlines = Vec::new();
    collect_inlines(&doc.blocks, &mut inlines);
    let mut seen = BTreeSet::new();
    inlines
        .into_iter()
        .filter_map(|inline| match inline {
            Inline::Image { target, .. } => Some(target),
            Inline::Text(_) => None,
        })
        .filter(|target| seen.insert(target.clone()))
        .collect()
}

/// Resolve one markdown image target against the markdown file's folder into
/// a normalized workspace relative path. Remote, absolute, and unsafe targets
/// return `None`; the caller renders their alt text instead.
pub fn resolve_image_target(base: &str, target: &str) -> Option<String> {
    let target = target.trim();
    if target.is_empty()
        || target.starts_with('/')
        || target.contains("://")
        || target.starts_with("data:")
        || target.starts_with("mailto:")
    {
        return None;
    }
    let decoded = percent_decode(target.as_bytes())?;
    if decoded.chars().any(char::is_control) {
        return None;
    }
    let joined = match base.rsplit_once('/') {
        Some((dir, _)) if !dir.is_empty() => format!("{dir}/{decoded}"),
        _ => decoded,
    };
    let normalized = joined
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/");
    crate::workspace::validate_relative_path(&normalized).ok()
}

fn percent_decode(bytes: &[u8]) -> Option<String> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let hex = bytes.get(index + 1..index + 3)?;
                let byte = u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
                out.push(byte);
                index += 3;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inline_text(block: &Block) -> &str {
        match block {
            Block::Paragraph { inlines } => match &inlines[0] {
                Inline::Text(span) => &span.text,
                _ => panic!("text expected"),
            },
            _ => panic!("paragraph expected"),
        }
    }

    #[test]
    fn headings_lists_quotes_and_fences_preserve_source() {
        let text = "# Heading\n\nfirst\nsecond\n- item\n> quote\n```rust\nlet x = 1;\n```\n---";
        let doc = prepare(text, true);
        assert!(matches!(
            doc.blocks.as_slice(),
            [
                Block::Heading { level: 1, .. },
                Block::Paragraph { .. },
                Block::List { .. },
                Block::Quote { .. },
                Block::Code { .. },
                Block::Rule
            ]
        ));
        assert_eq!(inline_text(&doc.blocks[1]), "first second");
        let Block::Code {
            language,
            text: code,
        } = &doc.blocks[4]
        else {
            panic!("code expected");
        };
        assert_eq!(language.as_deref(), Some("rust"));
        assert_eq!(code.trim_end(), "let x = 1;");
        assert_eq!(doc.lines.join("\n"), text);
    }

    #[test]
    fn inline_styles_links_and_tables_are_structured() {
        let doc = prepare(
            "# Title\n\n**bold** *it* ~~gone~~ `code` [site](https://example.net)\n\n\
             | a | b |\n|---|:-:|\n| 1 | 2 |\n",
            true,
        );
        let Block::Paragraph { inlines } = &doc.blocks[1] else {
            panic!("paragraph expected");
        };
        let styles: Vec<(bool, bool, bool, bool, Option<&str>)> = inlines
            .iter()
            .filter_map(|inline| match inline {
                Inline::Text(span) => Some((
                    span.bold,
                    span.italic,
                    span.strike,
                    span.code,
                    span.link.as_deref(),
                )),
                _ => None,
            })
            .collect();
        assert!(styles.contains(&(true, false, false, false, None)));
        assert!(styles.contains(&(false, true, false, false, None)));
        assert!(styles.contains(&(false, false, true, false, None)));
        assert!(styles.contains(&(false, false, false, true, None)));
        assert!(styles.contains(&(false, false, false, false, Some("https://example.net"))));
        let Block::Table { aligns, head, rows } = &doc.blocks[2] else {
            panic!("table expected");
        };
        assert_eq!(aligns, &vec![CellAlign::Default, CellAlign::Center]);
        assert_eq!(head.len(), 2);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn task_lists_nesting_and_images_are_kept() {
        let doc = prepare(
            "- [x] done\n- [ ] open\n  - nested\n\n> ![pic](img/pic.png)\n",
            true,
        );
        let Block::List { ordered, items, .. } = &doc.blocks[0] else {
            panic!("list expected");
        };
        assert!(!ordered);
        assert_eq!(items[0].checked, Some(true));
        assert_eq!(items[1].checked, Some(false));
        assert_eq!(inline_text(&items[1].blocks[0]), "open");
        let Block::List { items: nested, .. } = &items[1].blocks[1] else {
            panic!("nested list expected");
        };
        assert_eq!(nested.len(), 1);
        let Block::Quote { blocks } = &doc.blocks[1] else {
            panic!("quote expected");
        };
        let Block::Paragraph { inlines } = &blocks[0] else {
            panic!("paragraph expected");
        };
        assert_eq!(
            inlines[0],
            Inline::Image {
                target: "img/pic.png".into(),
                alt: "pic".into()
            }
        );
        assert_eq!(image_targets(&doc), vec!["img/pic.png".to_owned()]);
    }

    #[test]
    fn tight_items_keep_styled_content_and_html_images_are_lifted() {
        let doc = prepare(
            "- **Runs real terminals.** Each pane owns a PTY.\n- plain tail\n",
            true,
        );
        let Block::List { items, .. } = &doc.blocks[0] else {
            panic!("list expected");
        };
        let Block::Paragraph { inlines } = &items[0].blocks[0] else {
            panic!("tight item paragraph expected");
        };
        assert_eq!(
            inlines[0],
            Inline::Text(Span {
                text: "Runs real terminals.".into(),
                bold: true,
                italic: false,
                strike: false,
                code: false,
                link: None,
            })
        );
        assert_eq!(inline_text(&items[0].blocks[0]), "Runs real terminals.");
        assert_eq!(inline_text(&items[1].blocks[0]), "plain tail");
        let doc = prepare(
            "<p align=\"center\">\n  <img src=\"./assets/hero.svg\" width=\"100%\" alt=\"Hero\">\n</p>\n",
            true,
        );
        let Block::Paragraph { inlines } = &doc.blocks[0] else {
            panic!("image paragraph expected");
        };
        assert_eq!(
            inlines[0],
            Inline::Image {
                target: "./assets/hero.svg".into(),
                alt: "Hero".into()
            }
        );
        assert_eq!(image_targets(&doc), vec!["./assets/hero.svg".to_owned()]);
    }

    #[test]
    fn oversized_and_unicode_documents_are_bounded() {
        let doc = prepare(&"日".repeat(5000), true);
        assert!(doc.shortened);
        assert_eq!(doc.lines[0].chars().count(), 4097);
        let doc = prepare(&"line\n".repeat(9000), true);
        assert!(doc.shortened);
        assert_eq!(doc.lines.len(), 8000);
        assert_eq!(prepare("", true).lines, vec![String::new()]);
        let long = "para\n\n".repeat(3000);
        let doc = prepare(&long, true);
        assert!(doc.blocks.len() <= MAX_BLOCKS);
        assert!(doc.shortened);
        assert!(prepare("# skip\n", false).blocks.is_empty());
    }

    #[test]
    fn image_targets_resolve_only_inside_the_workspace() {
        let base = "docs/readme.md";
        assert_eq!(
            resolve_image_target(base, "pic.png").as_deref(),
            Some("docs/pic.png")
        );
        assert_eq!(
            resolve_image_target(base, "./pic.png").as_deref(),
            Some("docs/pic.png")
        );
        assert_eq!(
            resolve_image_target(base, "sub/%20space.png").as_deref(),
            Some("docs/sub/ space.png")
        );
        assert_eq!(
            resolve_image_target("readme.md", "pic.png").as_deref(),
            Some("pic.png")
        );
        for target in [
            "",
            "/abs.png",
            "../escape.png",
            "https://example.net/x.png",
            "data:image/png;base64,AAAA",
            "mailto:x@y.z",
            "a\0b.png",
        ] {
            assert!(resolve_image_target(base, target).is_none(), "{target}");
        }
    }
}
