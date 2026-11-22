use super::*;
use crate::buffer::Buffer;
use crate::mode::ModeId;
use crate::project::ProjectList;
use gpui_kit as gpui;
use gpui_kit::{Entity, TestAppContext, VisualTestContext, px, size};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "semacs-app-search-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn git_repo(&self, name: &str) -> PathBuf {
        let dir = self.0.join(name);
        fs::create_dir_all(&dir).unwrap();
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["init"])
            .status()
            .expect("git init");
        assert!(status.success());
        dir
    }

    fn file(&self, relative: &str, content: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn open(
    buffer: Buffer,
    startup: PathBuf,
    cx: &mut TestAppContext,
) -> (VisualTestContext, Entity<AppShell>) {
    cx.update(|cx| {
        crate::app::init(cx);
        cx.set_global(ProjectList::default());
        cx.set_global(crate::file_history::FileHistory::default());
    });
    let window = cx.open_window(size(px(900.), px(600.)), |window, cx| {
        let mut shell = AppShell::new(buffer, window, cx);
        shell.startup_directory = startup;
        shell
    });
    let shell = window.entity(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, _| window.activate_window());
    draw(&mut visual);
    (visual, shell)
}

fn draw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.run_until_parked();
}

fn echo(visual: &mut VisualTestContext, shell: &Entity<AppShell>) -> String {
    visual.update(|window, cx| shell.read(cx).echo_message(window, cx))
}

