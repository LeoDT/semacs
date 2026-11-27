use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::Buffer;
use crate::outline::OpenFileOverlay;
use crate::section::Sections;
use crate::tree::{Node, NodeKind, NodeTarget, RenderedTree, Tree, TreeGitStatus, TreeReadError};

#[cfg(test)]
#[path = "tree_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tree_navigation_tests.rs"]
mod navigation_tests;

pub(super) struct TreeSession {
    data: Arc<Tree>,
    nodes: Vec<Node>,
    git: Option<Arc<TreeGitStatus>>,
    git_version: u64,
    request: u64,
    git_serial: u64,
}

#[derive(Clone)]
pub(crate) enum Action {
    Refresh,
    Toggle(NodeTarget),
    Fold(NodeTarget, bool),
    Hidden,
    Locate(PathBuf),
}

pub(crate) struct Request {
    pub serial: u64,
    pub root: PathBuf,
    data: Arc<Tree>,
    previous: Option<Arc<Sections>>,
    git: Option<Arc<TreeGitStatus>>,
    pub git_version: u64,
    open_files: HashMap<PathBuf, OpenFileOverlay>,
    action: ReadAction,
}

enum ReadAction {
    Refresh,
    Hidden,
    Fold(Node, bool),
    Locate(PathBuf),
}

impl Request {
    pub fn read(self, cancelled: &AtomicBool) -> Result<(Arc<Tree>, RenderedTree), TreeReadError> {
        let data = match &self.action {
            ReadAction::Locate(path) => {
                Arc::new(self.data.as_ref().clone().locate(path, cancelled)?)
            }
            ReadAction::Refresh | ReadAction::Hidden => {
                let mut data = self.data.as_ref().clone();
                if matches!(self.action, ReadAction::Hidden) {
                    data.show_hidden = !data.show_hidden;
                }
                Arc::new(data.refresh_with_overlays(&self.open_files, cancelled)?)
            }
            ReadAction::Fold(node, false)
                if node.is_directory() && !self.data.is_loaded(&node.target) =>
            {
                Arc::new(self.data.as_ref().clone().expand(&node.target, cancelled)?)
            }
            ReadAction::Fold(node, false) if !node.is_directory() && !node.is_tag() => {
                Arc::new(self.data.as_ref().clone().expand_tags(
                    &node.target,
                    &self.open_files,
                    cancelled,
                )?)
            }
            ReadAction::Fold(..) => self.data,
        };
        let fold_target = match &self.action {
            ReadAction::Fold(node, collapsed) => Some((node.section_target(), *collapsed)),
            _ => None,
        };
        let mut rendered = data.render_with_git(
            self.previous.as_deref(),
            fold_target.as_ref().map(|(target, c)| (target, *c)),
            self.git.as_deref(),
        )?;
        if let ReadAction::Locate(path) = self.action {
            let heading = rendered
                .node_for(&path)
                .ok_or_else(|| TreeReadError::Other("Tree node no longer exists".into()))?
                .heading;
            rendered.sections.reveal(heading);
            rendered = data.render_with_git(Some(&rendered.sections), None, self.git.as_deref())?;
        }
        Ok((data, rendered))
    }
}

impl Buffer {
    pub(crate) fn tree(root: PathBuf) -> Self {
        let mut buffer = Self::named("*Treemacs*");
        let data = Arc::new(Tree::new(root));
        let rendered = data
            .render_with_git(None, None, None)
            .expect("valid initial tree projection");
        buffer.set_mode(crate::mode::ModeId::TREEMACS);
        buffer.set_read_only(true);
        buffer.replace_projection(&rendered.text, rendered.sections, rendered.styles);
        buffer.document.borrow_mut().tree = Some(TreeSession {
            data,
            nodes: rendered.nodes,
            git: None,
            git_version: 0,
            request: 0,
            git_serial: 0,
        });
        buffer
    }

    pub(crate) fn tree_root(&self) -> Option<PathBuf> {
        let doc = self.document.borrow();
        doc.path
            .is_none()
            .then(|| doc.tree.as_ref().map(|session| session.data.root.clone()))
            .flatten()
    }

