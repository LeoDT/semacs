use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::Buffer;
use crate::section::Sections;
use crate::tree::{Node, RenderedTree, Tree};

#[cfg(test)]
#[path = "tree_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tree_navigation_tests.rs"]
mod navigation_tests;

pub(super) struct TreeSession {
    data: Arc<Tree>,
    nodes: Vec<Node>,
    request: u64,
}

#[derive(Clone)]
pub(crate) enum Action {
    Refresh,
    Toggle(PathBuf),
    Fold(PathBuf, bool),
    Hidden,
    Locate(PathBuf),
}

pub(crate) struct Request {
    pub serial: u64,
    pub root: PathBuf,
    data: Arc<Tree>,
    previous: Option<Arc<Sections>>,
    action: ReadAction,
}

enum ReadAction {
    Refresh,
    Hidden,
    Fold(Node, bool),
    Locate(PathBuf),
}

impl Request {
    pub fn read(self, cancelled: &AtomicBool) -> Result<(Arc<Tree>, RenderedTree), String> {
        let data = match &self.action {
            ReadAction::Locate(path) => {
                Arc::new(self.data.as_ref().clone().locate(path, cancelled)?)
            }
            ReadAction::Refresh | ReadAction::Hidden => {
                let mut data = self.data.as_ref().clone();
                if matches!(self.action, ReadAction::Hidden) {
                    data.show_hidden = !data.show_hidden;
                }
                Arc::new(data.refresh(cancelled)?)
            }
            ReadAction::Fold(node, false) if !self.data.is_loaded(&node.target) => {
                Arc::new(self.data.as_ref().clone().expand(&node.target, cancelled)?)
            }
            ReadAction::Fold(..) => self.data,
        };
        let mut rendered = data.render(
            self.previous.as_deref(),
            match &self.action {
                ReadAction::Fold(node, collapsed) => Some((node.path.as_path(), *collapsed)),
                _ => None,
            },
        )?;
        if let ReadAction::Locate(path) = self.action {
            let heading = rendered
                .node_for(&path)
                .ok_or("Tree node no longer exists")?
                .heading;
            rendered.sections.reveal(heading);
            rendered = data.render(Some(&rendered.sections), None)?;
        }
        Ok((data, rendered))
    }
}

impl Buffer {
    pub(crate) fn tree(root: PathBuf) -> Self {
        let mut buffer = Self::named("*Treemacs*");
        let data = Arc::new(Tree::new(root));
        let rendered = data
            .render(None, None)
            .expect("valid initial tree projection");
        buffer.set_mode(crate::mode::ModeId::TREEMACS);
        buffer.set_read_only(true);
        buffer.replace_projection(&rendered.text, rendered.sections, Vec::new());
        buffer.document.borrow_mut().tree = Some(TreeSession {
            data,
            nodes: rendered.nodes,
            request: 0,
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
                if session.nodes[current - 1].directory && !nodes[current].collapsed =>
            {
                return Ok((
                    nodes[current].range.start,
                    Some(Action::Fold(session.nodes[current - 1].path.clone(), true)),
                ));
            }
            TreemacsCollapse | TreemacsParent | TreemacsCollapseParent => parent()?,
            _ => return Err("Not a tree navigation command".into()),
        };
        let fold = (command == TreemacsCollapseParent)
            .then(|| Action::Fold(session.nodes[target - 1].path.clone(), true));
        Ok((nodes[target].range.start, fold))
    }

    pub(crate) fn tree_action(&self, refresh: bool) -> Result<Action, String> {
        if refresh {
            return Ok(Action::Refresh);
        }
        let node = self.tree_node()?;
        if !node.directory {
            return Err(format!("No tags found for {}", node.path.display()));
        }
        Ok(Action::Toggle(node.path.clone()))
    }

    #[cfg(test)]
    fn begin_tree_request(&self, refresh: bool) -> Result<Request, String> {
        self.begin_tree_action(self.tree_action(refresh)?)
    }

    pub(crate) fn begin_tree_action(&self, action: Action) -> Result<Request, String> {
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
            Action::Toggle(ref path) | Action::Fold(ref path, _) => {
                let sections = previous.as_ref().ok_or("Tree is not ready")?;
                let (index, node) = session
                    .nodes
                    .iter()
                    .enumerate()
                    .find(|(_, node)| &node.path == path)
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
            action,
        })
    }

    pub(crate) fn install_tree(
        &mut self,
        serial: u64,
        root: &Path,
        data: Arc<Tree>,
        rendered: RenderedTree,
    ) -> bool {
        let valid = {
            let doc = self.document.borrow();
            doc.path.is_none()
                && doc.mode == crate::mode::ModeId::TREEMACS
                && doc
                    .tree
                    .as_ref()
                    .is_some_and(|session| session.request == serial && session.data.root == root)
                && data.root == root
        };
        if !valid {
            return false;
        }
        self.replace_projection(&rendered.text, rendered.sections, Vec::new());
        let mut doc = self.document.borrow_mut();
        let session = doc.tree.as_mut().unwrap();
        session.data = data;
        session.nodes = rendered.nodes;
        true
    }
}