fn wait_search_done(visual: &mut VisualTestContext, shell: &Entity<AppShell>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        draw(visual);
        let running = visual.read(|cx| shell.read(cx).search_work.is_some());
        if !running {
            return;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    panic!("search did not finish within 5s");
}

fn wait_file_open(
    visual: &mut VisualTestContext,
    shell: &Entity<AppShell>,
    expected_path: &Path,
    pane_check: impl Fn(&AppShell, &gpui::App) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        draw(visual);
        let ready = visual.read(|cx| {
            let shell_ref = shell.read(cx);
            shell_ref.opening.is_none()
                && shell_ref.file_task.is_none()
                && shell_ref
                    .buffers()
                    .any(|b| b.path().as_deref() == Some(expected_path))
                && pane_check(shell_ref, cx)
        });
        if ready {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("file {} did not finish opening", expected_path.display());
}

#[gpui::test]
fn project_find_regexp_navigation_and_global_next_error(cx: &mut TestAppContext) {
    let fixture = Fixture::new("project");
    let repo = fixture.git_repo("demo");
    let main_file = fixture.file("demo/src/main.rs", "fn main() {\n    target_sym();\n}\n");
    let lib_file = fixture.file(
        "demo/src/lib.rs",
        "pub fn alpha() {\n    // 中文 target_sym here\n}\npub fn target_sym() {}\n",
    );

    let buffer = Buffer::visiting(
        main_file.clone(),
        crate::files::load(&main_file).unwrap().file,
        None,
    );
    let (mut visual, shell) = open(buffer, repo.clone(), cx);

    // Move point onto `target_sym` in main.rs so default symbol is picked up.
    visual.read(|cx| {
        let view = shell.read(cx).active_view().clone();
        assert_eq!(view.read(cx).buffer().path(), Some(main_file.clone()));
    });
    visual.simulate_keystrokes("ctrl-n ctrl-f ctrl-f ctrl-f ctrl-f");
    draw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .symbol_at_point(),
            Some("target_sym".to_owned())
        );
    });

    // `C-x p g` with empty RET accepts the default symbol `target_sym`.
    visual.simulate_keystrokes("ctrl-x p g");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    wait_search_done(&mut visual, &shell);

    visual.read(|cx| {
        let shell_ref = shell.read(cx);
        let active = shell_ref.active_view().read(cx).buffer();
        assert_eq!(active.mode(), ModeId::RG);
        assert_eq!(active.display_name(), "*rg*");
        let text = active.text().to_string();
        assert!(text.contains("src/main.rs"), "text:\n{text}");
        assert!(text.contains("src/lib.rs"), "text:\n{text}");
        assert!(
            text.contains("rg finished (3 matches in 2 files"),
            "text:\n{text}"
        );
        assert_eq!(shell_ref.panes.ordinary_leaves().len(), 2);
    });

    // `n` moves to first match and displays it in the other pane without leaving `*rg*`.
    visual.simulate_keystrokes("n");
    draw(&mut visual);
    let first_match = visual.read(|cx| {
        let active = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(active.mode(), ModeId::RG);
        active.search_match_at_point().unwrap()
    });
    wait_file_open(&mut visual, &shell, &first_match.path, |s, cx| {
        s.panes.ordinary_leaves().into_iter().any(|p| {
            s.body_panes[&p]
                .active_view()
                .read(cx)
                .buffer()
                .path()
                .as_deref()
                == Some(first_match.path.as_path())
        })
    });
    visual.read(|cx| {
        // Focus stays on `*rg*` after `n`.
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().mode(),
            ModeId::RG
        );
    });

    // `}` jumps to the next file's first match inside `*rg*`, `{` goes back, `M-n` moves without changing other pane.
    visual.simulate_keystrokes("}");
    draw(&mut visual);
    let second_file_match = visual.read(|cx| {
        let active = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(active.mode(), ModeId::RG);
        active.search_match_at_point().unwrap()
    });
    assert_ne!(second_file_match.file_index, first_match.file_index);

    visual.simulate_keystrokes("{");
    draw(&mut visual);
    visual.read(|cx| {
        let m = shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .search_match_at_point()
            .unwrap();
        assert_eq!(m.file_index, first_match.file_index);
    });

    // `}` again, then `C-o` displays match without leaving `*rg*`.
    visual.simulate_keystrokes("}");
    draw(&mut visual);
    visual.simulate_keystrokes("ctrl-o");
    wait_file_open(&mut visual, &shell, &second_file_match.path, |s, cx| {
        s.panes.ordinary_leaves().into_iter().any(|p| {
            s.body_panes[&p]
                .active_view()
                .read(cx)
                .buffer()
                .path()
                .as_deref()
                == Some(second_file_match.path.as_path())
        })
    });
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().mode(),
            ModeId::RG
        );
    });

    // `RET` visits the match in the other pane and selects that code pane.
    visual.simulate_keystrokes("enter");
    wait_file_open(&mut visual, &shell, &second_file_match.path, |s, cx| {
        s.active_view().read(cx).buffer().path().as_deref()
            == Some(second_file_match.path.as_path())
    });
    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(
            buf.path().as_deref(),
            Some(second_file_match.path.as_path())
        );
        let pt = buf.text().offset_to_point(buf.point().0);
        assert_eq!(pt.row as usize + 1, second_file_match.line);
        assert_eq!(pt.column as usize, second_file_match.column);
    });

    // Global `M-g p` steps backward from the code window and updates point.
    visual.simulate_keystrokes("alt-g p");
    draw(&mut visual);
    wait_file_open(&mut visual, &shell, &first_match.path, |s, cx| {
        s.active_view().read(cx).buffer().path().as_deref() == Some(first_match.path.as_path())
    });

    // Global `C-x \`` (`next-error`) steps forward again.
    visual.simulate_keystrokes("ctrl-x `");
    draw(&mut visual);
    wait_file_open(&mut visual, &shell, &second_file_match.path, |s, cx| {
        s.active_view().read(cx).buffer().path().as_deref()
            == Some(second_file_match.path.as_path())
    });

    // Switch back to `*rg*` and press `q` to close the temporary split.
    visual.simulate_keystrokes("ctrl-x o");
    draw(&mut visual);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().mode(),
            ModeId::RG
        );
    });
    visual.simulate_keystrokes("q");
    draw(&mut visual);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).panes.ordinary_leaves().len(), 1);
        assert_ne!(
            shell.read(cx).active_view().read(cx).buffer().mode(),
            ModeId::RG
        );
    });

    // Verify `C-x p p` switch menu `[g]` starts project regexp search.
    visual.simulate_keystrokes("ctrl-x p p");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().unwrap().kind(),
            ReadKind::ProjectSwitchMenu { .. }
        ));
    });
    visual.simulate_keystrokes("g");
    draw(&mut visual);
    visual.simulate_input("alpha");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    wait_search_done(&mut visual, &shell);
    visual.read(|cx| {
        let active = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(active.mode(), ModeId::RG);
        let text = active.text().to_string();
        assert!(text.contains("src/lib.rs"));
        assert!(text.contains("1 match in 1 file"));
    });
    let _ = lib_file;
}