    pub(crate) fn tree_node(&self) -> Result<Node, String> {
        let doc = self.document.borrow();
        if doc.path.is_some() {
            return Err("Tree buffer was saved as a file".into());
        }
        let session = doc.tree.as_ref().ok_or("Not a tree buffer")?;
        let sections = doc.sections.as_ref().ok_or("Tree is not ready")?;
        session
            .nodes
            .get(sections.current(self.point()).saturating_sub(1))
            .cloned()
            .ok_or_else(|| "No tree node here".into())
    }

    pub(crate) fn tree_heading(&self, path: &Path) -> Option<crate::text::ByteOffset> {
        let doc = self.document.borrow();
        if doc.path.is_some() {
            return None;
        }
        doc.tree
            .as_ref()?
            .nodes
            .iter()
            .find(|node| node.represents(path))
            .map(|node| node.heading)
    }

    pub(crate) fn tree_navigation(
        &self,
        command: crate::command::Command,
    ) -> Result<(crate::text::ByteOffset, Option<Action>), String> {
        use crate::command::Command::*;
        let doc = self.document.borrow();
        if doc.path.is_some() {
            return Err("Tree buffer was saved as a file".into());
        }
        let session = doc.tree.as_ref().ok_or("Not a tree buffer")?;
        let sections = doc.sections.as_ref().ok_or("Tree is not ready")?;
        let current = sections.current(self.point()).max(1);
        let nodes = sections.nodes();
        let visible: Vec<_> = (1..nodes.len())
            .filter(|&i| sections.collapse_point(nodes[i].range.start) == nodes[i].range.start)
            .collect();
        let position = visible.iter().position(|&i| i == current).unwrap_or(0);
        let parent = || {
            nodes[current]
                .parent
                .filter(|&i| i != 0)
                .ok_or_else(|| "There is no parent to move up to.".to_string())
        };
        let target = match command {
            TreemacsNext => visible[(position + 1) % visible.len()],
            TreemacsPrevious => visible[(position + visible.len() - 1) % visible.len()],
            TreemacsNextNeighbour | TreemacsPreviousNeighbour => {
                let siblings: Vec<_> = visible
                    .iter()
                    .copied()
                    .filter(|&i| nodes[i].parent == nodes[current].parent)
                    .collect();
                let position = siblings.iter().position(|&i| i == current).unwrap_or(0);
                if command == TreemacsNextNeighbour {
                    siblings.get(position + 1).copied()
                } else {
                    position.checked_sub(1).map(|i| siblings[i])
                }
                .ok_or("No neighbour in this direction")?
            }
            TreemacsCollapse
                if (session.nodes[current - 1].is_directory()
                    || matches!(
                        session.nodes[current - 1].kind,
                        NodeKind::Tag {
                            has_children: true,
                            ..
                        }
                    )
                    || (!session.nodes[current - 1].is_tag()
                        && nodes[current].body < nodes[current].range.end))
                    && !nodes[current].collapsed =>
            {
                return Ok((
                    nodes[current].range.start,
                    Some(Action::Fold(
                        session.nodes[current - 1].section_target(),
                        true,
                    )),
                ));
            }
            TreemacsCollapse | TreemacsParent | TreemacsCollapseParent => parent()?,
            _ => return Err("Not a tree navigation command".into()),
        };
        let fold = (command == TreemacsCollapseParent)
            .then(|| Action::Fold(session.nodes[target - 1].section_target(), true));
        Ok((nodes[target].range.start, fold))
    }

    pub(crate) fn tree_action(&self, refresh: bool) -> Result<Option<Action>, String> {
        if refresh {
            return Ok(Some(Action::Refresh));
        }
        let node = self.tree_node()?;
        match &node.kind {
            NodeKind::Directory => Ok(Some(Action::Toggle(NodeTarget::Path(node.path)))),
            NodeKind::File => {
                if !crate::syntax::supports_path(&node.target) {
                    return Err(format!("No tags found for {}", node.path.display()));
                }
                Ok(Some(Action::Toggle(NodeTarget::Path(node.path))))
            }
            NodeKind::Tag {
                key, has_children, ..
            } => {
                if *has_children {
                    Ok(Some(Action::Toggle(NodeTarget::Tag(key.clone()))))
                } else {
                    Ok(None)
                }
            }
        }
    }

