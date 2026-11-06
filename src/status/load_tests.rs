use super::*;
use crate::diff::tests::Repository as Fixture;
use std::sync::atomic::AtomicBool;

fn load(repo: &Fixture) -> Result<Status, String> {
    Status::load(
        Repository {
            root: repo.0.clone(),
            git_dir: repo.0.join(".git"),
        },
        &AtomicBool::new(false),
    )
}

fn tracked() -> Fixture {
    let repo = Fixture::new();
    repo.write("a", "before\n");
    repo.write(".gitattributes", "");
    repo.commit();
    repo
}

#[test]
fn initial_read_rejects_clean_and_process_filters_before_either_can_start() {
    for driver in ["evil", "set", "unset", "unspecified", "Case.Sensitive"] {
        for kind in ["clean", "process"] {
            let repo = tracked();
            repo.write(".gitattributes", format!("a filter={driver}\n"));
            repo.write("a", "after, force content comparison\n");
            repo.git(&[
                "config",
                &format!("filter.{driver}.{kind}"),
                "touch SHOULD_NOT_EXIST; cat",
            ]);
            repo.git(&["config", &format!("filter.{driver}.required"), "true"]);
            let index = std::fs::read(repo.0.join(".git/index")).unwrap();
            let error = load(&repo).unwrap_err();
            assert!(error.contains("external clean/process filter"), "{error}");
            assert!(!repo.0.join("SHOULD_NOT_EXIST").exists());
            assert_eq!(std::fs::read(repo.0.join(".git/index")).unwrap(), index);
        }
    }
}

#[test]
fn configured_but_unused_filters_and_empty_overrides_preserve_builtin_conversion() {
    let repo = tracked();
    repo.git(&[
        "config",
        "filter.unused.clean",
        "touch SHOULD_NOT_EXIST; cat",
    ]);
    repo.write(".gitattributes", "a text eol=lf\n");
    repo.commit();
    repo.write("a", "before\r\n");
    let status = load(&repo).unwrap();
    let oracle = repo.git(&[
        "--no-optional-locks",
        "status",
        "--porcelain=v2",
        "--branch",
        "--no-ahead-behind",
        "-z",
        "--untracked-files=normal",
        "--ignore-submodules=none",
        "--find-renames=50%",
    ]);
    let expected = Status::parse(status.repository().clone(), &oracle).unwrap();
    assert_eq!(status.entries(), expected.entries());
    assert_eq!(
        repo.git(&["hash-object", "--path=a", "a"]),
        repo.git(&["rev-parse", ":a"])
    );
    assert!(!repo.0.join("SHOULD_NOT_EXIST").exists());
    repo.git(&["config", "--add", "filter.unused.clean", ""]);
    repo.write(".gitattributes", "a filter=unused\n");
    repo.write("a", "after\n");
    assert!(
        load(&repo)
            .unwrap()
            .entries()
            .iter()
            .any(|entry| entry.path == std::path::Path::new("a"))
    );
    assert!(!repo.0.join("SHOULD_NOT_EXIST").exists());
}

#[test]
fn attributes_from_index_fallback_info_and_included_configuration_are_screened() {
    let repo = tracked();
    repo.write(".gitattributes", "a filter=evil\n");
    repo.git(&["add", "--", ".gitattributes"]);
    std::fs::remove_file(repo.0.join(".gitattributes")).unwrap();
    repo.write("a", "changed\n");
    repo.write(
        "filter-config",
        "[filter \"evil\"]\nclean = touch SHOULD_NOT_EXIST; cat\n",
    );
    repo.git(&[
        "config",
        "include.path",
        repo.0.join("filter-config").to_str().unwrap(),
    ]);
    assert!(
        load(&repo)
            .unwrap_err()
            .contains("external clean/process filter")
    );
    assert!(!repo.0.join("SHOULD_NOT_EXIST").exists());
    repo.write(".gitattributes", "a -filter\n");
    assert!(load(&repo).is_ok());
    repo.write(".git/info/attributes", "a filter=evil\n");
    assert!(
        load(&repo)
            .unwrap_err()
            .contains("external clean/process filter")
    );
    assert!(!repo.0.join("SHOULD_NOT_EXIST").exists());
}

