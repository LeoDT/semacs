//! Filesystem data and text/section projection for the folder tree.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::outline::{self, OpenFileOverlay, Symbol};
use crate::section::{Section, Sections};
use crate::text::{ByteOffset, Rope};
use crate::theme::StyleRole;

mod git;
pub use git::{GitNodeState, TreeGitStatus};

#[cfg(test)]
pub(crate) mod tests;

const COLLAPSE_DEPTH: usize = 3;

#[derive(Clone, Debug)]
struct Entry {
    path: PathBuf,
    directory: bool,
}

#[derive(Clone, Debug)]
pub struct Tree {
    pub root: PathBuf,
    pub show_hidden: bool,
    loaded: BTreeSet<PathBuf>,
    directories: BTreeMap<PathBuf, Vec<Entry>>,
    tags: BTreeMap<PathBuf, Vec<Symbol>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Directory,
    File,
    Tag {
        key: String,
        has_children: bool,
        line: usize,
        column: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeTarget {
    Path(PathBuf),
    Tag(String),
}

#[derive(Clone, Debug)]
pub enum TreeReadError {
    NoTags(PathBuf),
    Other(String),
}

impl std::fmt::Display for TreeReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoTags(path) => write!(f, "No tags found for {}", path.display()),
            Self::Other(message) => write!(f, "{message}"),
        }
    }
}

impl From<String> for TreeReadError {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub path: PathBuf,
    pub target: PathBuf,
    pub kind: NodeKind,
    pub heading: ByteOffset,
}

pub struct RenderedTree {
    pub text: String,
    pub sections: Sections,
    pub nodes: Vec<Node>,
    pub styles: Vec<(Range<ByteOffset>, StyleRole)>,
}

impl Node {
    pub fn is_directory(&self) -> bool {
        matches!(self.kind, NodeKind::Directory)
    }

    pub fn is_tag(&self) -> bool {
        matches!(self.kind, NodeKind::Tag { .. })
    }

    pub fn section_target(&self) -> NodeTarget {
        match &self.kind {
            NodeKind::Tag { key, .. } => NodeTarget::Tag(key.clone()),
            NodeKind::Directory | NodeKind::File => NodeTarget::Path(self.path.clone()),
        }
    }

    pub fn represents(&self, path: &Path) -> bool {
        match self.kind {
            NodeKind::Tag { .. } => false,
            NodeKind::File => self.path == path || self.target == path,
            NodeKind::Directory => {
                self.path == path
                    || self.target == path
                    || (path.starts_with(&self.path) && self.target.starts_with(path))
            }
        }
    }
}

impl RenderedTree {
    pub fn node_for(&self, path: &Path) -> Option<&Node> {
        self.nodes.iter().find(|node| node.represents(path))
    }
}

