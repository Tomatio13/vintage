//! Files panel. Filesystem work runs in the registered runtime service.
use super::*;
use crate::workspace::button;
use std::collections::{BTreeMap, BTreeSet};
use vintage_core::document::{self, BlockKind, Document};
use vintage_runtime::files::{Content, Entry, ImageKind, Listing, Preview, WorkspaceFiles};

pub struct FilesPanel {
    workspace: u64,
    root: PathBuf,
    service: Option<Arc<WorkspaceFiles>>,
    listings: BTreeMap<String, Listing>,
    expanded: BTreeSet<String>,
    directory_errors: BTreeMap<String, String>,
    selected: Option<String>,
    preview: Option<Preview>,
    document: Option<Document>,
    image: Option<Arc<gpui::Image>>,
    preview_error: Option<String>,
    source: bool,
    focus: FocusHandle,
    epoch: u64,
    request: u64,
    init_task: Option<gpui::Task<()>>,
    directory_tasks: BTreeMap<String, gpui::Task<()>>,
    preview_task: Option<gpui::Task<()>>,
}
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
            selected: None,
            preview: None,
            document: None,
            image: None,
            preview_error: None,
            source: false,
            focus: cx.focus_handle(),
            epoch: 0,
            request: 0,
            init_task: None,
            directory_tasks: BTreeMap::new(),
            preview_task: None,
        };
        view.reload(cx);
        view
    }
    pub fn close(&mut self, cx: &mut App) {
        self.epoch += 1;
        self.request += 1;
        if let Some(service) = self.service.take() {
            service.revoke();
        }
        if let Some(image) = self.image.take() {
            image.remove_asset(cx);
        }
        self.init_task = None;
        self.directory_tasks.clear();
        self.preview_task = None;
    }
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.close(cx);
        self.listings.clear();
        self.expanded.clear();
        self.directory_errors.clear();
        self.preview = None;
        self.document = None;
        self.preview_error = None;
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
            cx.notify();
            return;
        }
        if self.expanded.len() >= 128 {
            self.preview_error =
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
        let Some(service) = self.service.clone() else {
            return;
        };
        self.request += 1;
        let request = self.request;
        let epoch = self.epoch;
        let id = self.workspace;
        self.selected = Some(path.clone());
        self.preview = None;
        self.document = None;
        self.preview_error = None;
        if let Some(image) = self.image.take() {
            image.remove_asset(cx);
        }
        let executor = cx.background_executor().clone();
        self.preview_task = Some(cx.spawn(async move |entity, cx| {
            let result = executor
                .spawn(async move {
                    let preview = service.preview(id, &path)?;
                    let document = match &preview.content {
                        Content::Text { text, .. } => Some(document::prepare(text)),
                        _ => None,
                    };
                    Ok::<_, anyhow::Error>((preview, document))
                })
                .await;
            let _ = entity.update(cx, |view, cx| {
                if view.request != request || view.epoch != epoch {
                    return;
                }
                match result {
                    Ok((mut preview, document)) => {
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
                        view.preview = Some(preview);
                        view.document = document;
                    }
                    Err(error) => view.preview_error = Some(error.to_string()),
                }
                cx.notify();
            });
        }));
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
    fn preview_body(
        &self,
        palette: crate::theme::Palette,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
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
                .child(if self.selected.is_some() {
                    "Loading file…"
                } else {
                    "Select a file to preview."
                })
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
                    div()
                        .id(("markdown-preview", self.request))
                        .size_full()
                        .overflow_y_scroll()
                        .px_4()
                        .py_3()
                        .children(doc.blocks.iter().map(|block| {
                            let mut row = div()
                                .mb_2()
                                .text_size(gpui::rems(0.8125))
                                .line_height(gpui::rems(1.3125));
                            match block.kind {
                                BlockKind::Heading(level) => {
                                    row = row
                                        .mt_3()
                                        .text_size(gpui::rems(match level {
                                            1 => 1.5,
                                            2 => 1.25,
                                            3 => 1.0625,
                                            _ => 0.875,
                                        }))
                                        .font_weight(FontWeight::BOLD);
                                }
                                BlockKind::Code => {
                                    row = row
                                        .mb_0()
                                        .px_2()
                                        .bg(rgb(0x151411))
                                        .font_family(if cfg!(windows) {
                                            "Consolas"
                                        } else {
                                            "DejaVu Sans Mono"
                                        })
                                        .text_size(gpui::rems(0.75));
                                }
                                BlockKind::Quote => {
                                    row = row
                                        .pl_3()
                                        .border_l_2()
                                        .border_color(rgb(0x8c754e))
                                        .text_color(rgb(0xbab1a1));
                                }
                                BlockKind::Rule => {
                                    row = row.my_3().border_b_1().border_color(rgb(0x39352e));
                                }
                                BlockKind::Paragraph | BlockKind::List => {}
                            }
                            row.child(block.text.clone())
                        }))
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
}
impl Render for FilesPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = crate::theme::Palette::new(window, cx);
        let rgb = move |v| palette.color(v);
        let mut rows = Vec::new();
        self.rows("", 0, &mut rows);
        let count = rows.len();
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
        let body = self.preview_body(palette, cx);
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
                                                cx.listener(move |view, _, _, cx| {
                                                    if directory {
                                                        view.toggle_directory(path.clone(), cx);
                                                    } else {
                                                        view.open_file(path.clone(), cx);
                                                    }
                                                }),
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
                            .child(self.selected.clone().unwrap_or_else(|| "Preview".into())),
                    )
                    .when(self.document.is_some(), |row| {
                        row.child(button("copy-file-text", "Copy").on_click(cx.listener(
                            |view, _, _, cx| {
                                if let Some(Preview {
                                    content: Content::Text { text, .. },
                                    ..
                                }) = &view.preview
                                {
                                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                                }
                            },
                        )))
                    })
                    .when(markdown, |row| {
                        row.child(
                            button(
                                "preview-mode",
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
