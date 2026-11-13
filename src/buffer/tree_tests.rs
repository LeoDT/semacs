use super::*;
use crate::buffer::{Edit, EditError, EditSource};
use crate::text::ByteOffset;
use crate::tree::tests::Fixture;
use std::fs;

fn finish(buffer: &mut Buffer, refresh: bool) {
    let request = buffer.begin_tree_request(refresh).unwrap();
    let serial = request.serial;
    let root = request.root.clone();
    let (data, rendered) = request.read(&AtomicBool::new(false)).unwrap();
    assert!(buffer.install_tree(serial, &root, data, rendered));
}

fn heading(buffer: &Buffer, path: &Path) -> ByteOffset {
    buffer
        .document
        .borrow()
        .tree
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find(|node| node.path == path)
        .unwrap()
        .heading
}

fn current_path(buffer: &Buffer) -> PathBuf {
    let sections = buffer.sections().unwrap();
    let index = sections.current(buffer.point());
    buffer.document.borrow().tree.as_ref().unwrap().nodes[index - 1]
        .path
        .clone()
}

#[test]
fn tree_is_a_read_only_generated_buffer_with_a_shared_session() {
    let fixture = Fixture::new();
    let file = fixture.file("file");
    let mut buffer = Buffer::tree(fixture.0.clone());
    finish(&mut buffer, true);
    assert_eq!(buffer.mode(), crate::mode::ModeId::TREEMACS);
    assert!(buffer.is_read_only());
    assert!(!buffer.is_modified());
    assert_eq!(buffer.path(), None);
    let mut second = buffer.new_view();
    second.set_point(heading(&buffer, &file)).unwrap();
    assert_ne!(buffer.point(), second.point());
    assert_eq!(second.tree_root(), Some(fixture.0.clone()));
    assert_eq!(
        buffer.edit(Edit {
            snapshot: buffer.snapshot_id(),
            range: ByteOffset(0)..ByteOffset(0),
            text: "no",
            source: EditSource::Command,
        }),
        Err(EditError::ReadOnly)
    );
}

#[test]
fn lazy_open_and_close_keep_text_and_section_visibility_consistent() {
    let fixture = Fixture::new();
    let file = fixture.file("dir/child");
    let directory = fixture.0.join("dir");
    let mut buffer = Buffer::tree(fixture.0.clone());
    finish(&mut buffer, true);
    buffer.set_point(heading(&buffer, &directory)).unwrap();
    finish(&mut buffer, false);
    assert!(buffer.text().to_string().contains("- dir\n     child"));
    assert_eq!(buffer.sections().unwrap().visible_lines().len(), 3);
    let mut second = buffer.new_view();
    second.set_point(heading(&buffer, &file)).unwrap();
    finish(&mut buffer, false);
    assert_eq!(buffer.sections().unwrap().visible_lines().len(), 2);
    assert!(buffer.text().to_string().contains("+ dir"));
    assert_eq!(current_path(&second), directory);
    finish(&mut buffer, false);
    assert_eq!(buffer.sections().unwrap().visible_lines().len(), 3);
}

#[test]
fn refresh_preserves_each_views_node_and_existing_folds() {
    let fixture = Fixture::new();
    let first = fixture.file("dir/z");
    let second = fixture.file("other");
    fixture.file("closed/child");
    let directory = fixture.0.join("dir");
    let mut buffer = Buffer::tree(fixture.0.clone());
    finish(&mut buffer, true);
    buffer.set_point(heading(&buffer, &directory)).unwrap();
    finish(&mut buffer, false);
    buffer.set_point(heading(&buffer, &first)).unwrap();
    let mut peer = buffer.new_view();
    peer.set_point(heading(&buffer, &second)).unwrap();
    fixture.file("dir/a");
    finish(&mut buffer, true);
    assert_eq!(current_path(&buffer), first);
    assert_eq!(current_path(&peer), second);
    let text = buffer.text().to_string();
    assert!(text.contains("- dir"));
    assert!(text.contains("+ closed"));
}

#[test]
fn refresh_preserves_open_nodes_when_a_collapsed_chain_splits() {
    let fixture = Fixture::new();
    let file = fixture.file("a/b/c/file");
    let directory = fixture.0.join("a");
    let mut buffer = Buffer::tree(fixture.0.clone());
    finish(&mut buffer, true);
    buffer.set_point(heading(&buffer, &directory)).unwrap();
    finish(&mut buffer, false);
    buffer.set_point(heading(&buffer, &file)).unwrap();
    fixture.file("a/b/new-sibling");
    finish(&mut buffer, true);
    assert_eq!(current_path(&buffer), file);
    let sections = buffer.sections().unwrap();
    assert_eq!(sections.collapse_point(buffer.point()), buffer.point());
    assert!(buffer.text().to_string().contains("- a/b"));
}

