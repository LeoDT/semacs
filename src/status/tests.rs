use super::*;
use crate::diff::tests::Repository as Fixture;
use crate::text::{ByteOffset, Rope};
use std::path::Path;
use std::sync::atomic::AtomicBool;

fn repository() -> Repository {
    Repository {
        root: "/fixture/worktree".into(),
        git_dir: "/fixture/git-dir".into(),
    }
}

fn protocol(records: &[u8]) -> Vec<u8> {
    let mut bytes = format!("# branch.oid {}\0# branch.head main\0", "1".repeat(40)).into_bytes();
    bytes.extend_from_slice(records);
    bytes
}

fn ordinary(xy: &str, path: &str) -> String {
    format!(
        "1 {xy} N... 100644 100644 100644 {} {} {path}\0",
        "1".repeat(40),
        "2".repeat(40)
    )
}

fn fixture_status(repo: &Fixture) -> Status {
    Status::load(
        Repository {
            root: repo.0.clone(),
            git_dir: repo.0.join(".git"),
        },
        &AtomicBool::new(false),
    )
    .unwrap()
}

#[test]
fn clean_unborn_detached_and_sha256_headers() {
    let clean = Status::parse(repository(), &protocol(b"")).unwrap();
    assert!(clean.entries().is_empty());
    assert_eq!(
        clean.head(),
        &Head::Branch {
            name: b"main".to_vec(),
            oid: "1".repeat(40)
        }
    );
    let rendered = clean.render().unwrap();
    assert_eq!(
        rendered.text,
        format!(
            "Status: /fixture/worktree\nBranch: main\nHEAD: {}\nUntracked files (0)\nUnstaged changes (0)\nStaged changes (0)\nUnmerged paths (0)\n",
            "1".repeat(40)
        )
    );
    assert_eq!(rendered.sections.nodes().len(), 5);
    let unborn = Status::parse(
        repository(),
        b"# branch.oid (initial)\0# branch.head main\0",
    )
    .unwrap();
    assert_eq!(
        unborn.head(),
        &Head::Unborn {
            name: b"main".to_vec()
        }
    );
    assert!(unborn.render().unwrap().text.contains("HEAD: (unborn)"));
    let mut bytes = format!(
        "# branch.head (detached)\0# branch.oid {}\0",
        "a".repeat(64)
    )
    .into_bytes();
    bytes.extend_from_slice(b"# future.optional ignored\0");
    let detached = Status::parse(repository(), &bytes).unwrap();
    assert_eq!(
        detached.head(),
        &Head::Detached {
            oid: "a".repeat(64)
        }
    );
    assert!(
        detached
            .render()
            .unwrap()
            .text
            .contains("Branch: (detached)")
    );
}

#[test]
fn semantic_styles_separate_labels_counts_branches_and_hashes() {
    use crate::theme::StyleRole;
    let rendered = Status::parse(
        repository(),
        &protocol(ordinary("MM", "文件.txt").as_bytes()),
    )
    .unwrap()
    .render()
    .unwrap();
    let spans: Vec<_> = rendered
        .styles
        .iter()
        .map(|(range, role)| (&rendered.text[range.start.0..range.end.0], *role))
        .collect();
    assert!(spans.contains(&("main", StyleRole::MagitBranch)));
    assert!(spans.contains(&("Unstaged changes", StyleRole::SectionHeading)));
    assert!(
        spans
            .iter()
            .any(|(text, role)| text.len() == 40 && *role == StyleRole::MagitHash)
    );
    assert!(
        spans
            .iter()
            .all(|(text, _)| !text.contains("(1)") && !text.contains("文件.txt"))
    );
    assert!(
        rendered
            .styles
            .windows(2)
            .all(|pair| pair[0].0.end <= pair[1].0.start)
    );
}

