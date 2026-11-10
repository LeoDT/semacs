use super::*;
use crate::diff::tests::Repository as Fixture;
use crate::text::ByteOffset;

fn repository(fixture: &Fixture) -> Repository {
    Repository {
        root: fixture.0.clone(),
        git_dir: fixture.0.join(".git"),
    }
}
fn oid(fixture: &Fixture, rev: &str) -> String {
    String::from_utf8(fixture.git(&["rev-parse", rev]))
        .unwrap()
        .trim()
        .into()
}
fn revision(fixture: &Fixture, rev: &str) -> (Data, RenderedHistory) {
    let query = Query::Revision {
        repository: repository(fixture),
        oid: oid(fixture, rev),
        environment: ReadEnvironment::Projection,
    };
    let data = query.load(&AtomicBool::new(false)).unwrap();
    let rendered = data.render(&query).unwrap();
    (data, rendered)
}

fn action(fixture: &Fixture, data: &Data, location: &Location, point: ByteOffset) -> Action {
    let Data::Revision(revision) = data else {
        panic!()
    };
    let query = Query::Revision {
        repository: repository(fixture),
        oid: revision.info.oid.clone(),
        environment: ReadEnvironment::Projection,
    };
    location.action_at(data, &query, point).unwrap()
}

#[test]
fn log_has_graph_metadata_limits_and_immutable_identities() {
    let fixture = Fixture::new();
    fixture.write("a", "base\n");
    fixture.commit();
    fixture.write("a", "next\n");
    fixture.commit();
    let mut query = Query::current(repository(&fixture), false, &AtomicBool::new(false)).unwrap();
    if let Query::Log { limit, .. } = &mut query {
        *limit = 1;
    }
    let Data::Log(data) = query.load(&AtomicBool::new(false)).unwrap() else {
        panic!()
    };
    assert!(data.more);
    assert_eq!(
        data.entries
            .iter()
            .filter(|entry| entry.commit.is_some())
            .count(),
        1
    );
    let commit = data
        .entries
        .iter()
        .find_map(|entry| entry.commit.as_ref())
        .unwrap();
    assert_eq!(commit.oid, oid(&fixture, "HEAD"));
    assert!(!commit.author.is_empty());
    assert!(data.entries[0].graph.contains('*'));
    Data::Log(data).render(&query).unwrap();
}

#[test]
fn root_and_normal_revisions_visit_fixed_blobs_on_both_sides() {
    let fixture = Fixture::new();
    fixture.write("a", "base\n");
    fixture.commit();
    let (_, root) = revision(&fixture, "HEAD");
    assert!(root.text.contains("+base"));
    fixture.write("a", "changed\n");
    fixture.commit();
    let (data, rendered) = revision(&fixture, "HEAD");
    let removed = rendered
        .locations
        .iter()
        .find(|location| {
            rendered.text[location.range.start.0..location.range.end.0].starts_with("-base")
        })
        .unwrap();
    let Action::File {
        default: Some(Visit::Blob { oid: old, .. }),
        versions,
    } = action(&fixture, &data, removed, removed.range.start)
    else {
        panic!()
    };
    assert_eq!(old, oid(&fixture, "HEAD^:a"));
    assert_eq!(versions.len(), 2);
    let added = rendered
        .locations
        .iter()
        .find(|location| {
            rendered.text[location.range.start.0..location.range.end.0].starts_with("+changed")
        })
        .unwrap();
    let Action::File {
        default: Some(Visit::Blob {
            oid: new, column, ..
        }),
        ..
    } = action(&fixture, &data, added, ByteOffset(added.range.start.0 + 4))
    else {
        panic!()
    };
    assert_eq!(column, 3);
    assert_eq!(new, oid(&fixture, "HEAD:a"));
}

pub(crate) fn merge(fixture: &Fixture, count: usize) {
    fixture.write("a", "base\n");
    fixture.commit();
    let base = oid(fixture, "HEAD");
    let mut parents = Vec::new();
    for index in 0..count {
        fixture.git(&["checkout", "--detach", &base]);
        fixture.write("a", format!("parent {index}\n"));
        fixture.commit();
        parents.push(oid(fixture, "HEAD"));
    }
    fixture.write("a", "resolved\n");
    fixture.git(&["add", "a"]);
    let tree = String::from_utf8(fixture.git(&["write-tree"])).unwrap();
    let mut args = vec!["commit-tree", tree.trim(), "-m", "resolution"];
    for parent in &parents {
        args.extend(["-p", parent]);
    }
    let merge = String::from_utf8(fixture.git(&args)).unwrap();
    fixture.git(&["reset", "--hard", merge.trim()]);
}

