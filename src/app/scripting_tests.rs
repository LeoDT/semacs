use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use gpui_kit as gpui;
use gpui_kit::{Entity, InputHandler, TestAppContext, VisualTestContext, px, size};

use super::AppShell;
use crate::appearance::FontSettings;
use crate::buffer::Buffer;
use crate::command::Command;
use crate::files::{DecodedFile, FileFormat};
use crate::view::BufferView;

struct Fixture(PathBuf);

impl Fixture {
    fn new(source: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".pi/tmp")
            .join(format!(
                "script-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&path).unwrap();
        let fixture = Self(path.join("init.js"));
        fixture.write(source);
        fixture
    }

    fn write(&self, source: &str) {
        std::fs::write(&self.0, source).unwrap();
    }

    fn load(&self) -> Result<crate::scripting::LoadedConfiguration, String> {
        gpui::block_on(crate::scripting::load(self.0.clone()))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(self.0.parent().unwrap()).unwrap();
    }
}

fn open(
    fixture: &Fixture,
    cx: &mut TestAppContext,
) -> (VisualTestContext, Entity<AppShell>, Entity<BufferView>) {
    cx.background_executor.allow_parking();
    let loaded = fixture.load();
    cx.update(crate::app::init);
    let path = fixture.0.clone();
    let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
        AppShell::with_configuration(
            Buffer::new(
                None,
                DecodedFile {
                    text: "abc def".into(),
                    format: FileFormat::default(),
                },
            ),
            Some(path),
            loaded,
            window,
            cx,
        )
    });
    let shell = window.entity(cx).unwrap();
    let view = shell.read_with(cx, |shell, _| shell.active_view().clone());
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
    });
    (visual, shell, view)
}

fn wait(visual: &mut VisualTestContext, shell: &Entity<AppShell>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        visual.run_until_parked();
        if visual.read(|cx| {
            let shell = shell.read(cx);
            shell.script_task.is_none() && shell.reload_task.is_none()
        }) {
            break;
        }
        assert!(Instant::now() < deadline, "script worker did not complete");
        std::thread::sleep(Duration::from_millis(1));
    }
    visual.update(|window, cx| window.draw(cx).clear(cx));
}

fn select(visual: &mut VisualTestContext) {
    visual.simulate_keystrokes("ctrl-a ctrl-t ctrl-f ctrl-f ctrl-f");
}

fn text(visual: &mut VisualTestContext, view: &Entity<BufferView>) -> String {
    visual.read(|cx| view.read(cx).buffer().text().to_string())
}

#[gpui::test]
fn themes_apply_at_startup_and_reload_atomically(cx: &mut TestAppContext) {
    use crate::theme::{StyleRole, ThemeSettings};
    let fixture = Fixture::new(
        r##"
        semacs.defineTheme("night", {appearance:"dark",styles:{
            default:{foreground:"#112233",background:"#223344"}
        }});
        semacs.loadTheme("night");
        semacs.setStyle("cursor",{background:"#ffcc66"});
    "##,
    );
    let (mut visual, shell, view) = open(&fixture, cx);
    let original = visual.read(|cx| {
        let settings = ThemeSettings::global(cx);
        assert_eq!(settings.active(), "night");
        assert_eq!(
            settings.style(StyleRole::Default).foreground,
            gpui::rgb(0x112233).into()
        );
        settings.clone()
    });
    fixture
        .write(r##"semacs.loadTheme("light"); semacs.setStyle("cursor",{background:"invalid"})"##);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.reload_configuration(&view, window, cx)
        })
    });
    wait(&mut visual, &shell);
    visual.read(|cx| assert_eq!(ThemeSettings::global(cx), &original));

    visual.simulate_keystrokes("alt-x");
    visual.simulate_input("load-theme");
    visual.simulate_keystrokes("enter");
    visual.read(|cx| {
        assert!(
            shell
                .read(cx)
                .session()
                .unwrap()
                .candidates()
                .unwrap()
                .items
                .iter()
                .any(|item| item.candidate.value == "night")
        );
    });
    fixture.write("");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.reload_configuration(&view, window, cx)
        })
    });
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(ThemeSettings::global(cx), &ThemeSettings::default());
        assert!(
            !shell
                .read(cx)
                .session()
                .unwrap()
                .candidates()
                .unwrap()
                .items
                .iter()
                .any(|item| item.candidate.value == "night")
        );
    });
    visual.simulate_keystrokes("ctrl-g");
}