impl Tree {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            show_hidden: true,
            loaded: BTreeSet::new(),
            directories: BTreeMap::new(),
            tags: BTreeMap::new(),
        }
    }

    pub fn is_loaded(&self, directory: &Path) -> bool {
        self.loaded.contains(directory)
    }

    /// Reads one requested directory and at most three single-child links per
    /// child directory. Other subtrees are never recursively scanned.
    pub fn expand(mut self, directory: &Path, cancelled: &AtomicBool) -> Result<Self, String> {
        self.load(directory, cancelled)?;
        Ok(self)
    }

    pub(crate) fn expand_tags(
        mut self,
        file: &Path,
        open_files: &HashMap<PathBuf, OpenFileOverlay>,
        cancelled: &AtomicBool,
    ) -> Result<Self, TreeReadError> {
        let symbols = if let Some(overlay) = open_files.get(file) {
            outline::extract_from_overlay(overlay, cancelled)?
        } else {
            outline::extract_from_file(file, cancelled)?
        };
        if symbols.is_empty() {
            return Err(TreeReadError::NoTags(file.to_path_buf()));
        }
        self.tags.insert(file.to_path_buf(), symbols);
        Ok(self)
    }

    /// Refreshes loaded paths and their ancestors, including paths split out
    /// of a previously merged chain. Unopened subtrees remain lazy.
    pub fn refresh(self, cancelled: &AtomicBool) -> Result<Self, String> {
        self.refresh_with_overlays(&HashMap::new(), cancelled)
    }

    pub(crate) fn refresh_with_overlays(
        self,
        open_files: &HashMap<PathBuf, OpenFileOverlay>,
        cancelled: &AtomicBool,
    ) -> Result<Self, String> {
        let mut next = Self::new(self.root.clone());
        next.show_hidden = self.show_hidden;
        next.load(&self.root, cancelled)?;
        for directory in self.loaded.iter().filter(|path| *path != &self.root) {
            check_cancelled(cancelled)?;
            let mut ancestors: Vec<_> = directory
                .ancestors()
                .take_while(|path| *path != self.root)
                .collect();
            ancestors.reverse();
            for ancestor in ancestors {
                if next.is_loaded(ancestor) {
                    continue;
                }
                if !next.reachable(ancestor) {
                    break;
                }
                next.load(ancestor, cancelled)?;
            }
        }
        for file in self.tags.keys() {
            check_cancelled(cancelled)?;
            let symbols = if let Some(overlay) = open_files.get(file) {
                outline::extract_from_overlay(overlay, cancelled)
            } else {
                outline::extract_from_file(file, cancelled)
            };
            if let Ok(symbols) = symbols
                && !symbols.is_empty()
            {
                next.tags.insert(file.clone(), symbols);
            }
        }
        Ok(next)
    }

    pub fn locate(mut self, path: &Path, cancelled: &AtomicBool) -> Result<Self, String> {
        if !path.starts_with(&self.root) {
            return Err(format!(
                "{} does not fall under the project",
                path.display()
            ));
        }
        if !self.is_loaded(&self.root) {
            let root = self.root.clone();
            self.load(&root, cancelled)?;
        }
        loop {
            check_cancelled(cancelled)?;
            let rendered = self.render(None, None)?;
            if rendered.node_for(path).is_some() {
                return Ok(self);
            }
            let directory = rendered
                .nodes
                .iter()
                .find(|node| {
                    node.is_directory()
                        && path.starts_with(&node.target)
                        && !self.is_loaded(&node.target)
                })
                .map(|node| node.target.clone());
            let Some(directory) = directory else {
                return Err(format!("Cannot locate {} in the tree", path.display()));
            };
            self.load(&directory, cancelled)?;
        }
    }

    fn reachable(&self, directory: &Path) -> bool {
        let Some(parent) = directory.parent() else {
            return false;
        };
        self.directories.get(parent).is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| entry.directory && entry.path == directory)
        })
    }

    fn load(&mut self, directory: &Path, cancelled: &AtomicBool) -> Result<(), String> {
        let entries = read_directory(directory, self.show_hidden, cancelled)?;
        let children: Vec<_> = entries
            .iter()
            .filter(|entry| entry.directory)
            .map(|entry| entry.path.clone())
            .collect();
        self.directories.insert(directory.to_path_buf(), entries);
        self.loaded.insert(directory.to_path_buf());
        for child in children {
            let mut current = child;
            for _ in 0..=COLLAPSE_DEPTH {
                check_cancelled(cancelled)?;
                let entries = match read_directory(&current, self.show_hidden, cancelled) {
                    Ok(entries) => entries,
                    Err(error) if cancelled.load(Ordering::Relaxed) => return Err(error),
                    Err(_) => break,
                };
                let single =
                    (entries.len() == 1 && entries[0].directory).then(|| entries[0].path.clone());
                self.directories.insert(current, entries);
                let Some(next) = single else { break };
                current = next;
            }
        }
        Ok(())
    }

    fn collapsed_target(&self, path: &Path) -> PathBuf {
        let mut target = path;
        for _ in 0..COLLAPSE_DEPTH {
            let Some(entries) = self.directories.get(target) else {
                break;
            };
            if entries.len() != 1 || !entries[0].directory {
                break;
            }
            target = &entries[0].path;
        }
        target.to_path_buf()
    }

    pub fn render(
        &self,
        previous: Option<&Sections>,
        fold: Option<(&NodeTarget, bool)>,
    ) -> Result<RenderedTree, String> {
        self.render_with_git(previous, fold, None)
    }

    pub fn render_with_git(
        &self,
        previous: Option<&Sections>,
        fold: Option<(&NodeTarget, bool)>,
        git: Option<&TreeGitStatus>,
    ) -> Result<RenderedTree, String> {
        let remembered: HashMap<_, _> = previous
            .into_iter()
            .flat_map(|sections| sections.nodes())
            .map(|node| (node.key.as_str(), node.collapsed))
            .collect();
        let mut projection = Projection {
            tree: self,
            git,
            text: String::new(),
            sections: vec![Section {
                key: "tree-buffer".into(),
                parent: None,
                range: ByteOffset(0)..ByteOffset(0),
                body: ByteOffset(0),
                collapsed: false,
                hunk: false,
            }],
            nodes: Vec::new(),
            styles: Vec::new(),
            remembered,
            fold,
        };
        projection.append(&self.root, &self.root, true, 0, 0);
        projection.text.pop();
        let end = ByteOffset(projection.text.len());
        projection.sections[0].range.end = end;
        for section in &mut projection.sections {
            section.range.end = section.range.end.min(end);
            section.body = section.body.min(end);
        }
        let sections = Sections::new(&Rope::from(projection.text.as_str()), projection.sections)?;
        Ok(RenderedTree {
            text: projection.text,
            sections,
            nodes: projection.nodes,
            styles: projection.styles,
        })
    }
}