#[test]
fn deleted_node_point_moves_to_surviving_parent() {
    let fixture = Fixture::new();
    let file = fixture.file("dir/child");
    let directory = fixture.0.join("dir");
    let mut buffer = Buffer::tree(fixture.0.clone());
    finish(&mut buffer, true);
    buffer.set_point(heading(&buffer, &directory)).unwrap();
    finish(&mut buffer, false);
    buffer.set_point(heading(&buffer, &file)).unwrap();
    fs::remove_file(&file).unwrap();
    finish(&mut buffer, true);
    assert_eq!(current_path(&buffer), directory);
}

#[test]
fn late_results_and_results_after_save_as_cannot_replace_projection() {
    let fixture = Fixture::new();
    fixture.file("one");
    let mut buffer = Buffer::tree(fixture.0.clone());
    let old = buffer.begin_tree_request(true).unwrap();
    let next = buffer.begin_tree_request(true).unwrap();
    let old_serial = old.serial;
    let (data, rendered) = old.read(&AtomicBool::new(false)).unwrap();
    assert!(!buffer.install_tree(old_serial, &fixture.0, data, rendered));
    let serial = next.serial;
    let (data, rendered) = next.read(&AtomicBool::new(false)).unwrap();
    assert!(buffer.install_tree(serial, &fixture.0, data, rendered));
    let saved = buffer.snapshot_id();
    let request = buffer.begin_tree_request(true).unwrap();
    let serial = request.serial;
    let (data, rendered) = request.read(&AtomicBool::new(false)).unwrap();
    buffer.set_path(fixture.0.join("saved-tree"));
    assert!(!buffer.install_tree(serial, &fixture.0, data, rendered));
    assert_eq!(buffer.snapshot_id(), saved);
    assert!(buffer.begin_tree_request(true).is_err());
    assert_eq!(buffer.tree_root(), None);
}

#[test]
fn failed_refresh_keeps_the_previous_text_point_and_projection() {
    let fixture = Fixture::new();
    let file = fixture.file("one");
    let mut buffer = Buffer::tree(fixture.0.clone());
    finish(&mut buffer, true);
    buffer.set_point(heading(&buffer, &file)).unwrap();
    let snapshot = buffer.snapshot_id();
    let point = buffer.point();
    let sections = buffer.sections().unwrap();
    fs::remove_dir_all(&fixture.0).unwrap();
    assert!(
        buffer
            .begin_tree_request(true)
            .unwrap()
            .read(&AtomicBool::new(false))
            .is_err()
    );
    assert_eq!(buffer.snapshot_id(), snapshot);
    assert_eq!(buffer.point(), point);
    assert!(Arc::ptr_eq(&sections, &buffer.sections().unwrap()));
}

#[test]
fn root_can_be_collapsed_and_reopened_without_losing_loaded_nodes() {
    let fixture = Fixture::new();
    fixture.file("one");
    let mut buffer = Buffer::tree(fixture.0.clone());
    finish(&mut buffer, true);
    finish(&mut buffer, false);
    assert_eq!(buffer.sections().unwrap().visible_lines().len(), 1);
    finish(&mut buffer, false);
    assert_eq!(buffer.sections().unwrap().visible_lines().len(), 2);
}

#[test]
fn closing_a_root_moves_another_views_eof_out_of_the_hidden_body() {
    let fixture = Fixture::new();
    fixture.file("last");
    let mut buffer = Buffer::tree(fixture.0.clone());
    finish(&mut buffer, true);
    let mut peer = buffer.new_view();
    peer.set_point(ByteOffset(peer.text().len())).unwrap();
    finish(&mut buffer, false);
    assert_eq!(peer.point(), ByteOffset(0));
    peer.set_point(ByteOffset(peer.text().len())).unwrap();
    assert_eq!(peer.point(), ByteOffset(0));
}

#[test]
fn eof_stays_on_the_last_node_during_refresh() {
    let fixture = Fixture::new();
    let file = fixture.file("last");
    let mut buffer = Buffer::tree(fixture.0.clone());
    finish(&mut buffer, true);
    buffer.set_point(ByteOffset(buffer.text().len())).unwrap();
    finish(&mut buffer, true);
    assert_eq!(current_path(&buffer), file);
}