#[test]
fn groups_preserve_record_order_and_do_not_count_conflicts_twice() {
    let records = format!(
        "{}? directory/\0{}u UU N... 100644 100644 100644 100644 {} {} {} conflict\0? -leading file\0",
        ordinary("MM", "both"),
        ordinary(".D", "deleted"),
        "1".repeat(40),
        "2".repeat(40),
        "3".repeat(40)
    );
    let status = Status::parse(repository(), &protocol(records.as_bytes())).unwrap();
    let paths = |group| {
        status
            .group(group)
            .map(|(_, entry)| entry.path.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        paths(Group::Untracked),
        [PathBuf::from("directory"), "-leading file".into()]
    );
    assert_eq!(
        paths(Group::Unstaged),
        [PathBuf::from("both"), "deleted".into()]
    );
    assert_eq!(paths(Group::Staged), [PathBuf::from("both")]);
    assert_eq!(paths(Group::Unmerged), [PathBuf::from("conflict")]);
    let rendered = status.render().unwrap();
    assert!(
        rendered
            .text
            .contains("Untracked files (2)\n ?? directory/\n ?? -leading file\n")
    );
    assert!(rendered.text.contains("Unmerged paths (1)\n UU conflict\n"));
    for (group, expected) in [
        (Group::Untracked, 2),
        (Group::Unstaged, 2),
        (Group::Staged, 1),
        (Group::Unmerged, 1),
    ] {
        let parent = rendered
            .sections
            .nodes()
            .iter()
            .position(|node| node.key == group.key())
            .unwrap();
        assert_eq!(
            rendered
                .sections
                .nodes()
                .iter()
                .filter(|node| node.parent == Some(parent))
                .count(),
            expected
        );
    }
}

#[test]
fn real_git_covers_unborn_two_sides_deletion_rename_and_recreated_path() {
    let repo = Fixture::new();
    assert!(matches!(fixture_status(&repo).head(), Head::Unborn { .. }));
    repo.write("both", "base\n");
    repo.write("delete", "delete\n");
    repo.write("rename old", "preserve this unique content\n");
    repo.write("recreated", "tracked content\n");
    repo.commit();
    assert!(fixture_status(&repo).entries().is_empty());
    repo.write("both", "staged\n");
    repo.git(&["add", "--", "both"]);
    repo.write("both", "worktree\n");
    std::fs::remove_file(repo.0.join("delete")).unwrap();
    repo.git(&["mv", "rename old", "rename new"]);
    repo.git(&["rm", "-q", "--", "recreated"]);
    repo.write("recreated", "untracked replacement\n");
    std::fs::create_dir(repo.0.join("directory")).unwrap();
    repo.write("directory/child", "not expanded\n");
    let status = fixture_status(&repo);
    assert_eq!(status.group(Group::Unstaged).count(), 2);
    assert_eq!(status.group(Group::Staged).count(), 3);
    assert_eq!(status.group(Group::Untracked).count(), 2);
    let both = status
        .entries()
        .iter()
        .find(|entry| entry.path == Path::new("both"))
        .unwrap();
    assert_eq!(both.xy, *b"MM");
    let Record::Tracked {
        state,
        rename: None,
    } = &both.record
    else {
        panic!()
    };
    let indexed = repo.git(&["rev-parse", ":both"]);
    assert_eq!(
        state.index_oid.as_bytes(),
        indexed.strip_suffix(b"\n").unwrap()
    );
    let rename = status
        .entries()
        .iter()
        .find(|entry| entry.path == Path::new("rename new"))
        .unwrap();
    assert!(
        matches!(&rename.record, Record::Tracked { rename: Some(Rename { source, score: 100, copy: false }), .. } if source == Path::new("rename old"))
    );
    assert_eq!(
        status
            .entries()
            .iter()
            .filter(|entry| entry.path == Path::new("recreated"))
            .count(),
        2
    );
    assert!(
        status
            .render()
            .unwrap()
            .text
            .contains("rename old → rename new")
    );
    repo.git(&["checkout", "-q", "--detach"]);
    assert!(matches!(
        fixture_status(&repo).head(),
        Head::Detached { .. }
    ));
}

#[test]
fn real_conflict_is_one_unmerged_record() {
    let repo = Fixture::new();
    repo.write("conflict", "base\n");
    repo.commit();
    repo.git(&["checkout", "-qb", "other"]);
    repo.write("conflict", "other\n");
    repo.commit();
    repo.git(&["checkout", "-q", "-"]);
    repo.write("conflict", "current\n");
    repo.commit();
    let result = repo
        .command()
        .args(["merge", "--no-edit", "other"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    let status = fixture_status(&repo);
    assert_eq!(status.entries().len(), 1);
    assert_eq!(status.entries()[0].xy, *b"UU");
    assert!(
        matches!(&status.entries()[0].record, Record::Unmerged { modes, oids } if *modes == [0o100644; 4] && oids.iter().all(|oid| oid.len() == 40))
    );
    assert_eq!(status.group(Group::Unmerged).count(), 1);
    assert_eq!(status.group(Group::Staged).count(), 0);
    assert_eq!(status.group(Group::Unstaged).count(), 0);
}

#[cfg(unix)]
#[test]
fn real_binary_symlink_mode_type_change_and_raw_paths() {
    use std::os::unix::{
        ffi::OsStringExt,
        fs::{PermissionsExt, symlink},
    };
    let repo = Fixture::new();
    repo.write("binary", b"old\0binary");
    repo.write("mode", "same\n");
    repo.write("type", "regular\n");
    symlink("old target", repo.0.join("link")).unwrap();
    let raw = PathBuf::from(std::ffi::OsString::from_vec(b"raw-\xff\n\t\\".to_vec()));
    repo.write(&raw, "before\n");
    repo.commit();
    repo.write("binary", b"new\0binary");
    std::fs::set_permissions(repo.0.join("mode"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::remove_file(repo.0.join("type")).unwrap();
    symlink("target", repo.0.join("type")).unwrap();
    std::fs::remove_file(repo.0.join("link")).unwrap();
    symlink("new target", repo.0.join("link")).unwrap();
    repo.write(&raw, "after\n");
    let status = fixture_status(&repo);
    assert_eq!(status.entries().len(), 5);
    let entry = status
        .entries()
        .iter()
        .find(|entry| entry.path == raw)
        .unwrap();
    assert_eq!(entry.xy, *b".M");
    let mode = status
        .entries()
        .iter()
        .find(|entry| entry.path == Path::new("mode"))
        .unwrap();
    assert!(matches!(
        &mode.record,
        Record::Tracked {
            state: Tracked {
                head_mode: 0o100644,
                index_mode: 0o100644,
                worktree_mode: 0o100755,
                ..
            },
            ..
        }
    ));
    let typed = status
        .entries()
        .iter()
        .find(|entry| entry.path == Path::new("type"))
        .unwrap();
    assert_eq!(typed.xy, *b".T");
    let rendered = status.render().unwrap();
    assert!(rendered.text.contains("raw-\\xff\\n\\t\\\\"));
    assert!(!rendered.text.contains('�'));
}

#[test]
fn real_submodule_dirty_flags_and_intent_to_add_are_not_normalized_away() {
    let source = Fixture::new();
    source.write("tracked", "base\n");
    source.commit();
    let repo = Fixture::new();
    repo.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "-q",
        "--",
        source.0.to_str().unwrap(),
        "module",
    ]);
    repo.commit();
    repo.write("module/tracked", "dirty\n");
    repo.write("module/untracked", "new\n");
    repo.write("intent", "not yet staged\n");
    repo.git(&["add", "-N", "--", "intent"]);
    let status = fixture_status(&repo);
    let module = status
        .entries()
        .iter()
        .find(|entry| entry.path == Path::new("module"))
        .unwrap();
    assert_eq!(
        module.submodule,
        Submodule {
            is_submodule: true,
            commit_changed: false,
            tracked_dirty: true,
            untracked_dirty: true
        }
    );
    let intent = status
        .entries()
        .iter()
        .find(|entry| entry.path == Path::new("intent"))
        .unwrap();
    assert_eq!(intent.xy, *b".A");
    assert!(matches!(
        &intent.record,
        Record::Tracked {
            state: Tracked {
                head_mode: 0,
                index_mode: 0,
                ..
            },
            ..
        }
    ));
    repo.git(&["-C", "module", "add", "--", "tracked"]);
    repo.git(&[
        "-C",
        "module",
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "commit",
        "-qm",
        "module change",
    ]);
    let status = fixture_status(&repo);
    let module = status
        .entries()
        .iter()
        .find(|entry| entry.path == Path::new("module"))
        .unwrap();
    assert_eq!(
        module.submodule,
        Submodule {
            is_submodule: true,
            commit_changed: true,
            tracked_dirty: false,
            untracked_dirty: true
        }
    );
}

proptest::proptest! {
    #[test]
    fn arbitrary_status_bytes_never_panic(records in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..1024)) {
        if let Ok(status) = Status::parse(repository(), &protocol(&records)) {
            let rendered = status.render().unwrap();
            proptest::prop_assert!(rendered.text.len() <= MAX_BYTES);
            proptest::prop_assert!(status.entries().len() <= MAX_RECORDS);
        }
    }
}

#[test]
fn copy_and_submodule_protocol_fields_are_preserved_without_queries() {
    let records = format!(
        "2 C. N... 100644 100644 100644 {} {} C85 copied\0original\01 .M SCMU 160000 160000 160000 {} {} submodule\0",
        "1".repeat(40),
        "2".repeat(40),
        "1".repeat(40),
        "2".repeat(40)
    );
    let status = Status::parse(repository(), &protocol(records.as_bytes())).unwrap();
    assert!(
        matches!(&status.entries()[0].record, Record::Tracked { rename: Some(Rename { source, copy: true, score: 85 }), .. } if source == Path::new("original"))
    );
    assert_eq!(
        status.entries()[1].submodule,
        Submodule {
            is_submodule: true,
            commit_changed: true,
            tracked_dirty: true,
            untracked_dirty: true
        }
    );
    assert!(
        status
            .render()
            .unwrap()
            .text
            .contains("submodule [submodule CMU]")
    );
}

#[test]
fn section_leaves_are_noop_and_restore_uses_group_and_raw_path() {
    let status = Status::parse(repository(), &protocol(ordinary("MM", "both").as_bytes())).unwrap();
    let mut rendered = status.render().unwrap();
    let leaf = rendered
        .sections
        .nodes()
        .iter()
        .position(|node| node.key.starts_with("unstaged/file/"))
        .unwrap();
    let point = rendered.sections.nodes()[leaf].range.start;
    let location = rendered.location(ByteOffset(point.0 + 2)).unwrap();
    assert_eq!((location.group, location.entry), (Group::Unstaged, 0));
    let before = rendered.sections.visible_lines();
    rendered.sections.toggle(leaf).unwrap();
    assert_eq!(rendered.sections.visible_lines(), before);
    assert!(!rendered.sections.nodes()[leaf].collapsed);
    assert!(rendered.sections.toggle(0).is_err());
    let parent = rendered.sections.nodes()[leaf].parent.unwrap();
    let heading = rendered.sections.nodes()[parent].range.start;
    let anchor = rendered
        .sections
        .anchor(&Rope::from(rendered.text.as_str()), point);
    rendered.sections.toggle(parent).unwrap();
    assert_eq!(rendered.sections.collapse_point(point), heading);
    assert!(rendered.location(point).is_none());
    let mut next = Status::parse(repository(), &protocol(b"? unrelated\0"))
        .unwrap()
        .render()
        .unwrap();
    next.sections.restore(&rendered.sections);
    let parent = next
        .sections
        .nodes()
        .iter()
        .find(|node| node.key == "unstaged")
        .unwrap();
    assert!(parent.collapsed);
    assert_eq!(
        next.sections
            .resolve(&Rope::from(next.text.as_str()), &anchor),
        parent.range.start
    );
    for node in next
        .sections
        .nodes()
        .iter()
        .filter(|node| node.parent.is_none() || node.parent == Some(0))
    {
        assert!(next.location(node.range.start).is_none());
    }
    assert!(next.location(ByteOffset(next.text.len())).is_none());
}

#[cfg(unix)]
#[test]
fn escaped_labels_cannot_collide_with_raw_identity_keys() {
    let status = Status::parse(
        repository(),
        &protocol(b"? a\xff\0? a\\xff\0? a\xef\xbf\xbd\0? line\n# branch.oid forged\0"),
    )
    .unwrap();
    let rendered = status.render().unwrap();
    let keys: std::collections::HashSet<_> = rendered
        .sections
        .nodes()
        .iter()
        .map(|node| &node.key)
        .collect();
    assert_eq!(keys.len(), rendered.sections.nodes().len());
    assert!(rendered.text.contains("a\\xff\n"));
    assert!(rendered.text.contains("a\\\\xff\n"));
    assert!(rendered.text.contains("a�\n"));
    assert!(rendered.text.contains("line\\n# branch.oid forged"));
    assert_eq!(
        rendered
            .sections
            .nodes()
            .iter()
            .filter(|node| node.key.contains("/file/"))
            .count(),
        4
    );
}

#[test]
fn malformed_records_paths_headers_and_truncation_fail_closed() {
    let id = "1".repeat(40);
    for records in [
        b"".as_slice(),
        b"# branch.head main\0",
        b"# branch.oid (initial)\0",
        b"# branch.oid short\0# branch.head main\0",
        b"# branch.oid (initial)\0# branch.head (detached)\0",
        b"# branch.oid (initial)\0# branch.head main\0# branch.head other\0",
    ] {
        assert!(Status::parse(repository(), records).is_err(), "{records:?}");
    }
    for records in [
        "random\0".into(),
        "! ignored\0".into(),
        "? \0".into(),
        "? /absolute\0".into(),
        "? ../outside\0".into(),
        "? a/../outside\0".into(),
        "? a/./b\0".into(),
        "? a//b\0".into(),
        "? a\0? a\0".into(),
        ordinary("MM", "same") + &ordinary(".M", "same"),
        ordinary("UU", "conflict"),
        ordinary("AA", "conflict"),
        ordinary("DD", "conflict"),
        ordinary("R.", "rename"),
        ordinary("..", "clean"),
        ordinary(".Z", "bad"),
        ordinary(".M", "a").replace("N...", "NCMU"),
        ordinary(".M", "a").replace("100644", "BADMOD"),
        ordinary(".M", "a").replace(&id, "bad"),
        format!("2 R. N... 100644 100644 100644 {id} {id} R101 a\0b\0"),
        format!("2 R. N... 100644 100644 100644 {id} {id} C50 a\0b\0"),
        format!("2 R. N... 100644 100644 100644 {id} {id} R50 a\0"),
        format!("u MM N... 100644 100644 100644 100644 {id} {id} {id} a\0"),
        ordinary(".M", "a").replace(&id, &"a".repeat(64)),
    ] {
        assert!(
            Status::parse(repository(), &protocol(records.as_bytes())).is_err(),
            "{records:?}"
        );
    }
    assert!(
        Status::parse(
            repository(),
            format!("# branch.oid {}\0# branch.head main\0", "0".repeat(40)).as_bytes()
        )
        .is_err()
    );
    let bytes = protocol(ordinary("MM", "a").as_bytes());
    for end in 1..bytes.len() {
        if bytes[end - 1] != 0 {
            assert!(
                Status::parse(repository(), &bytes[..end]).is_err(),
                "truncation at {end}"
            );
        }
    }
}

#[test]
fn limits_fail_instead_of_returning_partial_models_or_projections() {
    let mut bytes = protocol(b"");
    for index in 0..MAX_RECORDS {
        bytes.extend_from_slice(format!("? file-{index}\0").as_bytes());
    }
    let status = Status::parse(repository(), &bytes).unwrap();
    assert_eq!(status.entries().len(), MAX_RECORDS);
    assert_eq!(
        status.render().unwrap().sections.nodes().len(),
        MAX_RECORDS + 5
    );
    bytes.extend_from_slice(b"? too-many\0");
    assert!(
        Status::parse(repository(), &bytes)
            .unwrap_err()
            .contains("4096")
    );
    assert!(
        Status::parse(repository(), &vec![b'x'; MAX_BYTES + 1])
            .unwrap_err()
            .contains("input limit")
    );
    let mut expanding = protocol(b"? ");
    expanding.resize(MAX_BYTES - 1, b'\t');
    expanding.push(0);
    let status = Status::parse(repository(), &expanding).unwrap();
    assert!(status.render().unwrap_err().contains("projection limit"));
}

#[test]
fn repository_discovery_pins_canonical_worktree_and_gitdir_and_rejects_bare() {
    let repo = Fixture::new();
    let cancelled = AtomicBool::new(false);
    std::fs::create_dir(repo.0.join("inside")).unwrap();
    let direct = crate::git::discover(&repo.0, &cancelled).unwrap();
    assert_eq!(
        crate::git::discover(&repo.0.join("inside"), &cancelled).unwrap(),
        direct
    );
    assert_eq!(direct.root, repo.0.canonicalize().unwrap());
    assert_eq!(direct.git_dir, repo.0.join(".git").canonicalize().unwrap());
    #[cfg(unix)]
    {
        let alias = repo.0.join("alias");
        std::os::unix::fs::symlink(&repo.0, &alias).unwrap();
        assert_eq!(crate::git::discover(&alias, &cancelled).unwrap(), direct);
    }
    let bare = repo.0.join("bare.git");
    repo.git(&["init", "--bare", "-q", bare.to_str().unwrap()]);
    assert!(crate::git::discover(&bare, &cancelled).is_err());
    assert!(crate::git::discover(&repo.0, &AtomicBool::new(true)).is_err());
}
