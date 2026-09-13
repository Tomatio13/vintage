//! In-memory workspace state, independent of GPUI and native resources.
use std::path::PathBuf;

pub type Id = u64;
pub const MAX_PANES: usize = 64;
const MAX_DEPTH: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// What a pane displays; file panes own no PTY or native resources.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaneKind {
    Terminal,
    File { path: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Layout {
    Pane {
        id: Id,
        kind: PaneKind,
    },
    Split {
        axis: Axis,
        first: Box<Layout>,
        second: Box<Layout>,
    },
}
impl Layout {
    pub fn panes(&self) -> Vec<Id> {
        self.panes_with_kinds()
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    }
    pub fn panes_with_kinds(&self) -> Vec<(Id, PaneKind)> {
        match self {
            Self::Pane { id, kind } => vec![(*id, kind.clone())],
            Self::Split { first, second, .. } => {
                let mut items = first.panes_with_kinds();
                items.extend(second.panes_with_kinds());
                items
            }
        }
    }
    fn split(&mut self, target: Id, new: Id, kind: PaneKind, axis: Axis, depth: usize) -> bool {
        match self {
            Self::Pane { id, kind: kept } if *id == target && depth < MAX_DEPTH => {
                *self = Self::Split {
                    axis,
                    first: Box::new(Self::Pane {
                        id: target,
                        kind: kept.clone(),
                    }),
                    second: Box::new(Self::Pane { id: new, kind }),
                };
                true
            }
            Self::Split { first, second, .. } => {
                first.split(target, new, kind.clone(), axis, depth + 1)
                    || second.split(target, new, kind, axis, depth + 1)
            }
            _ => false,
        }
    }
    fn remove(self, target: Id) -> Option<Self> {
        match self {
            Self::Pane { id, .. } => (id != target).then_some(self),
            Self::Split {
                axis,
                first,
                second,
            } => match (first.remove(target), second.remove(target)) {
                (Some(first), Some(second)) => Some(Self::Split {
                    axis,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (remaining, None) | (None, remaining) => remaining,
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct Tab {
    pub id: Id,
    pub title: String,
    pub layout: Layout,
    pub active_pane: Id,
}
#[derive(Clone, Debug)]
pub struct Workspace {
    pub id: Id,
    pub root: PathBuf,
    pub tabs: Vec<Tab>,
    pub active_tab: Option<Id>,
    next_tab: u64,
}
#[derive(Default)]
pub struct Workspaces {
    pub items: Vec<Workspace>,
    pub active: Option<Id>,
    next: Id,
}
impl Workspaces {
    /// Cycle tabs, panes or workspaces without creating or closing resources.
    pub fn navigate(&mut self, index: usize) {
        let next = |position: usize, len: usize| {
            if index.is_multiple_of(2) {
                (position + len - 1) % len
            } else {
                (position + 1) % len
            }
        };
        match index {
            0 | 1 => {
                if let Some(w) = self.current() {
                    if let Some(i) = w.tabs.iter().position(|t| Some(t.id) == w.active_tab) {
                        let target = w.tabs[next(i, w.tabs.len())].id;
                        self.activate(w.id, Some(target));
                    }
                }
            }
            2 | 3 => {
                if let Some(t) = self.tab() {
                    let panes = t.layout.panes();
                    if let Some(i) = panes.iter().position(|id| *id == t.active_pane) {
                        self.focus(panes[next(i, panes.len())]);
                    }
                }
            }
            4 | 5 => {
                if let Some(i) = self.items.iter().position(|w| Some(w.id) == self.active) {
                    let id = self.items[next(i, self.items.len())].id;
                    self.activate(id, None);
                }
            }
            _ => {}
        }
    }

    fn id(&mut self) -> Id {
        self.next += 1;
        self.next
    }
    pub fn pane_count(&self) -> usize {
        self.items
            .iter()
            .flat_map(|w| &w.tabs)
            .map(|t| t.layout.panes().len())
            .sum()
    }
    pub fn current(&self) -> Option<&Workspace> {
        self.items.iter().find(|w| Some(w.id) == self.active)
    }
    pub fn tab(&self) -> Option<&Tab> {
        let w = self.current()?;
        w.tabs.iter().find(|t| Some(t.id) == w.active_tab)
    }
    /// Returns the workspace and tab that own a live pane.
    pub fn pane_location(&self, pane: Id) -> Option<(Id, Id)> {
        self.items.iter().find_map(|workspace| {
            workspace
                .tabs
                .iter()
                .find(|tab| tab.layout.panes().contains(&pane))
                .map(|tab| (workspace.id, tab.id))
        })
    }
    fn current_mut(&mut self) -> Option<&mut Workspace> {
        self.items.iter_mut().find(|w| Some(w.id) == self.active)
    }
    pub fn add_workspace(&mut self, root: PathBuf) -> Result<Id, &'static str> {
        if let Some(w) = self.items.iter().find(|w| w.root == root) {
            self.active = Some(w.id);
            return Ok(w.id);
        }
        if self.items.len() >= 16 {
            return Err("Workspace limit reached (16)");
        }
        if self.pane_count() >= MAX_PANES {
            return Err("Pane limit reached (64)");
        }
        let id = self.id();
        self.items.push(Workspace {
            id,
            root,
            tabs: vec![],
            active_tab: None,
            next_tab: 0,
        });
        self.active = Some(id);
        self.add_tab()?;
        Ok(id)
    }
    pub fn activate(&mut self, workspace: Id, tab: Option<Id>) {
        if let Some(w) = self.items.iter_mut().find(|w| w.id == workspace) {
            self.active = Some(workspace);
            if let Some(id) = tab.filter(|id| w.tabs.iter().any(|t| t.id == *id)) {
                w.active_tab = Some(id);
            }
        }
    }
    pub fn add_tab(&mut self) -> Result<Id, &'static str> {
        if self.current().is_none() {
            return Err("Open a workspace first");
        }
        if self.pane_count() >= MAX_PANES {
            return Err("Terminal limit reached (64)");
        }
        let id = self.id();
        let pane = self.id();
        let w = self.current_mut().unwrap();
        w.next_tab += 1;
        w.tabs.push(Tab {
            id,
            title: format!("Terminal {}", w.next_tab),
            layout: Layout::Pane {
                id: pane,
                kind: PaneKind::Terminal,
            },
            active_pane: pane,
        });
        w.active_tab = Some(id);
        Ok(pane)
    }
    pub fn rename_tab(&mut self, id: Id, title: String) -> Result<(), &'static str> {
        let title = title.trim();
        if title.is_empty() {
            return Err("Tab name cannot be empty");
        }
        if title.chars().count() > 80 {
            return Err("Tab name is too long (80 characters maximum)");
        }
        let tab = self
            .current_mut()
            .and_then(|workspace| workspace.tabs.iter_mut().find(|tab| tab.id == id))
            .ok_or("Terminal tab not found")?;
        tab.title = title.to_owned();
        Ok(())
    }
    pub fn focus(&mut self, pane: Id) {
        if let Some(w) = self.current_mut() {
            if let Some(t) = w
                .tabs
                .iter_mut()
                .find(|t| Some(t.id) == w.active_tab && t.layout.panes().contains(&pane))
            {
                t.active_pane = pane;
            }
        }
    }
    pub fn split(&mut self, axis: Axis) -> Result<Id, &'static str> {
        if self.pane_count() >= MAX_PANES {
            return Err("Pane limit reached (64)");
        }
        if self.tab().is_none() {
            return Err("Open a tab first");
        }
        let new = self.id();
        let w = self.current_mut().unwrap();
        let t = w
            .tabs
            .iter_mut()
            .find(|t| Some(t.id) == w.active_tab)
            .unwrap();
        if !t
            .layout
            .split(t.active_pane, new, PaneKind::Terminal, axis, 0)
        {
            return Err("Split depth limit reached (8)");
        }
        t.active_pane = new;
        Ok(new)
    }
    /// Open a read-only file viewer pane beside the active pane, or focus the
    /// pane already showing the same file. The path is a normalized workspace
    /// relative path; the file service re-validates it before every read.
    pub fn open_file_pane(&mut self, path: &str) -> Result<Id, &'static str> {
        let path = validate_relative_path(path)?;
        if self.pane_count() >= MAX_PANES {
            return Err("Pane limit reached (64)");
        }
        if let Some(existing) = self.file_pane(&path) {
            self.focus(existing);
            return Ok(existing);
        }
        if self.tab().is_none() {
            return Err("Open a tab first");
        }
        let new = self.id();
        let w = self.current_mut().unwrap();
        let t = w
            .tabs
            .iter_mut()
            .find(|t| Some(t.id) == w.active_tab)
            .unwrap();
        if !t.layout.split(
            t.active_pane,
            new,
            PaneKind::File { path },
            Axis::Horizontal,
            0,
        ) {
            return Err("Split depth limit reached (8)");
        }
        t.active_pane = new;
        Ok(new)
    }
    /// The pane of the active tab that already shows this file, if any.
    fn file_pane(&self, path: &str) -> Option<Id> {
        self.tab()?
            .layout
            .panes_with_kinds()
            .into_iter()
            .find_map(|(id, kind)| match kind {
                PaneKind::File { path: found } if found == path => Some(id),
                _ => None,
            })
    }
    pub fn close_pane(&mut self, pane: Id) -> Vec<Id> {
        let Some(w) = self.current_mut() else {
            return vec![];
        };
        let Some(i) = w
            .tabs
            .iter()
            .position(|t| Some(t.id) == w.active_tab && t.layout.panes().contains(&pane))
        else {
            return vec![];
        };
        if let Some(layout) = w.tabs[i].layout.clone().remove(pane) {
            if w.tabs[i].active_pane == pane {
                w.tabs[i].active_pane = layout.panes()[0];
            }
            w.tabs[i].layout = layout;
        } else {
            w.tabs.remove(i);
            w.active_tab = w
                .tabs
                .get(i.min(w.tabs.len().saturating_sub(1)))
                .map(|t| t.id);
        }
        vec![pane]
    }
    pub fn close_tab(&mut self, id: Id) -> Vec<Id> {
        let Some(w) = self.current_mut() else {
            return vec![];
        };
        let Some(i) = w.tabs.iter().position(|t| t.id == id) else {
            return vec![];
        };
        let removed = w.tabs.remove(i).layout.panes();
        if w.active_tab == Some(id) {
            w.active_tab = w
                .tabs
                .get(i.min(w.tabs.len().saturating_sub(1)))
                .map(|t| t.id);
        }
        removed
    }
    pub fn close_workspace(&mut self, id: Id) -> Vec<Id> {
        let Some(i) = self.items.iter().position(|w| w.id == id) else {
            return vec![];
        };
        let removed = self
            .items
            .remove(i)
            .tabs
            .iter()
            .flat_map(|t| t.layout.panes())
            .collect();
        if self.active == Some(id) {
            self.active = self
                .items
                .get(i.min(self.items.len().saturating_sub(1)))
                .map(|w| w.id);
        }
        removed
    }
}
/// Mirrors the file service's normalized relative path rules so invalid
/// paths never enter workspace models.
pub fn validate_relative_path(path: &str) -> Result<String, &'static str> {
    if path.is_empty() {
        return Err("Select a file to open");
    }
    if path.len() > 8192 {
        return Err("The file path is too long");
    }
    if path.contains(['\\', ':', '\0'])
        || !path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
    {
        return Err("Use a normalized relative file path");
    }
    Ok(path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_wraps_and_preserves_inactive_panes() {
        let mut s = Workspaces::default();
        s.navigate(0);
        s.navigate(5);
        let a = s.add_workspace("/a".into()).unwrap();
        let first = s.tab().unwrap().active_pane;
        let second = s.split(Axis::Horizontal).unwrap();
        s.navigate(3);
        assert_eq!(s.tab().unwrap().active_pane, first);
        s.navigate(2);
        assert_eq!(s.tab().unwrap().active_pane, second);
        let tab = s.tab().unwrap().id;
        s.add_tab().unwrap();
        s.navigate(1);
        assert_eq!(s.tab().unwrap().id, tab);
        let b = s.add_workspace("/b".into()).unwrap();
        s.navigate(5);
        assert_eq!(s.active, Some(a));
        s.navigate(4);
        assert_eq!(s.active, Some(b));
        assert_eq!(s.pane_count(), 4);
    }
    #[test]
    fn pane_location_finds_inactive_tabs_and_workspaces() {
        let mut state = Workspaces::default();
        let first_workspace = state.add_workspace("/a".into()).unwrap();
        let first_tab = state.tab().unwrap().id;
        let first_pane = state.tab().unwrap().active_pane;
        let second_workspace = state.add_workspace("/b".into()).unwrap();
        let second_tab = state.tab().unwrap().id;
        let second_pane = state.tab().unwrap().active_pane;

        assert_eq!(
            state.pane_location(first_pane),
            Some((first_workspace, first_tab))
        );
        assert_eq!(
            state.pane_location(second_pane),
            Some((second_workspace, second_tab))
        );
        assert_eq!(state.pane_location(999), None);
    }
    #[test]
    fn recursive_splits_collapse_and_last_close_leaves_empty_workspace() {
        let mut state = Workspaces::default();
        state.add_workspace("/a".into()).unwrap();
        let first = state.tab().unwrap().active_pane;
        let second = state.split(Axis::Horizontal).unwrap();
        let third = state.split(Axis::Vertical).unwrap();
        assert_eq!(
            state.tab().unwrap().layout.panes(),
            vec![first, second, third]
        );
        assert_eq!(state.close_pane(second), vec![second]);
        assert_eq!(
            state.tab().unwrap().layout,
            Layout::Split {
                axis: Axis::Horizontal,
                first: Box::new(Layout::Pane {
                    id: first,
                    kind: PaneKind::Terminal
                }),
                second: Box::new(Layout::Pane {
                    id: third,
                    kind: PaneKind::Terminal
                })
            }
        );
        state.close_pane(third);
        assert_eq!(state.tab().unwrap().active_pane, first);
        state.close_pane(first);
        assert!(state.tab().is_none());
        assert!(state.current().is_some());
        state.add_tab().unwrap();
        assert_eq!(state.pane_count(), 1);
    }
    #[test]
    fn file_panes_split_beside_the_active_pane_and_dedupe_by_path() {
        let mut state = Workspaces::default();
        state.add_workspace("/a".into()).unwrap();
        let terminal = state.tab().unwrap().active_pane;
        let viewer = state.open_file_pane("docs/plan.md").unwrap();
        assert_eq!(state.tab().unwrap().active_pane, viewer);
        assert_eq!(state.open_file_pane("docs/plan.md").unwrap(), viewer);
        assert_eq!(state.pane_count(), 2);
        let other = state.split(Axis::Horizontal).unwrap();
        assert_eq!(
            state.tab().unwrap().layout.panes_with_kinds(),
            vec![
                (terminal, PaneKind::Terminal),
                (
                    viewer,
                    PaneKind::File {
                        path: "docs/plan.md".into()
                    }
                ),
                (other, PaneKind::Terminal),
            ]
        );
        state.close_pane(viewer);
        assert_eq!(
            state.tab().unwrap().layout.panes_with_kinds(),
            vec![(terminal, PaneKind::Terminal), (other, PaneKind::Terminal)]
        );
    }
    #[test]
    fn file_pane_paths_are_validated_before_layout_changes() {
        let mut state = Workspaces::default();
        state.add_workspace("/a".into()).unwrap();
        for path in ["", "/abs", "../up", "a/./b", "a//b", "C:\\temp", "a\0b"] {
            assert!(state.open_file_pane(path).is_err(), "{path}");
        }
        assert_eq!(state.pane_count(), 1);
        assert!(state.open_file_pane("ok.txt").is_ok());
    }
    #[test]
    fn switching_preserves_tabs_and_close_returns_only_owned_panes() {
        let mut s = Workspaces::default();
        let a = s.add_workspace("/a".into()).unwrap();
        let tab = s.tab().unwrap().id;
        let pane = s.tab().unwrap().active_pane;
        let other = s.add_tab().unwrap();
        let b = s.add_workspace("/b".into()).unwrap();
        let b_pane = s.tab().unwrap().active_pane;
        s.activate(a, Some(tab));
        assert_eq!(s.tab().unwrap().active_pane, pane);
        assert_eq!(s.close_tab(tab), vec![pane]);
        assert_eq!(s.tab().unwrap().active_pane, other);
        assert_eq!(s.close_workspace(a), vec![other]);
        assert_eq!(s.active, Some(b));
        assert_eq!(s.tab().unwrap().active_pane, b_pane);
        assert_eq!(s.add_workspace("/b".into()).unwrap(), b);
        assert_eq!(s.pane_count(), 1);
    }
    #[test]
    fn limits_and_unknown_ids_do_not_corrupt_layout() {
        let mut s = Workspaces::default();
        s.add_workspace("/a".into()).unwrap();
        for _ in 0..MAX_DEPTH {
            s.split(Axis::Horizontal).unwrap();
        }
        let before = s.tab().unwrap().layout.clone();
        assert!(s.split(Axis::Vertical).is_err());
        assert_eq!(s.tab().unwrap().layout, before);
        assert!(s.close_pane(999).is_empty());
        while s.pane_count() < MAX_PANES {
            s.add_tab().unwrap();
        }
        assert!(s.add_tab().is_err());
        assert!(s.open_file_pane("x").is_err());
        assert!(s.add_workspace("/b".into()).is_err());
        assert_eq!(s.items.len(), 1);
    }
    #[test]
    fn tab_names_are_validated_and_renamed_in_the_current_workspace() {
        let mut state = Workspaces::default();
        state.add_workspace("/a".into()).unwrap();
        let tab = state.tab().unwrap().id;
        assert!(state.rename_tab(tab, "  Build  ".into()).is_ok());
        assert_eq!(state.tab().unwrap().title, "Build");
        assert!(state.rename_tab(tab, " ".into()).is_err());
        assert!(state.rename_tab(tab, "x".repeat(81)).is_err());
        assert!(state.rename_tab(999, "Missing".into()).is_err());
    }
}