#[gpui::test]
fn invalid_startup_theme_does_not_partially_apply_font_or_colors(cx: &mut TestAppContext) {
    use crate::theme::ThemeSettings;
    let fixture = Fixture::new("semacs.setFont({size:28}); semacs.loadTheme('missing')");
    let (visual, _, _) = open(&fixture, cx);
    visual.read(|cx| {
        assert_eq!(ThemeSettings::global(cx), &ThemeSettings::default());
        assert_eq!(cx.global::<FontSettings>().size(), px(16.));
    });
}

const UPPER: &str = r#"
    semacs.setFont({size: 20});
    semacs.registerCommand("uppercase-region", text => text.toUpperCase());
    semacs.bindKey("ctrl-c u", "uppercase-region");
    semacs.bindKey("ctrl-c U", "uppercase-region");
"#;

#[gpui::test]
fn configured_first_draw_and_both_command_entry_points_work(cx: &mut TestAppContext) {
    let fixture = Fixture::new(UPPER);
    let (mut visual, shell, view) = open(&fixture, cx);
    visual.read(|cx| assert_eq!(f32::from(cx.global::<FontSettings>().size()), 20.));
    select(&mut visual);
    visual.simulate_keystrokes("ctrl-c u");
    wait(&mut visual, &shell);
    assert_eq!(text(&mut visual, &view), "ABC def");
    visual.simulate_keystrokes("ctrl-/");
    assert_eq!(text(&mut visual, &view), "abc def");
    select(&mut visual);
    visual.simulate_keystrokes("alt-x");
    visual.read(|cx| {
        let candidates = shell
            .read(cx)
            .session
            .as_ref()
            .unwrap()
            .candidates()
            .unwrap();
        assert!(
            candidates
                .items
                .iter()
                .any(|item| item.candidate.value == "uppercase-region"
                    && item.candidate.annotation.contains("C-c"))
        );
    });
    visual.simulate_input("uppercase-region");
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    assert_eq!(text(&mut visual, &view), "ABC def");
}

#[gpui::test]
fn invalid_startup_uses_defaults_and_keeps_native_editing(cx: &mut TestAppContext) {
    let fixture = Fixture::new("semacs.setFont({size: 28}); throw new Error('broken')");
    let (mut visual, _, view) = open(&fixture, cx);
    visual.read(|cx| {
        assert_eq!(f32::from(cx.global::<FontSettings>().size()), 16.);
        assert!(view.read(cx).message().unwrap().contains("using defaults"));
    });
    visual.simulate_keystrokes("ctrl-f");
    visual.read(|cx| assert_eq!(view.read(cx).buffer().point().0, 1));
}

#[gpui::test]
fn reload_is_atomic_replaces_commands_and_clears_pending(cx: &mut TestAppContext) {
    let fixture = Fixture::new(UPPER);
    let (mut visual, shell, view) = open(&fixture, cx);
    let old = visual.read(|cx| shell.read(cx).config_generation);
    fixture.write("semacs.setFont({size: 24}); semacs.bindKey('ctrl-f', 'backward-char'); throw new Error('broken')");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.reload_configuration(&view, window, cx)
        })
    });
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.config_generation, old);
        assert!(shell.commands.named("uppercase-region").is_some());
        assert_eq!(f32::from(cx.global::<FontSettings>().size()), 20.);
    });
    visual.simulate_keystrokes("ctrl-f");
    visual.read(|cx| assert_eq!(view.read(cx).buffer().point().0, 1));
    fixture.write("semacs.setFont({size: 24}); semacs.bindKey('ctrl-f', 'backward-char')");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.reload_configuration(&view, window, cx)
        })
    });
    visual.simulate_keystrokes("ctrl-x");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_ne!(shell.config_generation, old);
        assert!(shell.commands.named("uppercase-region").is_none());
        assert_eq!(f32::from(cx.global::<FontSettings>().size()), 24.);
    });
    visual.simulate_keystrokes("ctrl-f");
    visual.read(|cx| assert_eq!(view.read(cx).buffer().point().0, 0));
}