    #[cfg(test)]
    fn begin_tree_request(&self, refresh: bool) -> Result<Request, String> {
        let action = self
            .tree_action(refresh)?
            .ok_or_else(|| "Nothing to toggle".to_owned())?;
        self.begin_tree_action(action, HashMap::new())
    }

    pub(crate) fn begin_tree_action(
        &self,
        action: Action,
        open_files: HashMap<PathBuf, OpenFileOverlay>,
    ) -> Result<Request, String> {
        let mut doc = self.document.borrow_mut();
        if doc.path.is_some() {
            return Err("Tree buffer was saved as a file".into());
        }
        let previous = doc.sections.clone();
        let session = doc.tree.as_mut().ok_or("Not a tree buffer")?;
        let action = match action {
            Action::Refresh => ReadAction::Refresh,
            Action::Hidden => ReadAction::Hidden,
            Action::Locate(path) => ReadAction::Locate(path),
            Action::Toggle(ref target) | Action::Fold(ref target, _) => {
                let sections = previous.as_ref().ok_or("Tree is not ready")?;
                let (index, node) = session
                    .nodes
                    .iter()
                    .enumerate()
                    .find(|(_, node)| node.section_target() == *target)
                    .ok_or("Tree node no longer exists")?;
                let collapsed = if let Action::Fold(_, collapsed) = action {
                    collapsed
                } else {
                    !sections.nodes()[index + 1].collapsed
                };
                ReadAction::Fold(node.clone(), collapsed)
            }
        };
        session.request = session
            .request
            .checked_add(1)
            .expect("tree request identity exhausted");
        Ok(Request {
            serial: session.request,
            root: session.data.root.clone(),
            data: session.data.clone(),
            previous,
            git: session.git.clone(),
            git_version: session.git_version,
            open_files,
            action,
        })
    }

    pub(crate) fn begin_tree_git_request(&self) -> Option<(PathBuf, u64)> {
        let mut doc = self.document.borrow_mut();
        if doc.path.is_some() || doc.mode != crate::mode::ModeId::TREEMACS {
            return None;
        }
        let session = doc.tree.as_mut()?;
        session.git_serial = session
            .git_serial
            .checked_add(1)
            .expect("tree git request identity exhausted");
        Some((session.data.root.clone(), session.git_serial))
    }

    pub(crate) fn install_tree_git_status(
        &mut self,
        serial: u64,
        root: &Path,
        status: Option<Arc<TreeGitStatus>>,
    ) -> bool {
        let (data, previous) = {
            let doc = self.document.borrow();
            if doc.path.is_some() || doc.mode != crate::mode::ModeId::TREEMACS {
                return false;
            }
            let Some(session) = doc.tree.as_ref() else {
                return false;
            };
            if session.git_serial != serial || session.data.root != root {
                return false;
            }
            (session.data.clone(), doc.sections.clone())
        };
        let Ok(rendered) = data.render_with_git(previous.as_deref(), None, status.as_deref())
        else {
            return false;
        };
        self.replace_projection(&rendered.text, rendered.sections, rendered.styles);
        let mut doc = self.document.borrow_mut();
        let session = doc.tree.as_mut().unwrap();
        session.nodes = rendered.nodes;
        session.git = status;
        session.git_version += 1;
        true
    }

    pub(crate) fn install_tree(
        &mut self,
        serial: u64,
        git_version: u64,
        root: &Path,
        data: Arc<Tree>,
        mut rendered: RenderedTree,
    ) -> bool {
        let current_git = {
            let doc = self.document.borrow();
            let valid = doc.path.is_none()
                && doc.mode == crate::mode::ModeId::TREEMACS
                && doc
                    .tree
                    .as_ref()
                    .is_some_and(|session| session.request == serial && session.data.root == root)
                && data.root == root;
            if !valid {
                return false;
            }
            let session = doc.tree.as_ref().unwrap();
            (session.git_version != git_version).then(|| session.git.clone())
        };
        if let Some(git) = current_git
            && let Ok(re_rendered) =
                data.render_with_git(Some(&rendered.sections), None, git.as_deref())
        {
            rendered = re_rendered;
        }
        self.replace_projection(&rendered.text, rendered.sections, rendered.styles);
        let mut doc = self.document.borrow_mut();
        let session = doc.tree.as_mut().unwrap();
        session.data = data;
        session.nodes = rendered.nodes;
        true
    }
}