struct Projection<'a> {
    tree: &'a Tree,
    git: Option<&'a TreeGitStatus>,
    text: String,
    sections: Vec<Section>,
    nodes: Vec<Node>,
    styles: Vec<(Range<ByteOffset>, StyleRole)>,
    remembered: HashMap<&'a str, bool>,
    fold: Option<(&'a NodeTarget, bool)>,
}

impl Projection<'_> {
    fn append(&mut self, path: &Path, target: &Path, directory: bool, parent: usize, depth: usize) {
        let index = self.sections.len();
        let key = node_key(path);
        let tags = (!directory)
            .then(|| self.tree.tags.get(target).filter(|syms| !syms.is_empty()))
            .flatten();
        let fold_override = self
            .fold
            .filter(|(t, _)| matches!(t, NodeTarget::Path(node) if node == path))
            .map(|(_, collapsed)| collapsed);
        let collapsed = if directory {
            fold_override.unwrap_or_else(|| {
                self.remembered
                    .get(key.as_str())
                    .copied()
                    .unwrap_or(depth != 0 && !self.tree.is_loaded(target))
            })
        } else if tags.is_some() {
            fold_override
                .unwrap_or_else(|| self.remembered.get(key.as_str()).copied().unwrap_or(true))
        } else {
            true
        };
        let start = ByteOffset(self.text.len());
        self.text.push_str(&"  ".repeat(depth));
        let marker_start = ByteOffset(self.text.len());
        if depth != 0 {
            self.text.push_str(if directory {
                if collapsed { "+ " } else { "- " }
            } else {
                " "
            });
        }
        let marker_end = ByteOffset(self.text.len());
        let label = if depth == 0 {
            path.file_name().unwrap_or(path.as_os_str())
        } else {
            target
                .strip_prefix(path.parent().unwrap_or(path))
                .unwrap_or(target)
                .as_os_str()
        };
        let label_start = ByteOffset(self.text.len());
        self.text
            .push_str(&label.to_string_lossy().escape_debug().to_string());
        let label_end = ByteOffset(self.text.len());
        self.text.push('\n');
        if depth == 0 {
            if label_start < label_end {
                self.styles
                    .push((label_start..label_end, StyleRole::TreeRoot));
            }
        } else {
            let state = self
                .git
                .and_then(|git| git.state_for(path, target, directory));
            let label_role = match state {
                Some(GitNodeState::Ignored) => StyleRole::TreeIgnored,
                Some(GitNodeState::Conflict) => StyleRole::TreeGitConflict,
                Some(GitNodeState::Modified) => StyleRole::TreeGitModified,
                Some(GitNodeState::Added) => StyleRole::TreeGitAdded,
                Some(GitNodeState::Untracked) => StyleRole::TreeGitUntracked,
                None => {
                    if directory {
                        StyleRole::TreeDirectory
                    } else {
                        StyleRole::TreeFile
                    }
                }
            };
            if directory && marker_start < marker_end {
                let marker_role = if state == Some(GitNodeState::Ignored) {
                    StyleRole::TreeIgnored
                } else {
                    StyleRole::SyntaxPunctuation
                };
                self.styles.push((marker_start..marker_end, marker_role));
            }
            if label_start < label_end {
                self.styles.push((label_start..label_end, label_role));
            }
        }
        self.nodes.push(Node {
            path: path.to_path_buf(),
            target: target.to_path_buf(),
            kind: if directory {
                NodeKind::Directory
            } else {
                NodeKind::File
            },
            heading: start,
        });
        self.sections.push(Section {
            key,
            parent: Some(parent),
            range: start..start,
            body: ByteOffset(self.text.len()),
            collapsed,
            hunk: false,
        });
        if directory
            && self.tree.loaded.contains(target)
            && let Some(entries) = self.tree.directories.get(target)
        {
            for entry in entries {
                let target = if entry.directory {
                    self.tree.collapsed_target(&entry.path)
                } else {
                    entry.path.clone()
                };
                self.append(&entry.path, &target, entry.directory, index, depth + 1);
            }
        } else if let Some(symbols) = tags {
            self.append_tags(target, symbols, index, depth + 1, "");
        }
        self.sections[index].range.end = ByteOffset(self.text.len());
    }

    fn append_tags(
        &mut self,
        file: &Path,
        symbols: &[Symbol],
        parent: usize,
        depth: usize,
        prefix_path: &str,
    ) {
        let mut counts: HashMap<(&str, &str), usize> = HashMap::new();
        for sym in symbols {
            let entry = counts
                .entry((sym.kind.label(), sym.name.as_str()))
                .or_insert(0);
            let occ = *entry;
            *entry += 1;
            let segment = if prefix_path.is_empty() {
                format!("{}:{}#{}", sym.kind.label(), sym.name, occ)
            } else {
                format!("{prefix_path}/{}:{}#{}", sym.kind.label(), sym.name, occ)
            };
            let key = tag_key(file, &segment);
            let has_children = !sym.children.is_empty();
            let collapsed = if has_children {
                self.fold
                    .filter(|(t, _)| matches!(t, NodeTarget::Tag(k) if k == &key))
                    .map(|(_, collapsed)| collapsed)
                    .unwrap_or_else(|| self.remembered.get(key.as_str()).copied().unwrap_or(true))
            } else {
                false
            };
            let start = ByteOffset(self.text.len());
            self.text.push_str(&"  ".repeat(depth));
            let marker_start = ByteOffset(self.text.len());
            if has_children {
                self.text.push_str(if collapsed { "+ " } else { "- " });
            } else {
                self.text.push_str("· ");
            }
            let marker_end = ByteOffset(self.text.len());
            let label_start = ByteOffset(self.text.len());
            for ch in sym.name.chars() {
                if !ch.is_control() {
                    self.text.push(ch);
                }
            }
            let label_end = ByteOffset(self.text.len());
            self.text.push('\n');
            if marker_start < marker_end {
                self.styles
                    .push((marker_start..marker_end, StyleRole::SyntaxPunctuation));
            }
            if label_start < label_end {
                self.styles
                    .push((label_start..label_end, sym.kind.style_role()));
            }
            self.nodes.push(Node {
                path: file.to_path_buf(),
                target: file.to_path_buf(),
                kind: NodeKind::Tag {
                    key: key.clone(),
                    has_children,
                    line: sym.line,
                    column: sym.column,
                },
                heading: start,
            });
            let section_index = self.sections.len();
            self.sections.push(Section {
                key,
                parent: Some(parent),
                range: start..start,
                body: ByteOffset(self.text.len()),
                collapsed,
                hunk: false,
            });
            if has_children {
                self.append_tags(file, &sym.children, section_index, depth + 1, &segment);
            }
            self.sections[section_index].range.end = ByteOffset(self.text.len());
        }
    }
}

