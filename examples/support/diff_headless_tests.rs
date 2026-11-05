#[path = "diff_test_fixture.rs"]
mod fixture;

#[test]
fn production_diff_workload_uses_both_comparisons_and_shared_projection() {
    let fixture = fixture::create();
    let report = fixture.join("test.csv");
    let result = super::run(fixture.clone(), report.clone());
    assert!(
        result.is_ok(),
        "{result:?}; artifacts: {}",
        fixture.display()
    );
    let csv = std::fs::read_to_string(report).unwrap();
    assert!(csv.contains("# completed=true"));
    for kind in ["staged", "unstaged"] {
        assert!(csv.contains(&format!("{kind},refresh,")));
    }
    std::fs::remove_dir_all(fixture).unwrap();
}