#[test]
fn submodule_filters_are_screened_before_superproject_status_starts() {
    let source = tracked();
    let repo = tracked();
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
    assert!(load(&repo).unwrap().entries().is_empty());
    repo.write("module/.gitattributes", "a filter=child-only\n");
    repo.write("module/a", "child dirt\n");
    repo.git(&[
        "-C",
        "module",
        "config",
        "filter.child-only.clean",
        "touch SHOULD_NOT_EXIST; cat",
    ]);
    let root_index = std::fs::read(repo.0.join(".git/index")).unwrap();
    let module_git = crate::git::discover(&repo.0.join("module"), &AtomicBool::new(false)).unwrap();
    let child_index = std::fs::read(module_git.git_dir.join("index")).unwrap();
    assert!(
        load(&repo)
            .unwrap_err()
            .contains("external clean/process filter")
    );
    assert!(!repo.0.join("SHOULD_NOT_EXIST").exists());
    assert!(!repo.0.join("module/SHOULD_NOT_EXIST").exists());
    assert_eq!(
        std::fs::read(repo.0.join(".git/index")).unwrap(),
        root_index
    );
    assert_eq!(
        std::fs::read(module_git.git_dir.join("index")).unwrap(),
        child_index
    );
}

#[test]
fn nested_submodules_and_inherited_config_are_screened_without_expanding_status() {
    let leaf = tracked();
    let middle = tracked();
    middle.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "-q",
        "--",
        leaf.0.to_str().unwrap(),
        "nested",
    ]);
    middle.commit();
    let repo = tracked();
    repo.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "-q",
        "--",
        middle.0.to_str().unwrap(),
        "module",
    ]);
    repo.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "update",
        "--init",
        "--recursive",
    ]);
    repo.commit();
    repo.write("module/nested/.gitattributes", "a filter=leaf\n");
    repo.write("module/nested/a", "leaf changed\n");
    repo.git(&[
        "-C",
        "module/nested",
        "config",
        "filter.leaf.process",
        "touch SHOULD_NOT_EXIST; cat",
    ]);
    assert!(
        load(&repo)
            .unwrap_err()
            .contains("external clean/process filter")
    );
    assert!(!repo.0.join("module/nested/SHOULD_NOT_EXIST").exists());
    repo.git(&[
        "-C",
        "module/nested",
        "config",
        "--unset",
        "filter.leaf.process",
    ]);
    let status = load(&repo).unwrap();
    assert_eq!(status.entries().len(), 1);
    assert_eq!(status.entries()[0].path, std::path::Path::new("module"));
    assert!(status.entries()[0].submodule.tracked_dirty);
}