#[test]
fn combined_keeps_all_parent_paths_blobs_and_result_line_mapping() {
    for count in [2, 3] {
        let fixture = Fixture::new();
        merge(&fixture, count);
        let (data, rendered) = revision(&fixture, "HEAD");
        let Data::Revision(Revision {
            patch: Patch::Combined(files),
            ..
        }) = &data
        else {
            panic!()
        };
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].parents.len(), count);
        assert!(rendered.text.contains("resolved"));
        for location in &rendered.locations {
            let selected = action(&fixture, &data, location, location.range.start);
            if let Action::File {
                default:
                    Some(Visit::Blob {
                        oid: result, line, ..
                    }),
                versions,
            } = &selected
            {
                assert_eq!(*result, oid(&fixture, "HEAD:a"));
                assert_eq!(*line, 1);
                assert_eq!(versions.len(), count + 1);
                for index in 0..count {
                    let Visit::Blob { oid: parent, .. } = &versions[index + 1].visit else {
                        panic!()
                    };
                    assert_eq!(*parent, oid(&fixture, &format!("HEAD^{}:a", index + 1)));
                }
            }
        }
    }
}

#[test]
fn dense_combined_can_omit_all_or_only_some_file_patches() {
    for mixed in [false, true] {
        let fixture = Fixture::new();
        let base_text = (0..40)
            .map(|index| format!("line {index:02}\n"))
            .collect::<String>();
        let shown = "b\tshown\"é";
        fixture.write("a", &base_text);
        if mixed {
            fixture.write(shown, "base\n");
        }
        fixture.commit();
        let base = oid(&fixture, "HEAD");
        fixture.write("a", base_text.replacen("line 00", "main change", 1));
        if mixed {
            fixture.write(shown, "main\n");
        }
        fixture.commit();
        let main = oid(&fixture, "HEAD");
        fixture.git(&["checkout", "--detach", &base]);
        fixture.write("a", base_text.replace("line 39", "side change"));
        if mixed {
            fixture.write(shown, "side\n");
        }
        fixture.commit();
        let side = oid(&fixture, "HEAD");
        fixture.write(
            "a",
            base_text
                .replacen("line 00", "main change", 1)
                .replace("line 39", "side change"),
        );
        if mixed {
            fixture.write(shown, "resolved\n");
        }
        fixture.git(&["add", "--all"]);
        let tree = String::from_utf8(fixture.git(&["write-tree"])).unwrap();
        let merge = String::from_utf8(fixture.git(&[
            "commit-tree",
            tree.trim(),
            "-p",
            &main,
            "-p",
            &side,
            "-m",
            "dense merge",
        ]))
        .unwrap();
        fixture.git(&["reset", "--hard", merge.trim()]);
        let (data, rendered) = revision(&fixture, "HEAD");
        let Data::Revision(Revision {
            patch: Patch::Combined(files),
            ..
        }) = &data
        else {
            panic!()
        };
        assert_eq!(files.len(), if mixed { 2 } else { 1 });
        assert_eq!(files[0].result.path, PathBuf::from("a"));
        assert!(matches!(files[0].body, combined::Body::Omitted));
        assert!(
            rendered
                .text
                .contains("Patch omitted by dense-combined diff")
        );
        let heading = rendered
            .locations
            .iter()
            .find(|location| {
                rendered.text[location.range.start.0..location.range.end.0]
                    .starts_with("Combined a")
            })
            .unwrap();
        let Action::File {
            default: Some(Visit::Blob {
                oid: result, path, ..
            }),
            versions,
        } = action(&fixture, &data, heading, heading.range.start)
        else {
            panic!()
        };
        assert_eq!(path, PathBuf::from("a"));
        assert_eq!(result, oid(&fixture, "HEAD:a"));
        assert_eq!(versions.len(), 3);
        if mixed {
            assert_eq!(files[1].result.path, PathBuf::from(shown));
            assert_eq!(files[1].hunks().len(), 1);
            let line = rendered
                .locations
                .iter()
                .find(|location| {
                    rendered.text[location.range.start.0..location.range.end.0]
                        .starts_with("++resolved")
                })
                .unwrap();
            let Action::File {
                default:
                    Some(Visit::Blob {
                        path, oid: result, ..
                    }),
                ..
            } = action(&fixture, &data, line, line.range.start)
            else {
                panic!()
            };
            assert_eq!(path, PathBuf::from(shown));
            assert_eq!(result, files[1].result.oid);
        }
    }
}