fn read_directory(
    path: &Path,
    show_hidden: bool,
    cancelled: &AtomicBool,
) -> Result<Vec<Entry>, String> {
    check_cancelled(cancelled)?;
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path).map_err(|error| format!("{}: {error}", path.display()))? {
        check_cancelled(cancelled)?;
        let entry = entry.map_err(|error| format!("{}: {error}", path.display()))?;
        let name = entry.file_name();
        if name == ".git"
            || name == ".jj"
            || (!show_hidden && name.as_encoded_bytes().starts_with(b"."))
        {
            continue;
        }
        let path = entry.path();
        let directory = entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
            || path.is_dir();
        entries.push(Entry { path, directory });
    }
    entries.sort_by(|left, right| {
        right
            .directory
            .cmp(&left.directory)
            .then_with(|| left.path.file_name().cmp(&right.path.file_name()))
    });
    Ok(entries)
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("Tree read cancelled".into())
    } else {
        Ok(())
    }
}

pub(crate) fn node_key(path: &Path) -> String {
    use std::fmt::Write;
    let mut key = String::from("tree:");
    for byte in path.as_os_str().as_encoded_bytes() {
        write!(key, "{byte:02x}").unwrap();
    }
    key
}

fn tag_key(path: &Path, segment: &str) -> String {
    use std::fmt::Write;
    let mut key = String::from("tag:");
    for byte in path.as_os_str().as_encoded_bytes() {
        write!(key, "{byte:02x}").unwrap();
    }
    key.push(':');
    for byte in segment.as_bytes() {
        write!(key, "{byte:02x}").unwrap();
    }
    key
}
