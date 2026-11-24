use super::*;
use crate::command::Command;
use crate::text::ByteOffset;
use crate::tree::tests::Fixture;

fn apply(buffer: &mut Buffer, action: Action) {
    let request = buffer.begin_tree_action(action).unwrap();
    let serial = request.serial;
    let git_version = request.git_version;
    let root = request.root.clone();
    let (data, rendered) = request.read(&AtomicBool::new(false)).unwrap();
    assert!(buffer.install_tree(serial, git_version, &root, data, rendered));
}

fn navigate(buffer: &mut Buffer, command: Command) -> Result<(), String> {
    let (point, action) = buffer.tree_navigation(command)?;
    buffer.set_point(point).unwrap();
    if let Some(action) = action {
        apply(buffer, action);
    }
    Ok(())
}

#[test]
fn tree_navigation_wraps_visible_rows_but_neighbours_stop_at_the_parent() {
    let fixture = Fixture::new();
    let first = fixture.file("a/one");
    fixture.file("a/two");
    fixture.file("b/child");
    let last = fixture.file("z");
    let mut buffer = Buffer::tree(fixture.0.clone());
    apply(&mut buffer, Action::Refresh);
    navigate(&mut buffer, Command::TreemacsPrevious).unwrap();
    assert_eq!(buffer.tree_node().unwrap().path, last);
    navigate(&mut buffer, Command::TreemacsNext).unwrap();
    assert_eq!(buffer.point(), ByteOffset(0));
    navigate(&mut buffer, Command::TreemacsNext).unwrap();
    assert_eq!(buffer.tree_node().unwrap().path, fixture.0.join("a"));
    apply(&mut buffer, Action::Toggle(fixture.0.join("a")));
    navigate(&mut buffer, Command::TreemacsNextNeighbour).unwrap();
    assert_eq!(buffer.tree_node().unwrap().path, fixture.0.join("b"));
    navigate(&mut buffer, Command::TreemacsPreviousNeighbour).unwrap();
    navigate(&mut buffer, Command::TreemacsNext).unwrap();
    assert_eq!(buffer.tree_node().unwrap().path, first);
    let point = buffer.point();
    assert!(navigate(&mut buffer, Command::TreemacsPreviousNeighbour).is_err());
    assert_eq!(buffer.point(), point);
    navigate(&mut buffer, Command::TreemacsNextNeighbour).unwrap();
    let point = buffer.point();
    assert!(navigate(&mut buffer, Command::TreemacsNextNeighbour).is_err());
    assert_eq!(buffer.point(), point);
}

#[test]
fn collapse_parent_and_root_errors_match_navigation_semantics() {
    let fixture = Fixture::new();
    let file = fixture.file("dir/file");
    let mut buffer = Buffer::tree(fixture.0.clone());
    apply(&mut buffer, Action::Locate(file.clone()));
    buffer
        .set_point(buffer.tree_heading(&file).unwrap())
        .unwrap();
    navigate(&mut buffer, Command::TreemacsCollapse).unwrap();
    assert_eq!(buffer.tree_node().unwrap().path, fixture.0.join("dir"));
    navigate(&mut buffer, Command::TreemacsCollapse).unwrap();
    assert_eq!(buffer.tree_node().unwrap().path, fixture.0.join("dir"));
    assert!(buffer.text().to_string().contains("+ dir"));
    navigate(&mut buffer, Command::TreemacsCollapse).unwrap();
    assert_eq!(buffer.point(), ByteOffset(0));
    assert!(navigate(&mut buffer, Command::TreemacsParent).is_err());
    assert!(navigate(&mut buffer, Command::TreemacsCollapseParent).is_err());
    apply(&mut buffer, Action::Locate(file.clone()));
    buffer
        .set_point(buffer.tree_heading(&file).unwrap())
        .unwrap();
    navigate(&mut buffer, Command::TreemacsCollapseParent).unwrap();
    assert_eq!(buffer.tree_node().unwrap().path, fixture.0.join("dir"));
    assert!(buffer.text().to_string().contains("+ dir"));
}

#[test]
fn locate_only_loads_ancestors_and_reveals_them_without_opening_other_branches() {
    let fixture = Fixture::new();
    let file = fixture.file("a/b/c/file");
    fixture.file("other/child/file");
    let mut buffer = Buffer::tree(fixture.0.clone());
    apply(&mut buffer, Action::Refresh);
    apply(&mut buffer, Action::Fold(fixture.0.clone(), true));
    apply(&mut buffer, Action::Locate(file.clone()));
    let heading = buffer.tree_heading(&file).unwrap();
    let sections = buffer.sections().unwrap();
    assert_eq!(sections.collapse_point(heading), heading);
    assert_eq!(sections.visible_lines().len(), 4);
    assert!(buffer.text().to_string().contains("+ other/child"));
    assert!(!buffer.text().to_string().contains("       file"));
    let doc = buffer.document.borrow();
    let data = &doc.tree.as_ref().unwrap().data;
    assert!(data.is_loaded(&fixture.0.join("a/b/c")));
    assert!(!data.is_loaded(&fixture.0.join("other/child")));
}

#[test]
fn hidden_toggle_preserves_loaded_folds_and_failed_locate_does_not_install() {
    let fixture = Fixture::new();
    let hidden = fixture.file(".hidden");
    let file = fixture.file("dir/file");
    let mut buffer = Buffer::tree(fixture.0.clone());
    apply(&mut buffer, Action::Locate(file));
    apply(&mut buffer, Action::Hidden);
    assert!(buffer.tree_heading(&hidden).is_none());
    assert!(buffer.text().to_string().contains("- dir"));
    let text = buffer.text().to_string();
    for path in [
        hidden.clone(),
        fixture.0.join("missing"),
        fixture.0.parent().unwrap().join("outside"),
    ] {
        let request = buffer.begin_tree_action(Action::Locate(path)).unwrap();
        assert!(request.read(&AtomicBool::new(false)).is_err());
        assert_eq!(buffer.text().to_string(), text);
    }
    apply(&mut buffer, Action::Hidden);
    assert!(buffer.tree_heading(&hidden).is_some());
}

#[test]
fn locating_an_intermediate_merged_directory_uses_the_merged_heading() {
    let fixture = Fixture::new();
    fixture.file("a/b/c/file");
    let mut buffer = Buffer::tree(fixture.0.clone());
    apply(&mut buffer, Action::Locate(fixture.0.join("a/b")));
    assert_eq!(
        buffer.tree_heading(&fixture.0.join("a/b")),
        buffer.tree_heading(&fixture.0.join("a"))
    );
    assert_eq!(buffer.sections().unwrap().visible_lines().len(), 2);
}