#[gpui::test]
fn directory_rg_and_rerun_keys(cx: &mut TestAppContext) {
    let fixture = Fixture::new("dir");
    let base = fixture.0.join("folder");
    fs::create_dir_all(base.join(".git")).unwrap();
    fixture.file("folder/.gitignore", "ignored.log\n");
    fixture.file(
        "folder/code.rs",
        "fn Hello() {}\nfn hello() {}\nlet x = \"[raw\";\n",
    );
    fixture.file("folder/notes.md", "hello markdown\n");
    fixture.file("folder/ignored.log", "hello ignored\n");
    let other_dir = fixture.0.join("other");
    fs::create_dir_all(&other_dir).unwrap();
    fixture.file("other/only.txt", "hello from other\n");

    let (mut visual, shell) = open(Buffer::scratch(), base.clone(), cx);

    // `M-x rg`: empty query without default stays in minibuffer with hint.
    visual.simulate_keystrokes("alt-x");
    draw(&mut visual);
    visual.simulate_input("rg");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.read(|cx| {
        assert!(shell.read(cx).session().is_some());
    });

    // Enter pattern `hello`, accept prefilled directory `base`.
    visual.simulate_input("hello");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    wait_search_done(&mut visual, &shell);

    // Smart-case lowercase matches Hello + hello in code.rs and hello in notes.md (3 matches, 2 files).
    visual.read(|cx| {
        let text = shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .text()
            .to_string();
        assert!(text.contains("3 matches in 2 files"), "text:\n{text}");
        assert!(!text.contains("ignored.log"));
    });

    // `c` toggles smart -> sensitive: lowercase `hello` skips `Hello` (2 matches in 2 files).
    visual.simulate_keystrokes("c");
    wait_search_done(&mut visual, &shell);
    visual.read(|cx| {
        let text = shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .text()
            .to_string();
        assert!(text.contains("case: sensitive"), "text:\n{text}");
        assert!(text.contains("2 matches in 2 files"), "text:\n{text}");
    });

    // `f` filters to `*.rs` (only code.rs remains: 1 match in 1 file).
    visual.simulate_keystrokes("f");
    draw(&mut visual);
    visual.simulate_input("*.rs");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    wait_search_done(&mut visual, &shell);
    visual.read(|cx| {
        let text = shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .text()
            .to_string();
        assert!(text.contains("files: *.rs"), "text:\n{text}");
        assert!(text.contains("1 match in 1 file"), "text:\n{text}");
    });

    // Clear `f` filter with empty input, then `i` toggles ignore off (includes ignored.log).
    visual.simulate_keystrokes("f");
    draw(&mut visual);
    visual.simulate_keystrokes("ctrl-a ctrl-k enter");
    wait_search_done(&mut visual, &shell);
    visual.simulate_keystrokes("i");
    wait_search_done(&mut visual, &shell);
    visual.read(|cx| {
        let text = shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .text()
            .to_string();
        assert!(text.contains("ignore: off"), "text:\n{text}");
        assert!(text.contains("ignored.log"), "text:\n{text}");
    });

    // `t` switches to literal mode so `[raw` matches literally without regex error.
    visual.simulate_keystrokes("t");
    draw(&mut visual);
    visual.simulate_keystrokes("ctrl-a ctrl-k");
    visual.simulate_input("[raw");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    wait_search_done(&mut visual, &shell);
    visual.read(|cx| {
        let text = shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .text()
            .to_string();
        assert!(text.contains("mode: literal"), "text:\n{text}");
        assert!(text.contains("1 match in 1 file"), "text:\n{text}");
    });

    // `r` switches back to regexp, then `d` changes directory to `other_dir`.
    visual.simulate_keystrokes("r");
    draw(&mut visual);
    visual.simulate_keystrokes("ctrl-a ctrl-k");
    visual.simulate_input("hello");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    wait_search_done(&mut visual, &shell);

    visual.simulate_keystrokes("d");
    draw(&mut visual);
    visual.simulate_keystrokes("ctrl-a ctrl-k");
    visual.simulate_input(&format!("{}/", other_dir.to_string_lossy()));
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    wait_search_done(&mut visual, &shell);
    visual.read(|cx| {
        let text = shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .text()
            .to_string();
        assert!(text.contains("only.txt"), "text:\n{text}");
        assert!(text.contains("1 match in 1 file"), "text:\n{text}");
    });

    // `C-c C-k` when idle reports no active search process.
    visual.simulate_keystrokes("ctrl-c ctrl-k");
    draw(&mut visual);
    assert_eq!(echo(&mut visual, &shell), "No active search process");

    // Fold the file section with TAB, then `g` recompile preserves collapsed state.
    visual.simulate_keystrokes("alt-n");
    draw(&mut visual);
    visual.simulate_keystrokes("tab");
    draw(&mut visual);
    visual.read(|cx| {
        let sections = shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .sections()
            .unwrap();
        assert!(sections.nodes()[1].collapsed);
    });
    visual.simulate_keystrokes("g");
    wait_search_done(&mut visual, &shell);
    visual.read(|cx| {
        let sections = shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .sections()
            .unwrap();
        assert!(sections.nodes()[1].collapsed);
    });

    // Kill `*rg*`, then `M-g n` reports no search buffer.
    visual.simulate_keystrokes("ctrl-x ctrl-k");
    draw(&mut visual);
    visual.simulate_keystrokes("alt-g n");
    draw(&mut visual);
    assert_eq!(echo(&mut visual, &shell), "No search results buffer");
}

