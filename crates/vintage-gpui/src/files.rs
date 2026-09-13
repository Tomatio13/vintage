//! Files panel and read-only file viewers. Filesystem work runs in the
//! registered runtime service.
use super::*;
use crate::workspace::button;
use std::collections::{BTreeMap, BTreeSet};
use vintage_core::document::{self, Block, CellAlign, Document, Inline};
use vintage_runtime::files::{Content, Entry, ImageKind, Listing, Preview, WorkspaceFiles};

/// Markdown images resolve against the markdown file's folder, load through
/// the registered file service, and are bounded in count and total bytes.
const MAX_MARKDOWN_IMAGES: usize = 32;
const MAX_MARKDOWN_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MONO_FONT: &str = if cfg!(windows) {
    "Consolas"
} else {
    "DejaVu Sans Mono"
};

/// Apply a markdown table column alignment to a cell container.
fn table_align(cell: gpui::Div, align: Option<CellAlign>) -> gpui::Div {
    match align {
        Some(CellAlign::Left) => cell.text_left(),
        Some(CellAlign::Center) => cell.text_center(),
        Some(CellAlign::Right) => cell.text_right(),
        _ => cell,
    }
}

/// Base style for one markdown text stretch; runs override it per span.
#[derive(Clone, Copy)]
struct InlineStyle {
    color: u32,
    bold: bool,
}

fn is_web_link(link: &str) -> bool {
    link.starts_with("http://") || link.starts_with("https://")
}

/// Turn one accumulated text stretch into a single styled text node; with
/// web links present, the node becomes clickable on those byte ranges.
fn flush_text(
    text: &mut String,
    runs: &mut Vec<gpui::TextRun>,
    links: &mut Vec<(Range<usize>, String)>,
    segments: &mut Vec<gpui::AnyElement>,
    id: usize,
) {
    if text.is_empty() {
        return;
    }
    let styled = gpui::StyledText::new(std::mem::take(text)).with_runs(std::mem::take(runs));
    if links.is_empty() {
        segments.push(styled.into_any_element());
        return;
    }
    let ranges: Vec<Range<usize>> = links.iter().map(|(range, _)| range.clone()).collect();
    let urls: Arc<Vec<String>> = Arc::new(links.iter().map(|(_, url)| url.clone()).collect());
    segments.push(
        gpui::InteractiveText::new(("md-text", id), styled)
            .on_click(ranges, move |index, _, cx| {
                if let Some(url) = urls.get(index) {
                    cx.open_url(url);
                }
            })
            .into_any_element(),
    );
    links.clear();
}

/// Request to show a workspace file in a viewer pane.
pub struct OpenInPane(pub String);