#[gpui::test]
fn custom_body_keys_do_not_replace_minibuffer_keys(cx: &mut TestAppContext) {
    let fixture =
        Fixture::new("semacs.bindKey('ctrl-f', 'backward-char'); semacs.bindKey('ctrl-g', null)");
    let (mut visual, shell, _) = open(&fixture, cx);
    visual.simulate_keystrokes("alt-x");
    visual.simulate_input("ab");
    visual.simulate_keystrokes("ctrl-a ctrl-f");
    visual.read(|cx| {
        let editor = shell
            .read(cx)
            .minibuffer
            .as_ref()
            .unwrap()
            .view
            .read(cx)
            .editor();
        let buffer = editor.read(cx).buffer();
        assert_eq!(buffer.point().0, buffer.read_only_prefix().0 + 1);
    });
    visual.simulate_keystrokes("ctrl-g");
    visual.read(|cx| assert!(shell.read(cx).session.is_none()));
}

#[gpui::test]
fn stale_result_cannot_follow_focus_or_overwrite_new_text(cx: &mut TestAppContext) {
    let fixture = Fixture::new(UPPER);
    let (mut visual, shell, view) = open(&fixture, cx);
    select(&mut visual);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let crate::command_registry::CommandId::Script(command) =
                shell.commands.named("uppercase-region").unwrap()
            else {
                panic!()
            };
            shell.execute_script(command, &view, window, cx);
            view.update(cx, |view, cx| {
                view.execute_command(Command::DeleteChar, window, cx)
            });
        });
    });
    wait(&mut visual, &shell);
    assert_eq!(text(&mut visual, &view), "abcdef");
    visual.read(|cx| assert!(view.read(cx).message().unwrap().contains("discarded")));
}

#[gpui::test]
fn shared_buffer_transform_has_one_undo_and_other_view_sync(cx: &mut TestAppContext) {
    let fixture = Fixture::new(UPPER);
    let (mut visual, shell, first) = open(&fixture, cx);
    visual.simulate_keystrokes("ctrl-x 3");
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.simulate_keystrokes("ctrl-x o");
    let second = visual.read(|cx| shell.read(cx).active_view().clone());
    assert_ne!(first, second);
    select(&mut visual);
    visual.simulate_keystrokes("ctrl-c u");
    wait(&mut visual, &shell);
    assert_eq!(text(&mut visual, &first), "ABC def");
    assert_eq!(text(&mut visual, &second), "ABC def");
    visual.simulate_keystrokes("ctrl-x o ctrl-/");
    assert_eq!(text(&mut visual, &first), "abc def");
    visual.simulate_keystrokes("ctrl-?");
    assert_eq!(text(&mut visual, &second), "ABC def");
}

#[gpui::test]
fn unbinding_masks_defaults_and_missing_region_leaves_text_untouched(cx: &mut TestAppContext) {
    let fixture = Fixture::new(&format!("{UPPER}\nsemacs.bindKey('ctrl-f', null);"));
    let (mut visual, shell, view) = open(&fixture, cx);
    visual.simulate_keystrokes("ctrl-f");
    visual.read(|cx| {
        assert_eq!(view.read(cx).buffer().point().0, 0);
        assert!(view.read(cx).message().unwrap().contains("undefined"));
    });
    visual.simulate_keystrokes("ctrl-c u");
    wait(&mut visual, &shell);
    assert_eq!(text(&mut visual, &view), "abc def");
    visual.read(|cx| assert!(view.read(cx).message().unwrap().contains("mark")));
}

