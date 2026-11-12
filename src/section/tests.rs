use super::*;

fn tree(hunk: &str) -> (Rope, Sections) {
    let text = Rope::from("root\nfile\nhunk\nbody\nnext\n");
    let section = |key: &str, parent, start, body, end| Section {
        key: key.into(),
        parent,
        range: ByteOffset(start)..ByteOffset(end),
        body: ByteOffset(body),
        collapsed: false,
        hunk: false,
    };
    let nodes = vec![
        section("root", None, 0, 5, 25),
        section("file", Some(0), 5, 10, 20),
        section(hunk, Some(1), 10, 15, 20),
        section("next", Some(0), 20, 25, 25),
    ];
    let sections = Sections::new(&text, nodes).unwrap();
    (text, sections)
}

#[test]
fn parent_folding_preserves_child_state_and_visible_navigation() {
    let (_, mut sections) = tree("hunk");
    sections.toggle(2).unwrap();
    sections.toggle(1).unwrap();
    assert_eq!(sections.collapse_point(ByteOffset(16)), ByteOffset(5));
    assert_eq!(sections.navigate(ByteOffset(5), true), Some(ByteOffset(20)));
    sections.toggle(1).unwrap();
    assert!(sections.nodes()[2].collapsed);
    assert_eq!(sections.collapse_point(ByteOffset(16)), ByteOffset(10));
    sections.reveal(ByteOffset(15));
    assert!(!sections.nodes()[1].collapsed);
    assert!(sections.toggle(0).is_err());
}

#[test]
fn missing_section_falls_back_through_actual_ancestry_and_cached_state_survives_absence() {
    let (text, mut original) = tree("hunk");
    original.toggle(2).unwrap();
    let anchor = original.anchor(&text, ByteOffset(12));
    let (_, mut changed) = tree("different hunk");
    changed.restore(&original);
    assert_eq!(changed.resolve(&text, &anchor), ByteOffset(5));
    assert!(!changed.nodes()[2].collapsed);
    let (_, mut restored) = tree("hunk");
    restored.restore(&changed);
    assert!(restored.nodes()[2].collapsed);
}

#[test]
fn invalid_hierarchies_and_partial_line_boundaries_are_rejected() {
    let (text, sections) = tree("hunk");
    for mutate in [
        |nodes: &mut Vec<Section>| nodes[2].parent = Some(3),
        |nodes: &mut Vec<Section>| nodes[3].range.start = ByteOffset(15),
        |nodes: &mut Vec<Section>| nodes[2].body = ByteOffset(14),
        |nodes: &mut Vec<Section>| nodes[2].key = "file".into(),
    ] {
        let mut nodes = sections.nodes().to_vec();
        mutate(&mut nodes);
        assert!(Sections::new(&text, nodes).is_err());
    }
}

#[test]
fn only_a_hunk_heading_reports_a_pinnable_range() {
    let (_, mut sections) = tree("hunk");
    sections.nodes[2].hunk = true;
    assert_eq!(
        sections.hunk_at(ByteOffset(10)),
        Some(ByteOffset(10)..ByteOffset(20))
    );
    // A hunk body is not its heading, and other sections are not hunks even
    // when their key happens to mention one.
    assert_eq!(sections.hunk_at(ByteOffset(16)), None);
    assert_eq!(sections.hunk_at(ByteOffset(5)), None);
    assert_eq!(sections.hunk_at(ByteOffset(20)), None);
}

#[test]
fn global_cycle_expands_all_then_collapses_only_top_level_sections() {
    let (_, mut sections) = tree("hunk");
    sections.toggle(2).unwrap();
    sections.cycle_global();
    assert!(sections.nodes().iter().skip(1).all(|node| !node.collapsed));

    sections.cycle_global();
    assert!(sections.nodes()[1].collapsed);
    assert!(!sections.nodes()[2].collapsed);
    assert!(!sections.nodes()[3].collapsed);

    sections.cycle_global();
    assert!(sections.nodes().iter().skip(1).all(|node| !node.collapsed));
}
