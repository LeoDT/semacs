use super::*;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile_support::TempDir;

mod tempfile_support {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    pub struct TempDir(pub PathBuf);

    impl TempDir {
        pub fn new(label: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(1);
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "semacs-search-test-{}-{}-{id}",
                std::process::id(),
                label
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[test]
fn json_parser_handles_escapes_surrogates_numbers_and_base64() {
    let input = r#"{"type":"match","data":{"path":{"text":"./dir:1/file.rs"},"lines":{"text":"let s = \"😀中\\n\";\n"},"line_number":42,"submatches":[{"start":9,"end":16}]}}"#;
    let parsed = json::parse(input).unwrap();
    assert_eq!(parsed.get("type").unwrap().as_str(), Some("match"));
    let data = parsed.get("data").unwrap();
    assert_eq!(data.get("line_number").unwrap().as_u64(), Some(42));
    assert_eq!(
        data.get("lines").unwrap().get("text").unwrap().as_str(),
        Some("let s = \"😀中\\n\";\n")
    );

    let surrogate = json::parse(r#""\uD83D\uDE00\u4E2D""#).unwrap();
    assert_eq!(surrogate.as_str(), Some("😀中"));

    assert_eq!(json::decode_base64("aGVsbG8=").unwrap(), b"hello");
}

#[test]
fn query_args_and_glob_parsing() {
    let mut query = SearchQuery::new("-flag".into(), PathBuf::from("/tmp/work"), None);
    assert_eq!(
        query.build_args(),
        vec![
            "--json",
            "--line-number",
            "--column",
            "--smart-case",
            "--",
            "-flag",
            ".",
        ]
    );

    query.literal = true;
    query.case_mode = CaseMode::Sensitive;
    query.respect_ignore = false;
    query.files_glob = Some("*.{rs,toml}, !target/*  src/**/*.{ts,tsx}".into());
    assert_eq!(
        query.build_args(),
        vec![
            "--json",
            "--line-number",
            "--column",
            "--case-sensitive",
            "--fixed-strings",
            "--no-ignore",
            "-g",
            "*.{rs,toml}",
            "-g",
            "!target/*",
            "-g",
            "src/**/*.{ts,tsx}",
            "--",
            "-flag",
            ".",
        ]
    );
}

#[test]
fn duplicate_sanitized_display_names_still_produce_unique_section_keys() {
    let query = SearchQuery::new("x".into(), PathBuf::from("/proj"), None);
    let mut data = SearchData::running(query);
    let make_group = |display: &str, path: &str| FileGroup {
        relative_display: display.to_owned(),
        path: PathBuf::from(path),
        matches: vec![MatchLine {
            line_number: 1,
            match_byte_column: 0,
            display_column: 1,
            text: "x".to_owned(),
            submatches: std::iter::once(0..1).collect(),
        }],
    };
    data.files = vec![
        make_group("foo", "/proj/foo1"),
        make_group("foo", "/proj/foo2"),
        make_group("foo#1", "/proj/foo#1"),
    ];
    data.total_matches = 3;
    let rendered = data.render();
    assert_eq!(rendered.sections.nodes().len(), 4);
}

#[test]
fn render_sections_styles_and_navigation_work_together() {
    let query = SearchQuery::new("needle".into(), PathBuf::from("/proj"), None);
    let mut data = SearchData::running(query);
    let mut file_indices = HashMap::new();
    for line in [
        r#"{"type":"match","data":{"path":{"text":"./a.rs"},"lines":{"text":"fn needle_one() {}\n"},"line_number":3,"submatches":[{"start":3,"end":9}]}}"#,
        r#"{"type":"match","data":{"path":{"text":"./a.rs"},"lines":{"text":"// needle and needle\n"},"line_number":10,"submatches":[{"start":3,"end":9},{"start":14,"end":20}]}}"#,
        r#"{"type":"match","data":{"path":{"text":"./sub/b.rs"},"lines":{"text":"中文 needle\n"},"line_number":7,"submatches":[{"start":7,"end":13}]}}"#,
    ] {
        let val = json::parse(line).unwrap();
        assert!(apply_json_record(&mut data, &mut file_indices, &val).is_some());
    }
    data.status = SearchStatus::Finished {
        elapsed_ms: 12,
        truncated: false,
    };

    let rendered = data.render();
    // Named styles must be strictly ordered and non-overlapping.
    for pair in rendered.styles.windows(2) {
        assert!(pair[0].0.start < pair[0].0.end);
        assert!(pair[0].0.end <= pair[1].0.start);
    }
    assert_eq!(rendered.navigation.matches.len(), 3);
    assert_eq!(rendered.navigation.files.len(), 2);

    // Heading resolves to the first match of that file.
    let first_heading = rendered.navigation.files[0].heading_range.start;
    let at_heading = rendered.navigation.match_at(first_heading).unwrap();
    assert_eq!(at_heading.path, PathBuf::from("/proj/a.rs"));
    assert_eq!(at_heading.line, 3);
    assert_eq!(at_heading.column, 3);

    // Step forward from buffer start visits match 0, 1, 2, then errors.
    let m0 = rendered.navigation.step_match(ByteOffset(0), true).unwrap();
    assert_eq!(m0.line, 3);
    let m1 = rendered.navigation.step_match(m0.target, true).unwrap();
    assert_eq!(m1.line, 10);
    let m2 = rendered.navigation.step_match(m1.target, true).unwrap();
    assert_eq!(m2.path, PathBuf::from("/proj/sub/b.rs"));
    assert_eq!(m2.line, 7);
    assert_eq!(m2.column, 7); // byte offset after "中文 "
    assert_eq!(
        rendered.navigation.step_match(m2.target, true).unwrap_err(),
        "Past last match"
    );

    // Step file forward/backward.
    let next_file = rendered.navigation.step_file(m0.target, true).unwrap();
    assert_eq!(next_file.file_index, 1);
    let prev_file = rendered
        .navigation
        .step_file(next_file.target, false)
        .unwrap();
    assert_eq!(prev_file.file_index, 0);
    assert_eq!(prev_file.line, 3);
}

#[test]
fn streaming_rg_finds_matches_and_respects_gitignore_and_case() {
    let dir = TempDir::new("rg-live");
    std::fs::write(dir.0.join(".gitignore"), "ignored.txt\n").unwrap();
    std::fs::create_dir_all(dir.0.join(".git")).unwrap();
    std::fs::write(
        dir.0.join("alpha.rs"),
        "fn HelloWorld() {}\nfn hello() {}\n",
    )
    .unwrap();
    std::fs::write(dir.0.join("ignored.txt"), "hello ignored\n").unwrap();

    let run = |query: SearchQuery| {
        let controller = Arc::new(SearchController::new());
        let latest = Arc::new(Mutex::new(None));
        let signal = Arc::new(SearchSignal::new());
        run_streaming(query, controller, latest.clone(), signal);
        latest.lock().unwrap().take().unwrap()
    };

    // Smart-case lowercase matches both HelloWorld and hello, ignoring ignored.txt.
    let update = run(SearchQuery::new("hello".into(), dir.0.clone(), None));
    assert!(matches!(update.status, SearchStatus::Finished { .. }));
    assert_eq!(update.total_matches, 2);
    assert_eq!(update.file_count, 1);

    // Smart-case with uppercase matches only HelloWorld.
    let update = run(SearchQuery::new("Hello".into(), dir.0.clone(), None));
    assert_eq!(update.total_matches, 1);

    // Toggling respect_ignore includes ignored.txt.
    let mut query = SearchQuery::new("hello".into(), dir.0.clone(), None);
    query.respect_ignore = false;
    let update = run(query);
    assert_eq!(update.total_matches, 3);
    assert_eq!(update.file_count, 2);

    // No matches exits cleanly (not Failed).
    let update = run(SearchQuery::new(
        "definitely_not_present_xyz".into(),
        dir.0.clone(),
        None,
    ));
    assert!(matches!(update.status, SearchStatus::Finished { .. }));
    assert_eq!(update.total_matches, 0);

    // Invalid regex reports Failed with rg stderr.
    let update = run(SearchQuery::new("[unclosed".into(), dir.0.clone(), None));
    assert!(matches!(update.status, SearchStatus::Failed(_)));

    // Literal mode finds "[unclosed" cleanly without regex error.
    let mut literal_query = SearchQuery::new("[unclosed".into(), dir.0.clone(), None);
    literal_query.literal = true;
    let update = run(literal_query);
    assert!(matches!(update.status, SearchStatus::Finished { .. }));
}