#[cfg(unix)]
#[test]
fn fsmonitor_hooks_and_diff_helpers_cannot_execute_in_parent_or_child() {
    use std::os::unix::fs::PermissionsExt;
    let source = tracked();
    let repo = tracked();
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
    let module = crate::git::discover(&repo.0.join("module"), &AtomicBool::new(false)).unwrap();
    for (path, git_dir) in [
        (&repo.0, repo.0.join(".git")),
        (&repo.0.join("module"), module.git_dir),
    ] {
        let monitor = path.join("monitor");
        std::fs::write(&monitor, "#!/bin/sh\ntouch SHOULD_NOT_EXIST\nexit 1\n").unwrap();
        std::fs::set_permissions(&monitor, std::fs::Permissions::from_mode(0o755)).unwrap();
        repo.git(&[
            "-C",
            path.to_str().unwrap(),
            "config",
            "core.fsmonitor",
            monitor.to_str().unwrap(),
        ]);
        repo.git(&[
            "-C",
            path.to_str().unwrap(),
            "config",
            "diff.evil.command",
            "touch SHOULD_NOT_EXIST",
        ]);
        repo.git(&[
            "-C",
            path.to_str().unwrap(),
            "config",
            "diff.evil.textconv",
            "touch SHOULD_NOT_EXIST",
        ]);
        std::fs::write(path.join(".gitattributes"), "a diff=evil\n").unwrap();
        std::fs::write(path.join("a"), "change\n").unwrap();
        for name in ["pre-commit", "post-index-change"] {
            let hook = git_dir.join("hooks").join(name);
            std::fs::write(&hook, "#!/bin/sh\ntouch SHOULD_NOT_EXIST\n").unwrap();
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    assert!(load(&repo).is_ok());
    assert!(!repo.0.join("SHOULD_NOT_EXIST").exists());
    assert!(!repo.0.join("module/SHOULD_NOT_EXIST").exists());
}

#[cfg(unix)]
#[test]
fn submodule_replaced_by_symlink_does_not_execute_the_target_repository_filters() {
    let source = tracked();
    let repo = tracked();
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
    std::fs::remove_dir_all(repo.0.join("module")).unwrap();
    source.write(".gitattributes", "a filter=evil\n");
    source.write("a", "after!\n");
    source.git(&["config", "filter.evil.clean", "touch SHOULD_NOT_EXIST; cat"]);
    std::os::unix::fs::symlink(&source.0, repo.0.join("module")).unwrap();
    assert!(load(&repo).unwrap_err().contains("symbolic link"));
    assert!(!source.0.join("SHOULD_NOT_EXIST").exists());
    assert!(!repo.0.join("SHOULD_NOT_EXIST").exists());
}

#[test]
fn command_specific_config_override_cannot_hide_repository_filters_child() {
    if std::env::var_os("SEMACS_STATUS_CONFIG_CHILD").is_none() {
        return;
    }
    for kind in ["clean", "process"] {
        let repo = tracked();
        repo.write(".gitattributes", "a filter=local\n");
        repo.git(&[
            "config",
            &format!("filter.local.{kind}"),
            "touch SHOULD_NOT_EXIST; cat",
        ]);
        repo.write("a", "after!\n");
        assert_eq!(
            std::fs::read(repo.0.join("a")).unwrap().len(),
            b"before\n".len()
        );
        let error = load(&repo).unwrap_err();
        assert!(error.contains("external clean/process filter"), "{error}");
        assert!(!repo.0.join("SHOULD_NOT_EXIST").exists());
    }
}

#[test]
fn git_config_override_does_not_diverge_preflight_from_status_configuration() {
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "status::load_tests::command_specific_config_override_cannot_hide_repository_filters_child", "--nocapture"])
        .env("SEMACS_STATUS_CONFIG_CHILD", "1")
        .env("GIT_CONFIG", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output().unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn inherited_git_config_is_screened_while_repository_redirects_are_removed_child() {
    if std::env::var_os("SEMACS_STATUS_ENV_CHILD").is_none() {
        return;
    }
    let repo = tracked();
    repo.write(".gitattributes", "a filter=inherited\n");
    repo.write("a", "changed\n");
    let error = load(&repo).unwrap_err();
    assert!(error.contains("external clean/process filter"), "{error}");
    assert!(error.contains(repo.0.to_str().unwrap()));
    assert!(!repo.0.join("SHOULD_NOT_EXIST").exists());
}

#[test]
fn inherited_git_environment_cannot_redirect_preflight_or_skip_filter_screening() {
    let decoy = tracked();
    let index = std::fs::read(decoy.0.join(".git/index")).unwrap();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "status::load_tests::inherited_git_config_is_screened_while_repository_redirects_are_removed_child", "--nocapture"])
        .env("SEMACS_STATUS_ENV_CHILD", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_COUNT", "1").env("GIT_CONFIG_KEY_0", "filter.inherited.clean")
        .env("GIT_CONFIG_VALUE_0", "touch SHOULD_NOT_EXIST; cat")
        .env("GIT_DIR", decoy.0.join(".git")).env("GIT_COMMON_DIR", decoy.0.join(".git"))
        .env("GIT_WORK_TREE", &decoy.0).env("GIT_INDEX_FILE", decoy.0.join(".git/index"))
        .env("GIT_OBJECT_DIRECTORY", decoy.0.join(".git/objects"))
        .output().unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(std::fs::read(decoy.0.join(".git/index")).unwrap(), index);
    assert!(!decoy.0.join("SHOULD_NOT_EXIST").exists());
}

#[test]
fn stale_repository_cancellation_and_existing_lock_never_write_or_fall_back() {
    let repo = tracked();
    let child = repo.0.join("child");
    std::fs::create_dir(&child).unwrap();
    let nested = Fixture(child);
    nested.git(&["init", "-q"]);
    let repository = crate::git::discover(&nested.0, &AtomicBool::new(false)).unwrap();
    std::fs::remove_dir_all(nested.0.join(".git")).unwrap();
    assert!(Status::load(repository, &AtomicBool::new(false)).is_err());
    repo.write(".git/index.lock", "user lock");
    assert!(load(&repo).is_ok());
    assert_eq!(
        std::fs::read(repo.0.join(".git/index.lock")).unwrap(),
        b"user lock"
    );
    assert!(
        Status::load(
            Repository {
                root: repo.0.clone(),
                git_dir: repo.0.join(".git")
            },
            &AtomicBool::new(true)
        )
        .unwrap_err()
        .contains("cancelled")
    );
}