/// Read-only viewer for one workspace file, used by the Files panel and by
/// viewer panes. Each viewer registers its own file service and loads on
/// workers; it never touches the filesystem from the UI thread.
pub struct FileViewer {
    workspace: u64,
    root: PathBuf,
    path: String,
    service: Option<Arc<WorkspaceFiles>>,
    preview: Option<Preview>,
    document: Option<Document>,
    image: Option<Arc<gpui::Image>>,
    /// Markdown images keyed by resolved workspace relative path.
    images: BTreeMap<String, Arc<gpui::Image>>,
    image_errors: BTreeMap<String, String>,
    image_tasks: Vec<gpui::Task<()>>,
    /// Remaining bytes the markdown viewer may still load.
    image_budget: u64,
    preview_error: Option<String>,
    source: bool,
    pub(crate) focus: FocusHandle,
    request: u64,
    load_task: Option<gpui::Task<()>>,
    /// Virtualized list state for the rendered markdown blocks.
    markdown_list: Option<gpui::ListState>,
}
impl FileViewer {
    pub fn new(workspace: u64, root: PathBuf, path: String, cx: &mut Context<Self>) -> Self {
        cx.on_release(|view, cx| view.close(cx)).detach();
        let mut view = Self {
            workspace,
            root,
            path,
            service: None,
            preview: None,
            document: None,
            image: None,
            images: BTreeMap::new(),
            image_errors: BTreeMap::new(),
            image_tasks: Vec::new(),
            image_budget: MAX_MARKDOWN_IMAGE_BYTES,
            preview_error: None,
            source: false,
            focus: cx.focus_handle(),
            request: 0,
            load_task: None,
            markdown_list: None,
        };
        view.load(cx);
        view
    }
    pub fn close(&mut self, cx: &mut App) {
        self.request += 1;
        if let Some(service) = self.service.take() {
            service.revoke();
        }
        if let Some(image) = self.image.take() {
            image.remove_asset(cx);
        }
        for (_, image) in std::mem::take(&mut self.images) {
            image.remove_asset(cx);
        }
        self.image_errors.clear();
        self.image_tasks.clear();
        self.markdown_list = None;
        self.load_task = None;
    }
    fn load(&mut self, cx: &mut Context<Self>) {
        self.request += 1;
        let request = self.request;
        let workspace = self.workspace;
        let root = self.root.clone();
        let path = self.path.clone();
        self.preview = None;
        self.document = None;
        self.preview_error = None;
        if let Some(image) = self.image.take() {
            image.remove_asset(cx);
        }
        for (_, image) in std::mem::take(&mut self.images) {
            image.remove_asset(cx);
        }
        self.image_errors.clear();
        self.image_tasks.clear();
        self.image_budget = MAX_MARKDOWN_IMAGE_BYTES;
        self.markdown_list = None;
        let executor = cx.background_executor().clone();
        self.load_task = Some(cx.spawn(async move |entity, cx| {
            let result = executor
                .spawn(async move {
                    let service = Arc::new(WorkspaceFiles::register(workspace, &root)?);
                    let preview = service.preview(workspace, &path)?;
                    let document = match &preview.content {
                        Content::Text { text, markdown, .. } => {
                            Some(document::prepare(text, *markdown))
                        }
                        _ => None,
                    };
                    Ok::<_, anyhow::Error>((service, preview, document))
                })
                .await;
            let _ = entity.update(cx, |view, cx| {
                if view.request != request {
                    return;
                }
                match result {
                    Ok((service, mut preview, document)) => {
                        if let Content::Image { bytes, kind } = &mut preview.content {
                            let format = match kind {
                                ImageKind::Png => gpui::ImageFormat::Png,
                                ImageKind::Jpeg => gpui::ImageFormat::Jpeg,
                            };
                            view.image = Some(Arc::new(gpui::Image::from_bytes(
                                format,
                                std::mem::take(bytes),
                            )));
                        }
                        view.service = Some(service);
                        let markdown =
                            matches!(&preview.content, Content::Text { markdown: true, .. });
                        view.preview = Some(preview);
                        view.document = document;
                        if markdown {
                            let count = view.document.as_ref().map(|d| d.blocks.len()).unwrap_or(0);
                            view.markdown_list = Some(gpui::ListState::new(
                                count,
                                gpui::ListAlignment::Top,
                                px(1000.),
                            ));
                            view.queue_markdown_images(request, cx);
                        }
                    }
                    Err(error) => view.preview_error = Some(error.to_string()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    /// Queue loads for every workspace-relative image referenced by the
    /// markdown document. Remote images are never fetched; their alt text
    /// renders instead.
    fn queue_markdown_images(&mut self, request: u64, cx: &mut Context<Self>) {
        let Some(doc) = &self.document else {
            return;
        };
        let Some(service) = self.service.clone() else {
            return;
        };
        let base_dir = match self.path.rsplit_once('/') {
            Some((dir, _)) => dir,
            None => "",
        };
        let targets = document::image_targets(doc);
        let executor = cx.background_executor().clone();
        for target in targets.into_iter().take(MAX_MARKDOWN_IMAGES) {
            let Some(path) = document::resolve_image_target(base_dir, &target) else {
                continue;
            };
            if self.images.contains_key(&path) || self.image_errors.contains_key(&path) {
                continue;
            }
            if self.image_budget == 0 {
                self.image_errors
                    .insert(path, "Image limit reached".to_owned());
                continue;
            }
            let workspace = self.workspace;
            let service = service.clone();
            let executor = executor.clone();
            let fetch_path = path.clone();
            let task = cx.spawn(async move |entity, cx| {
                let result = executor
                    .spawn(async move { service.preview(workspace, &fetch_path) })
                    .await;
                let _ = entity.update(cx, |view, cx| {
                    if view.request != request {
                        return;
                    }
                    match result {
                        Ok(Preview {
                            content: Content::Image { mut bytes, kind },
                            ..
                        }) => {
                            if (bytes.len() as u64) > view.image_budget {
                                view.image_errors
                                    .insert(path, "Image limit reached".to_owned());
                            } else {
                                view.image_budget -= bytes.len() as u64;
                                let format = match kind {
                                    ImageKind::Png => gpui::ImageFormat::Png,
                                    ImageKind::Jpeg => gpui::ImageFormat::Jpeg,
                                };
                                view.images.insert(
                                    path,
                                    Arc::new(gpui::Image::from_bytes(
                                        format,
                                        std::mem::take(&mut bytes),
                                    )),
                                );
                            }
                        }
                        Ok(Preview {
                            content: Content::Unsupported(message),
                            ..
                        }) => {
                            view.image_errors.insert(path, message);
                        }
                        Ok(_) | Err(_) => {
                            view.image_errors.insert(path, "Cannot load image".into());
                        }
                    }
                    cx.notify();
                });
            });
            self.image_tasks.push(task);
        }
    }
    fn body(&self, palette: crate::theme::Palette, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = move |v| palette.color(v);
        if let Some(error) = &self.preview_error {
            return div()
                .p_4()
                .text_color(rgb(0xe8aa82))
                .child(error.clone())
                .into_any_element();
        }
        let Some(preview) = &self.preview else {
            return div()
                .p_4()
                .text_color(rgb(0x9b958a))
                .child("Loading file…")
                .into_any_element();
        };
        match &preview.content {
            Content::Unsupported(message) => div()
                .p_4()
                .text_color(rgb(0x9b958a))
                .child(message.clone())
                .into_any_element(),
            Content::Image { .. } => div()
                .size_full()
                .p_3()
                .child(
                    gpui::img(self.image.as_ref().unwrap().clone())
                        .size_full()
                        .object_fit(gpui::ObjectFit::Contain)
                        .with_fallback(|| {
                            div()
                                .p_4()
                                .child("This image could not be decoded.")
                                .into_any_element()
                        }),
                )
                .into_any_element(),
            Content::Text { markdown, .. } => {
                let doc = self.document.as_ref().unwrap();
                if *markdown && !self.source {
                    let Some(list_state) = self.markdown_list.clone() else {
                        return div()
                            .p_4()
                            .text_color(rgb(0x9b958a))
                            .child("Loading file…")
                            .into_any_element();
                    };
                    // One list entry per top-level block; only the visible
                    // blocks are built and laid out per frame.
                    let this = cx.entity().downgrade();
                    let list = gpui::list(list_state, move |index, window, cx| {
                        let Some(view) = this.upgrade() else {
                            return gpui::Empty.into_any_element();
                        };
                        view.update(cx, |view, cx| {
                            let palette = crate::theme::Palette::new(window, cx);
                            let Some(doc) = &view.document else {
                                return gpui::Empty.into_any_element();
                            };
                            match doc.blocks.get(index) {
                                Some(block) => view
                                    .render_blocks(
                                        std::slice::from_ref(block),
                                        index * 1000,
                                        palette,
                                        InlineStyle {
                                            color: 0xe6e1d8,
                                            bold: false,
                                        },
                                    )
                                    .into_iter()
                                    .next()
                                    .expect("one block renders one element"),
                                None => gpui::Empty.into_any_element(),
                            }
                        })
                    })
                    .size_full();
                    div()
                        .size_full()
                        .px_4()
                        .py_3()
                        .text_size(gpui::rems(0.8125))
                        .line_height(gpui::rems(1.3125))
                        .child(list)
                        .into_any_element()
                } else {
                    gpui::uniform_list(
                        ("source-lines", self.request),
                        doc.lines.len(),
                        cx.processor(move |view, range: Range<usize>, _, _| {
                            let lines = &view.document.as_ref().unwrap().lines;
                            range
                                .map(|index| {
                                    div()
                                        .h(px(20.))
                                        .flex()
                                        .items_center()
                                        .font_family(if cfg!(windows) {
                                            "Consolas"
                                        } else {
                                            "DejaVu Sans Mono"
                                        })
                                        .text_size(gpui::rems(0.75))
                                        .child(
                                            div()
                                                .w(px(48.))
                                                .flex_none()
                                                .pr_2()
                                                .text_right()
                                                .text_color(rgb(0x777165))
                                                .child((index + 1).to_string()),
                                        )
                                        .child(
                                            div().whitespace_nowrap().child(lines[index].clone()),
                                        )
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .with_width_from_item(
                        doc.lines
                            .iter()
                            .enumerate()
                            .max_by_key(|(_, line)| line.chars().count())
                            .map(|(index, _)| index),
                    )
                    .with_horizontal_sizing_behavior(
                        gpui::ListHorizontalSizingBehavior::Unconstrained,
                    )
                    .size_full()
                    .into_any_element()
                }
            }
        }
    }
    fn base_dir(&self) -> &str {
        match self.path.rsplit_once('/') {
            Some((dir, _)) => dir,
            None => "",
        }
    }
    fn render_blocks(
        &self,
        blocks: &[Block],
        id_base: usize,
        palette: crate::theme::Palette,
        style: InlineStyle,
    ) -> Vec<gpui::AnyElement> {
        let rgb = |value| palette.color(value);
        let mut out = Vec::new();
        for (offset, block) in blocks.iter().enumerate() {
            let id_base = id_base + offset;
            match block {
                Block::Heading { level, inlines } => {
                    let size = match level {
                        1 => 1.5,
                        2 => 1.25,
                        3 => 1.0625,
                        _ => 0.875,
                    };
                    let inlines = self.render_inlines(
                        inlines,
                        id_base,
                        palette,
                        InlineStyle {
                            color: style.color,
                            bold: true,
                        },
                    );
                    out.push(
                        div()
                            .mt_3()
                            .mb_1()
                            .text_size(gpui::rems(size))
                            .line_height(gpui::rems(size * 1.25))
                            .child(inlines)
                            .into_any_element(),
                    );
                }
                Block::Paragraph { inlines } => {
                    let inlines = self.render_inlines(inlines, id_base, palette, style);
                    out.push(div().mb_2().child(inlines).into_any_element());
                }
                Block::Code { text, language } => {
                    let mut block = div()
                        .id(("md-code", id_base))
                        .mb_2()
                        .overflow_x_scroll()
                        .rounded_sm()
                        .bg(rgb(0x151411));
                    if let Some(language) = language {
                        block = block.child(
                            div()
                                .px_2()
                                .pt_1()
                                .text_size(gpui::rems(0.6875))
                                .text_color(rgb(0x9b958a))
                                .child(language.clone()),
                        );
                    }
                    block = block.child(
                        div()
                            .px_2()
                            .py_1()
                            .font_family(MONO_FONT)
                            .text_size(gpui::rems(0.75))
                            .children(
                                text.lines()
                                    .map(|line| div().whitespace_nowrap().child(line.to_owned())),
                            ),
                    );
                    out.push(block.into_any_element());
                }
                Block::Quote { blocks: nested } => {
                    let children = self.render_blocks(
                        nested,
                        id_base,
                        palette,
                        InlineStyle {
                            color: 0xbab1a1,
                            bold: style.bold,
                        },
                    );
                    out.push(
                        div()
                            .mb_2()
                            .pl_3()
                            .border_l_2()
                            .border_color(rgb(0x8c754e))
                            .children(children)
                            .into_any_element(),
                    );
                }
                Block::List {
                    ordered,
                    start,
                    items,
                } => {
                    let mut rows = Vec::new();
                    for (index, item) in items.iter().enumerate() {
                        let marker = match item.checked {
                            Some(true) => "\u{2611}".to_owned(),
                            Some(false) => "\u{2610}".to_owned(),
                            None if *ordered => format!("{}.", start + index as u64),
                            None => "\u{2022}".to_owned(),
                        };
                        let content = self.render_blocks(&item.blocks, id_base, palette, style);
                        rows.push(
                            div()
                                .flex()
                                .gap_2()
                                .child(div().flex_none().child(marker))
                                .child(div().flex_1().min_w_0().children(content))
                                .into_any_element(),
                        );
                    }
                    out.push(
                        div()
                            .mb_2()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .children(rows)
                            .into_any_element(),
                    );
                }
                Block::Table { aligns, head, rows } => {
                    let mut cells = Vec::new();
                    for (index, cell) in head.iter().enumerate() {
                        let inlines = self.render_inlines(
                            cell,
                            id_base,
                            palette,
                            InlineStyle {
                                color: style.color,
                                bold: true,
                            },
                        );
                        let cell = div().flex_1().min_w_0().px_2().py_1().bg(rgb(0x2b2923));
                        cells.push(
                            table_align(cell, aligns.get(index).copied())
                                .child(inlines)
                                .into_any_element(),
                        );
                    }
                    let mut body_rows = Vec::new();
                    for row in rows {
                        let mut cells = Vec::new();
                        for (index, cell) in row.iter().enumerate() {
                            let inlines = self.render_inlines(
                                cell,
                                id_base,
                                palette,
                                InlineStyle {
                                    color: style.color,
                                    bold: false,
                                },
                            );
                            let cell = div()
                                .flex_1()
                                .min_w_0()
                                .px_2()
                                .py_1()
                                .border_t_1()
                                .border_color(rgb(0x39352e));
                            cells.push(
                                table_align(cell, aligns.get(index).copied())
                                    .child(inlines)
                                    .into_any_element(),
                            );
                        }
                        body_rows.push(div().flex().children(cells).into_any_element());
                    }
                    out.push(
                        div()
                            .id(("md-table", id_base))
                            .mb_2()
                            .overflow_x_scroll()
                            .border_1()
                            .rounded_sm()
                            .border_color(rgb(0x39352e))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(div().flex().children(cells))
                                    .children(body_rows),
                            )
                            .into_any_element(),
                    );
                }
                Block::Rule => out.push(
                    div()
                        .my_3()
                        .border_b_1()
                        .border_color(rgb(0x39352e))
                        .into_any_element(),
                ),
            }
        }
        out
    }
    /// Render inline runs as one wrapped text node per stretch, styled with
    /// text runs instead of a flex row of per-span containers, and one image
    /// element per inline image. Web links stay clickable through the text.
    fn render_inlines(
        &self,
        inlines: &[Inline],
        id_base: usize,
        palette: crate::theme::Palette,
        style: InlineStyle,
    ) -> gpui::AnyElement {
        let rgb = |value| palette.color(value);
        let mut segments: Vec<gpui::AnyElement> = Vec::new();
        let mut text = String::new();
        let mut runs: Vec<gpui::TextRun> = Vec::new();
        let mut links: Vec<(Range<usize>, String)> = Vec::new();
        let mut stretch = 0usize;
        for inline in inlines {
            match inline {
                Inline::Text(span) => {
                    if span.text.is_empty() {
                        continue;
                    }
                    let mut font = gpui::Font::default();
                    if span.code {
                        font.family = MONO_FONT.into();
                    }
                    if style.bold || span.bold {
                        font.weight = FontWeight::BOLD;
                    }
                    if span.italic {
                        font.style = FontStyle::Italic;
                    }
                    let color = match &span.link {
                        Some(link) if is_web_link(link) => rgb(0xc6a66b),
                        Some(_) => rgb(0xbab1a1),
                        None => rgb(style.color),
                    };
                    runs.push(gpui::TextRun {
                        len: span.text.len(),
                        font,
                        color: color.into(),
                        background_color: span.code.then_some(rgb(0x151411).into()),
                        underline: span.link.is_some().then_some(gpui::UnderlineStyle {
                            color: None,
                            thickness: px(1.),
                            wavy: false,
                        }),
                        strikethrough: span.strike.then_some(gpui::StrikethroughStyle {
                            color: None,
                            thickness: px(1.),
                        }),
                    });
                    match &span.link {
                        Some(link) if is_web_link(link) => {
                            let start = text.len();
                            text.push_str(&span.text);
                            links.push((start..text.len(), link.clone()));
                        }
                        _ => text.push_str(&span.text),
                    }
                }
                Inline::Image { target, alt } => {
                    flush_text(
                        &mut text,
                        &mut runs,
                        &mut links,
                        &mut segments,
                        id_base * 8 + stretch,
                    );
                    stretch += 1;
                    segments.push(self.render_image(target, alt, palette));
                }
            }
        }
        flush_text(
            &mut text,
            &mut runs,
            &mut links,
            &mut segments,
            id_base * 8 + stretch,
        );
        if segments.len() == 1 {
            segments.pop().expect("segment present")
        } else {
            div()
                .flex()
                .flex_col()
                .children(segments)
                .into_any_element()
        }
    }

    /// One markdown image: the decoded asset once loaded, else its alt text
    /// with the failure reason. Unresolvable targets never reach the loader.
    fn render_image(
        &self,
        target: &str,
        alt: &str,
        palette: crate::theme::Palette,
    ) -> gpui::AnyElement {
        let rgb = |value| palette.color(value);
        let resolved = document::resolve_image_target(self.base_dir(), target);
        let loaded = resolved
            .as_ref()
            .and_then(|path| self.images.get(path).cloned());
        match loaded {
            Some(image) => div()
                .flex_none()
                .my_1()
                .child(
                    gpui::img(image)
                        .max_w(relative(1.))
                        .max_h(gpui::rems(20.))
                        .object_fit(gpui::ObjectFit::Contain)
                        .rounded_sm(),
                )
                .into_any_element(),
            None => {
                let detail = resolved
                    .as_ref()
                    .and_then(|path| self.image_errors.get(path))
                    .map(|error| format!(" ({error})"))
                    .unwrap_or_default();
                div()
                    .text_color(rgb(0x9b958a))
                    .italic()
                    .child(format!("🖼 {alt}{detail}"))
                    .into_any_element()
            }
        }
    }
}
impl Render for FileViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = crate::theme::Palette::new(window, cx);
        let rgb = move |v| palette.color(v);
        let markdown = self
            .preview
            .as_ref()
            .is_some_and(|p| matches!(p.content, Content::Text { markdown: true, .. }));
        let shortened = self.document.as_ref().is_some_and(|d| d.shortened)
            || self.preview.as_ref().is_some_and(|p| {
                matches!(
                    p.content,
                    Content::Text {
                        truncated: true,
                        ..
                    }
                )
            });
        let size = self
            .preview
            .as_ref()
            .map(|p| format!("{} bytes", p.size))
            .unwrap_or_default();
        let body = self.body(palette, cx);
        div()
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(rgb(0x201f1c))
            .text_color(rgb(0xe6e1d8))
            .text_size(gpui::rems(0.75))
            .track_focus(&self.focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, window, cx| view.focus.focus(window, cx)),
            )
            .child(
                div()
                    .h(gpui::rems(2.125))
                    .flex_none()
                    .px_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(self.path.clone()),
                    )
                    .when(self.document.is_some(), |row| {
                        row.child(button(("copy-file-text", self.request), "Copy").on_click(
                            cx.listener(|view, _, _, cx| {
                                if let Some(Preview {
                                    content: Content::Text { text, .. },
                                    ..
                                }) = &view.preview
                                {
                                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                                }
                            }),
                        ))
                    })
                    .when(markdown, |row| {
                        row.child(
                            button(
                                ("preview-mode", self.request),
                                if self.source { "Preview" } else { "Source" },
                            )
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.source = !view.source;
                                cx.notify();
                            })),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(body),
            )
            .child(
                div()
                    .h(gpui::rems(1.625))
                    .flex_none()
                    .px_3()
                    .flex()
                    .items_center()
                    .text_color(rgb(0x9b958a))
                    .child(if shortened {
                        format!("Read only · {size} · Preview shortened")
                    } else {
                        format!("Read only · {size}")
                    }),
            )
    }
}

pub struct FilesPanel {
    workspace: u64,
    root: PathBuf,
    service: Option<Arc<WorkspaceFiles>>,
    listings: BTreeMap<String, Listing>,
    expanded: BTreeSet<String>,
    directory_errors: BTreeMap<String, String>,
    tree_error: Option<String>,
    selected: Option<String>,
    viewer: Option<Entity<FileViewer>>,
    focus: FocusHandle,
    epoch: u64,
    init_task: Option<gpui::Task<()>>,
    directory_tasks: BTreeMap<String, gpui::Task<()>>,
}
impl gpui::EventEmitter<OpenInPane> for FilesPanel {}
impl FilesPanel {
    pub fn new(workspace: u64, root: PathBuf, cx: &mut Context<Self>) -> Self {
        cx.on_release(|view, cx| view.close(cx)).detach();
        let mut view = Self {
            workspace,
            root,
            service: None,
            listings: BTreeMap::new(),
            expanded: BTreeSet::new(),
            directory_errors: BTreeMap::new(),
            tree_error: None,
            selected: None,
            viewer: None,
            focus: cx.focus_handle(),
            epoch: 0,
            init_task: None,
            directory_tasks: BTreeMap::new(),
        };
        view.reload(cx);
        view
    }
    pub fn close(&mut self, cx: &mut App) {
        self.epoch += 1;
        if let Some(service) = self.service.take() {
            service.revoke();
        }
        if let Some(viewer) = self.viewer.take() {
            viewer.update(cx, |view, cx| view.close(cx));
        }
        self.init_task = None;
        self.directory_tasks.clear();
    }
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.close(cx);
        self.listings.clear();
        self.expanded.clear();
        self.directory_errors.clear();
        self.tree_error = None;
        let root = self.root.clone();
        let id = self.workspace;
        let epoch = self.epoch;
        let executor = cx.background_executor().clone();
        self.init_task = Some(cx.spawn(async move |entity, cx| {
            let result = executor
                .spawn(async move {
                    let service = Arc::new(WorkspaceFiles::register(id, &root)?);
                    let listing = service.list(id, "")?;
                    Ok::<_, anyhow::Error>((service, listing))
                })
                .await;
            let _ = entity.update(cx, |view, cx| {
                if view.epoch != epoch {
                    return;
                }
                match result {
                    Ok((service, listing)) => {
                        view.service = Some(service);
                        view.listings.insert(String::new(), listing);
                        if let Some(path) = view.selected.clone() {
                            view.open_file(path, cx);
                        }
                    }
                    Err(error) => {
                        view.directory_errors
                            .insert(String::new(), error.to_string());
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn toggle_directory(&mut self, path: String, cx: &mut Context<Self>) {
        if self.expanded.remove(&path) {
            let prefix = format!("{path}/");
            self.expanded.retain(|p| !p.starts_with(&prefix));
            self.listings
                .retain(|p, _| p != &path && !p.starts_with(&prefix));
            self.directory_tasks
                .retain(|p, _| p != &path && !p.starts_with(&prefix));
            self.directory_errors
                .retain(|p, _| p != &path && !p.starts_with(&prefix));
            if self.tree_error.is_some() && self.expanded.len() < 128 {
                self.tree_error = None;
            }
            cx.notify();
            return;
        }
        if self.expanded.len() >= 128 {
            self.tree_error =
                Some("Collapse a folder before expanding more (128-folder limit)".into());
            cx.notify();
            return;
        }
        let Some(service) = self.service.clone() else {
            return;
        };
        self.expanded.insert(path.clone());
        self.directory_errors.remove(&path);
        let task_path = path.clone();
        let id = self.workspace;
        let epoch = self.epoch;
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |entity, cx| {
            let lookup = task_path.clone();
            let result = executor
                .spawn(async move { service.list(id, &lookup) })
                .await;
            let _ = entity.update(cx, |view, cx| {
                if view.epoch != epoch || !view.expanded.contains(&task_path) {
                    return;
                }
                match result {
                    Ok(listing) => {
                        let count: usize = view.listings.values().map(|l| l.entries.len()).sum();
                        if count + listing.entries.len() > 10000 {
                            view.directory_errors.insert(
                                task_path,
                                "File tree limit reached; collapse folders or refresh".into(),
                            );
                        } else {
                            view.listings.insert(task_path, listing);
                        }
                    }
                    Err(error) => {
                        view.directory_errors.insert(task_path, error.to_string());
                    }
                }
                cx.notify();
            });
        });
        self.directory_tasks.insert(path, task);
        cx.notify();
    }
    fn open_file(&mut self, path: String, cx: &mut Context<Self>) {
        // Re-selecting the shown file keeps the loaded viewer; Refresh reloads.
        if self.selected.as_deref() == Some(path.as_str()) && self.viewer.is_some() {
            return;
        }
        self.selected = Some(path.clone());
        let (workspace, root) = (self.workspace, self.root.clone());
        self.viewer = Some(cx.new(|cx| FileViewer::new(workspace, root, path, cx)));
        cx.notify();
    }
    fn rows(&self, path: &str, depth: usize, rows: &mut Vec<(usize, Option<Entry>, String)>) {
        if let Some(error) = self.directory_errors.get(path) {
            rows.push((depth, None, error.clone()));
            return;
        }
        let Some(listing) = self.listings.get(path) else {
            rows.push((depth, None, "Loading…".into()));
            return;
        };
        for entry in &listing.entries {
            rows.push((depth, Some(entry.clone()), String::new()));
            if entry.directory && self.expanded.contains(&entry.path) {
                self.rows(&entry.path, depth + 1, rows);
            }
        }
        if listing.truncated {
            rows.push((
                depth,
                None,
                "Folder listing limited to 2,000 entries".into(),
            ));
        }
        if listing.entries.is_empty() {
            rows.push((depth, None, "Empty folder".into()));
        }
    }
}
impl Render for FilesPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = crate::theme::Palette::new(window, cx);
        let rgb = move |v| palette.color(v);
        let mut rows = Vec::new();
        self.rows("", 0, &mut rows);
        let count = rows.len();
        let body: gpui::AnyElement = match &self.viewer {
            Some(viewer) => viewer.clone().into_any_element(),
            None => div()
                .p_4()
                .text_color(rgb(0x9b958a))
                .child("Select a file to preview, double-click to open it in a pane.")
                .into_any_element(),
        };
        div()
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(rgb(0x201f1c))
            .text_color(rgb(0xe6e1d8))
            .text_size(gpui::rems(0.75))
            .track_focus(&self.focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, window, cx| view.focus.focus(window, cx)),
            )
            .child(
                div()
                    .h(gpui::rems(2.125))
                    .flex_none()
                    .px_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_color(rgb(0xc6a66b)).child("WORKSPACE FILES"))
                    .child(
                        button("refresh-files", "Refresh")
                            .on_click(cx.listener(|view, _, _, cx| view.reload(cx))),
                    ),
            )
            .child(
                div()
                    .h(px(220.))
                    .min_h_0()
                    .flex_none()
                    .border_b_1()
                    .border_color(rgb(0x39352e))
                    .child(
                        gpui::uniform_list(
                            "file-tree",
                            count,
                            cx.processor(move |view, range: Range<usize>, _, cx| {
                                range
                                    .map(|index| {
                                        let (depth, entry, message) = &rows[index];
                                        let row = div()
                                            .id(("file-row", index))
                                            .w_full()
                                            .h(gpui::rems(1.5))
                                            .pl(px(12. + *depth as f32 * 14.))
                                            .pr_2()
                                            .flex()
                                            .items_center()
                                            .overflow_hidden();
                                        if let Some(entry) = entry {
                                            let path = entry.path.clone();
                                            let directory = entry.directory;
                                            let marker = if directory {
                                                if view.expanded.contains(&entry.path) {
                                                    "▾"
                                                } else {
                                                    "▸"
                                                }
                                            } else if entry.symlink {
                                                "↗"
                                            } else {
                                                "·"
                                            };
                                            row.bg(rgb(
                                                if view.selected.as_ref() == Some(&entry.path) {
                                                    0x393328
                                                } else {
                                                    0x201f1c
                                                },
                                            ))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(rgb(0x343027)))
                                            .child(format!("{marker}  {}", entry.name))
                                            .on_click(
                                                cx.listener(
                                                    move |view, event: &gpui::ClickEvent, _, cx| {
                                                        if directory {
                                                            // The first click already toggled; a
                                                            // double-click would toggle twice.
                                                            if event.click_count() == 1 {
                                                                view.toggle_directory(
                                                                    path.clone(),
                                                                    cx,
                                                                );
                                                            }
                                                        } else if event.click_count() >= 2 {
                                                            view.selected = Some(path.clone());
                                                            cx.emit(OpenInPane(path.clone()));
                                                        } else {
                                                            view.open_file(path.clone(), cx);
                                                        }
                                                    },
                                                ),
                                            )
                                        } else {
                                            row.text_color(rgb(0x9b958a)).child(message.clone())
                                        }
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .size_full(),
                    ),
            )
            .when_some(self.tree_error.clone(), |panel, error| {
                panel.child(
                    div()
                        .flex_none()
                        .px_3()
                        .py_1()
                        .text_color(rgb(0xe8aa82))
                        .child(error),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(body),
            )
    }
}
