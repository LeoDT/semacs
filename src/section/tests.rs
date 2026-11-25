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
fn buffer_end_without_a_trailing_empty_line_belongs_to_the_last_section() {
    let (text, sections) = tree("hunk");
    assert_eq!(sections.current(ByteOffset(text.len())), 0);
    let text = Rope::from(text.to_string().trim_end_matches('\n'));
    let mut nodes = sections.nodes().to_vec();
    let end = ByteOffset(text.len());
    for node in &mut nodes {
        node.range.end = node.range.end.min(end);
        node.body = node.body.min(end);
    }
    let mut sections = Sections::new(&text, nodes).unwrap();
    assert_eq!(sections.current(end), 3);
    sections.toggle(1).unwrap();
    assert_eq!(sections.collapse_point(ByteOffset(19)), ByteOffset(5));
    assert_eq!(sections.collapse_point(end), end);
}

#[test]
fn folded_content_end_maps_to_its_heading_without_hiding_a_trailing_empty_line() {
    for ending in ["", "\n"] {
        let text = Rope::from(format!("root\nfile\nbody{ending}").as_str());
        let end = ByteOffset(text.len());
        let mut sections = Sections::new(
            &text,
            vec![
                Section {
                    key: "root".into(),
                    parent: None,
                    range: ByteOffset(0)..end,
                    body: ByteOffset(5),
                    collapsed: false,
                    hunk: false,
                },
                Section {
                    key: "file".into(),
                    parent: Some(0),
                    range: ByteOffset(5)..end,
                    body: ByteOffset(10),
                    collapsed: true,
                    hunk: false,
                },
            ],
        )
        .unwrap();
        let expected = if ending.is_empty() {
            ByteOffset(5)
        } else {
            end
        };
        assert_eq!(sections.collapse_point(end), expected);
        assert_eq!(sections.motion_point(end, true), expected);
        assert_eq!(sections.motion_point(ByteOffset(11), true), expected);
        sections.toggle(1).unwrap();
        assert_eq!(sections.collapse_point(end), end);
    }
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

fn status_like_tree(
    files: &[(&str, &[&str])],
    has_staged: bool,
) -> (Rope, Sections) {
    let mut text = String::from("Head:     main\n");
    let head_range = ByteOffset(0)..ByteOffset(text.len());
    let mut nodes = vec![
        Section {
            key: "root".into(),
            parent: None,
            range: ByteOffset(0)..ByteOffset(0),
            body: ByteOffset(0),
            collapsed: false,
            hunk: false,
        },
        Section {
            key: "header/head".into(),
            parent: Some(0),
            range: head_range.clone(),
            body: head_range.end,
            collapsed: false,
            hunk: false,
        },
    ];
    if !files.is_empty() {
        let g_start = ByteOffset(text.len());
        text.push_str("Unstaged changes\n");
        let g_body = ByteOffset(text.len());
        let g_idx = nodes.len();
        nodes.push(Section {
            key: "unstaged".into(),
            parent: Some(0),
            range: g_start..g_body,
            body: g_body,
            collapsed: false,
            hunk: false,
        });
        for (file, hunks) in files {
            let f_start = ByteOffset(text.len());
            text.push_str(&format!("modified   {file}\n"));
            let f_body = ByteOffset(text.len());
            let f_idx = nodes.len();
            nodes.push(Section {
                key: format!("unstaged/file/{file}"),
                parent: Some(g_idx),
                range: f_start..f_body,
                body: f_body,
                collapsed: false,
                hunk: false,
            });
            for hunk in *hunks {
                let h_start = ByteOffset(text.len());
                text.push_str(&format!("{hunk}\n"));
                let h_body = ByteOffset(text.len());
                text.push_str("+line\n");
                let h_end = ByteOffset(text.len());
                nodes.push(Section {
                    key: format!("unstaged/file/{file}/{hunk}"),
                    parent: Some(f_idx),
                    range: h_start..h_end,
                    body: h_body,
                    collapsed: false,
                    hunk: true,
                });
            }
            nodes[f_idx].range.end = ByteOffset(text.len());
        }
        nodes[g_idx].range.end = ByteOffset(text.len());
    }
    if has_staged {
        let s_start = ByteOffset(text.len());
        text.push_str("Staged changes\n");
        let s_body = ByteOffset(text.len());
        let s_idx = nodes.len();
        nodes.push(Section {
            key: "staged".into(),
            parent: Some(0),
            range: s_start..s_body,
            body: s_body,
            collapsed: false,
            hunk: false,
        });
        let f_start = ByteOffset(text.len());
        text.push_str("modified   done\n");
        let f_end = ByteOffset(text.len());
        nodes.push(Section {
            key: "staged/file/done".into(),
            parent: Some(s_idx),
            range: f_start..f_end,
            body: f_end,
            collapsed: false,
            hunk: false,
        });
        nodes[s_idx].range.end = f_end;
    }
    nodes[0].range.end = ByteOffset(text.len());
    let rope = Rope::from(text.as_str());
    let sections = Sections::new(&rope, nodes).unwrap();
    (rope, sections)
}

#[test]
fn successor_prefers_next_file_same_hunk_index_and_paired_group() {
    let (rope1, sec1) = status_like_tree(&[("a", &["@@ -1 +1 @@", "@@ -10 +10 @@"]), ("b", &["@@ -1 +1 @@"])], true);
    let a_pos = sec1.find_key("unstaged/file/a").unwrap();
    let a_anchor = sec1.anchor(&rope1, a_pos);
    let hunk1_pos = sec1.find_key("unstaged/file/a/@@ -10 +10 @@").unwrap();
    let hunk1_anchor = sec1.anchor(&rope1, hunk1_pos);

    // Staging hunk 1 leaves only hunk 0 in file `a`: point moves to remaining hunk 0.
    let (rope2, sec2) = status_like_tree(&[("a", &["@@ -1 +1 @@"]), ("b", &["@@ -1 +1 @@"])], true);
    assert_eq!(
        sec2.resolve(&rope2, &hunk1_anchor),
        sec2.find_key("unstaged/file/a/@@ -1 +1 @@").unwrap()
    );

    // Staging file `a` moves to next sibling file `b`.
    let (rope3, sec3) = status_like_tree(&[("b", &["@@ -1 +1 @@"])], true);
    assert_eq!(
        sec3.resolve(&rope3, &a_anchor),
        sec3.find_key("unstaged/file/b").unwrap()
    );

    // Staging last unstaged file `b` removes `unstaged` group and moves to paired `staged`.
    let b_anchor = sec3.anchor(&rope3, sec3.find_key("unstaged/file/b").unwrap());
    let (rope4, sec4) = status_like_tree(&[], true);
    assert_eq!(
        sec4.resolve(&rope4, &b_anchor),
        sec4.find_key("staged").unwrap()
    );
}

#[test]
fn sibling_navigation_level_visibility_and_section_cycles_match_magit() {
    let (_, mut sec) = status_like_tree(&[("a", &["@@ -1 +1 @@", "@@ -10 +10 @@"]), ("b", &["@@ -1 +1 @@"])], true);
    let a_pos = sec.find_key("unstaged/file/a").unwrap();
    let b_pos = sec.find_key("unstaged/file/b").unwrap();
    let unstaged_pos = sec.find_key("unstaged").unwrap();
    let staged_pos = sec.find_key("staged").unwrap();

    // M-n on file `a` skips its expanded hunks and lands on file `b`.
    assert_eq!(sec.navigate_sibling(a_pos, true), Some(b_pos));
    assert_eq!(sec.navigate_sibling(b_pos, false), Some(a_pos));
    // M-n on last file `b` falls back to forward visible section (`staged`).
    assert_eq!(sec.navigate_sibling(b_pos, true), Some(staged_pos));

    // Level 2 on `unstaged` expands `unstaged` and collapses files.
    sec.show_level(unstaged_pos, 2);
    assert!(!sec.nodes()[sec.nodes().iter().position(|n| n.key == "unstaged").unwrap()].collapsed);
    assert!(sec.nodes()[sec.nodes().iter().position(|n| n.key == "unstaged/file/a").unwrap()].collapsed);
    assert!(sec.nodes()[sec.nodes().iter().position(|n| n.key == "unstaged/file/b").unwrap()].collapsed);

    // Level 3 on `unstaged` expands files and collapses hunks.
    sec.show_level(unstaged_pos, 3);
    assert!(!sec.nodes()[sec.nodes().iter().position(|n| n.key == "unstaged/file/a").unwrap()].collapsed);
    assert!(sec.nodes()[sec.nodes().iter().position(|n| n.key == "unstaged/file/a/@@ -1 +1 @@").unwrap()].collapsed);

    // Level 4 on `unstaged` expands hunks too.
    sec.show_level(unstaged_pos, 4);
    assert!(!sec.nodes()[sec.nodes().iter().position(|n| n.key == "unstaged/file/a/@@ -1 +1 @@").unwrap()].collapsed);

    // M-1 collapses all top-level sections.
    sec.show_level_all(a_pos, 1);
    assert!(sec.nodes()[sec.nodes().iter().position(|n| n.key == "unstaged").unwrap()].collapsed);
    assert!(sec.nodes()[sec.nodes().iter().position(|n| n.key == "staged").unwrap()].collapsed);

    // C-c TAB on collapsed `unstaged`: step 1 expands `unstaged` with files collapsed;
    // step 2 expands file & hunk headings; step 3 collapses `unstaged`.
    sec.cycle(unstaged_pos);
    assert!(!sec.nodes()[sec.nodes().iter().position(|n| n.key == "unstaged").unwrap()].collapsed);
    assert!(sec.nodes()[sec.nodes().iter().position(|n| n.key == "unstaged/file/a").unwrap()].collapsed);
    sec.cycle(unstaged_pos);
    assert!(!sec.nodes()[sec.nodes().iter().position(|n| n.key == "unstaged/file/a").unwrap()].collapsed);
    sec.cycle(unstaged_pos);
    assert!(sec.nodes()[sec.nodes().iter().position(|n| n.key == "unstaged").unwrap()].collapsed);

    // M-<tab> in status mode: all files/hunks were expanded -> collapses files,
    // then expands file headings (hunks collapsed), then expands hunks, then collapses files.
    sec.cycle_diffs(true);
    assert!(!sec.nodes()[sec.nodes().iter().position(|n| n.key == "unstaged").unwrap()].collapsed);
    assert!(sec.nodes()[sec.nodes().iter().position(|n| n.key == "unstaged/file/a").unwrap()].collapsed);
    sec.cycle_diffs(true);
    assert!(!sec.nodes()[sec.nodes().iter().position(|n| n.key == "unstaged/file/a").unwrap()].collapsed);
    assert!(sec.nodes()[sec.nodes().iter().position(|n| n.key == "unstaged/file/a/@@ -1 +1 @@").unwrap()].collapsed);
    sec.cycle_diffs(true);
    assert!(!sec.nodes()[sec.nodes().iter().position(|n| n.key == "unstaged/file/a/@@ -1 +1 @@").unwrap()].collapsed);
    sec.cycle_diffs(true);
    assert!(sec.nodes()[sec.nodes().iter().position(|n| n.key == "unstaged/file/a").unwrap()].collapsed);
}