#[gpui::test]
fn save_as_detaches_search_buffer(cx: &mut TestAppContext) {
    let fixture = Fixture::new("detach");
    let dir = fixture.0.join("src");
    fs::create_dir_all(&dir).unwrap();
    fixture.file("src/a.txt", "hello world\n");
    let export_path = fixture.0.join("exported-rg.txt");

    let (mut visual, shell) = open(Buffer::scratch(), dir.clone(), cx);
    visual.simulate_keystrokes("alt-x");
    draw(&mut visual);
    visual.simulate_input("rg");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.simulate_input("hello");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    wait_search_done(&mut visual, &shell);

    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(buf.mode(), ModeId::RG);
        assert!(buf.is_read_only());
        assert!(buf.sections().is_some());
    });

    // `C-x C-w` writes `*rg*` to disk and detaches it into a normal editable buffer.
    visual.simulate_keystrokes("ctrl-x ctrl-w");
    draw(&mut visual);
    visual.simulate_keystrokes("ctrl-a ctrl-k");
    visual.simulate_input(&export_path.to_string_lossy());
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        draw(&mut visual);
        let detached = visual.read(|cx| {
            let buf = shell.read(cx).active_view().read(cx).buffer();
            buf.mode() == ModeId::FUNDAMENTAL
                && buf.path().as_deref() == Some(export_path.as_path())
        });
        if detached {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(buf.mode(), ModeId::FUNDAMENTAL);
        assert!(!buf.is_read_only());
        assert!(buf.sections().is_none());
        assert!(!buf.is_search_buffer());
    });
    assert!(export_path.is_file());
}