#[test]
fn log_output_encoding_is_utf8_even_with_a_legacy_git_default() {
    let fixture = Fixture::new();
    fixture.git(&["config", "user.name", "Renée"]);
    fixture.git(&["config", "i18n.logOutputEncoding", "ISO-8859-1"]);
    fixture.write("a", "base\n");
    fixture.git(&["add", "a"]);
    fixture.git(&["commit", "-qm", "café"]);
    for select in [false, true] {
        let query = Query::current(repository(&fixture), select, &AtomicBool::new(false)).unwrap();
        let data = query.load(&AtomicBool::new(false)).unwrap();
        let Data::Log(log) = &data else { panic!() };
        let commit = log
            .entries
            .iter()
            .find_map(|entry| entry.commit.as_ref())
            .unwrap();
        assert_eq!(commit.author, "Renée");
        assert_eq!(commit.message, "café");
        assert!(data.render(&query).unwrap().text.contains("café"));
    }
}

#[test]
fn malformed_history_records_and_combined_ranges_fail() {
    assert!(log::parse(b"* \0bad\0").is_err());
    assert!(combined::parse(b"::invalid\0", 2).is_err());
    assert!(
        Query::Revision {
            repository: Repository {
                root: ".".into(),
                git_dir: ".git".into()
            },
            oid: "--all".into(),
            environment: ReadEnvironment::Projection
        }
        .load(&AtomicBool::new(false))
        .is_err()
    );
}

#[test]
fn pending_revision_does_not_change_the_visible_identity_or_cross_environments() {
    let fixture = Fixture::new();
    fixture.write("a", "old\n");
    fixture.commit();
    let old_query = Query::Revision {
        repository: repository(&fixture),
        oid: oid(&fixture, "HEAD"),
        environment: ReadEnvironment::Projection,
    };
    let mut buffer = crate::buffer::Buffer::history(old_query.clone());
    let old_request = buffer.begin_history_request(old_query.clone()).unwrap();
    let data = old_query.load(&AtomicBool::new(false)).unwrap();
    let rendered = data.render(&old_query).unwrap();
    assert!(buffer.install_history(&old_query, old_request, data, rendered));
    let old_text = buffer.text().to_string();
    fixture.write("a", "new\n");
    fixture.commit();
    let new_query = Query::Revision {
        repository: repository(&fixture),
        oid: oid(&fixture, "HEAD"),
        environment: ReadEnvironment::Projection,
    };
    let request = buffer.begin_history_request(new_query.clone()).unwrap();
    assert_eq!(buffer.history_query(), Some(old_query.clone()));
    assert_eq!(
        buffer.history_commit(),
        match old_query {
            Query::Revision { ref oid, .. } => Some(oid.clone()),
            _ => None,
        }
    );
    assert_eq!(buffer.text().to_string(), old_text);
    assert!(!buffer.history_request_matches(&old_query, old_request));
    let data = new_query.load(&AtomicBool::new(false)).unwrap();
    let rendered = data.render(&new_query).unwrap();
    assert!(buffer.install_history(&new_query, request, data, rendered));
    assert_eq!(buffer.history_query(), Some(new_query.clone()));
    let mut commit_query = new_query;
    if let Query::Revision { environment, .. } = &mut commit_query {
        *environment = ReadEnvironment::Commit;
    }
    assert!(buffer.begin_history_request(commit_query).is_err());
}

#[test]
fn history_projection_preserves_points_and_detaches_without_resurrection() {
    let fixture = Fixture::new();
    fixture.write("a", "base\n");
    fixture.commit();
    let query = Query::current(repository(&fixture), false, &AtomicBool::new(false)).unwrap();
    let mut buffer = crate::buffer::Buffer::history(query.clone());
    let request = buffer.begin_history_request(query.clone()).unwrap();
    let data = query.load(&AtomicBool::new(false)).unwrap();
    let rendered = data.render(&query).unwrap();
    assert!(buffer.install_history(&query, request, data, rendered));
    let point = buffer.history_commit_point(None, false).unwrap();
    buffer.set_point(point).unwrap();
    let mut peer = buffer.new_view();
    let request = buffer.begin_history_request(query.clone()).unwrap();
    let data = query.load(&AtomicBool::new(false)).unwrap();
    let rendered = data.render(&query).unwrap();
    assert!(buffer.install_history(&query, request, data, rendered));
    assert_eq!(peer.point(), point);
    peer.detach_history();
    assert!(buffer.repository().is_none());
    assert!(!buffer.history_request_matches(&query, request));
}