#[gpui::test]
fn result_does_not_follow_a_switch_to_another_view(cx: &mut TestAppContext) {
    let fixture = Fixture::new(UPPER);
    let (mut visual, shell, view) = open(&fixture, cx);
    visual.simulate_keystrokes("ctrl-x 3");
    visual.update(|window, cx| window.draw(cx).clear(cx));
    select(&mut visual);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let crate::command_registry::CommandId::Script(command) =
                shell.commands.named("uppercase-region").unwrap()
            else {
                panic!()
            };
            shell.execute_script(command, &view, window, cx);
            shell.window_command(Command::OtherWindow, window, cx);
        })
    });
    wait(&mut visual, &shell);
    assert_eq!(text(&mut visual, &view), "abc def");
    visual.read(|cx| assert_ne!(shell.read(cx).active_view(), &view));
}

#[gpui::test]
fn successful_reload_cancels_old_results_and_busy_requests_do_not_queue(cx: &mut TestAppContext) {
    let fixture = Fixture::new(UPPER);
    let (mut visual, shell, view) = open(&fixture, cx);
    let next = fixture.load().unwrap();
    select(&mut visual);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let crate::command_registry::CommandId::Script(command) =
                shell.commands.named("uppercase-region").unwrap()
            else {
                panic!()
            };
            shell.execute_script(command, &view, window, cx);
            shell.execute_script(command, &view, window, cx);
            assert!(view.read(cx).message().unwrap().contains("already running"));
            shell.apply_configuration(next, window, cx);
        })
    });
    wait(&mut visual, &shell);
    assert_eq!(text(&mut visual, &view), "abc def");
}

#[test]
fn source_loading_handles_missing_invalid_and_oversized_files() {
    let fixture = Fixture::new(include_str!("../../examples/init.js"));
    let loaded = fixture.load().unwrap();
    assert!(
        loaded
            .configuration
            .commands
            .named("uppercase-region")
            .is_some()
    );
    std::fs::remove_file(&fixture.0).unwrap();
    let empty = fixture.load().unwrap();
    assert!(
        empty
            .configuration
            .commands
            .named("uppercase-region")
            .is_none()
    );
    std::fs::write(&fixture.0, [0xff]).unwrap();
    assert!(fixture.load().err().unwrap().contains("UTF-8"));
    fixture.write(&" ".repeat(crate::scripting::MAX_SOURCE_BYTES + 1));
    assert!(fixture.load().err().unwrap().contains("1 MiB"));
}

#[gpui::test]
fn timeout_does_not_block_native_commands_or_modify_text(cx: &mut TestAppContext) {
    let fixture = Fixture::new(
        r#"
        semacs.registerCommand("loop-region", () => { while (true) {} });
        semacs.bindKey("ctrl-c u", "loop-region");
    "#,
    );
    let (mut visual, shell, view) = open(&fixture, cx);
    select(&mut visual);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let crate::command_registry::CommandId::Script(command) =
                shell.commands.named("loop-region").unwrap()
            else {
                panic!()
            };
            shell.execute_script(command, &view, window, cx);
            view.update(cx, |view, cx| {
                view.execute_command(Command::ForwardChar, window, cx)
            });
            assert_eq!(view.read(cx).buffer().point().0, 4);
            assert!(shell.script_task.is_some());
        })
    });
    wait(&mut visual, &shell);
    assert_eq!(text(&mut visual, &view), "abc def");
    visual.read(|cx| assert!(view.read(cx).message().unwrap().contains("100 ms")));
}

#[gpui::test]
fn reload_rejects_installation_during_composition(cx: &mut TestAppContext) {
    let fixture = Fixture::new(UPPER);
    let (mut visual, shell, view) = open(&fixture, cx);
    let old = visual.read(|cx| shell.read(cx).config_generation);
    fixture.write("semacs.setFont({size: 24})");
    let mut input = crate::view::input_tests::handler(&view, &visual);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.reload_configuration(&view, window, cx)
        });
        input.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
    });
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).config_generation, old);
        assert!(view.read(cx).display_snapshot().marked_range().is_some());
        assert_eq!(f32::from(cx.global::<FontSettings>().size()), 20.);
    });
    assert_eq!(text(&mut visual, &view), "abc def");
}
