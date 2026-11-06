use crate::app::{AppShell, GitWritePhase, git_test_support::*};
use crate::buffer::Buffer;
use crate::command::Command;
use crate::diff::tests::Repository as Fixture;
use crate::files::{DecodedFile, FileFormat};
use crate::mode::ModeId;
use crate::status::{Group, index::Outcome};
use crate::text::ByteOffset;
use gpui_kit as gpui;
use gpui_kit::{Entity, TestAppContext, VisualTestContext, px, size};

fn select(visual: &mut VisualTestContext, shell: &Entity<AppShell>, group: Group) {
    visual.update(|window, cx| {
        let view = shell.read(cx).active_view().clone();
        let sections = view.read(cx).buffer().sections().unwrap();
        let point = sections
            .nodes()
            .iter()
            .find(|node| node.key.starts_with(&format!("{}/file/", group.key())))
            .unwrap()
            .range
            .start;
        view.update(cx, |view, cx| view.show_location(point, window, cx));
    });
}

fn opened(cx: &mut TestAppContext) -> (Fixture, VisualTestContext, Entity<AppShell>) {
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    select(&mut visual, &shell, Group::Unstaged);
    (repo, visual, shell)
}

fn settled(visual: &mut VisualTestContext, shell: &Entity<AppShell>) {
    until(visual, shell, |shell| {
        shell.script_task.is_none() && shell.git_task.is_none() && shell.reload_task.is_none()
    });
}

fn invoke(visual: &mut VisualTestContext, shell: &Entity<AppShell>, name: &str) {
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            shell.run_command(shell.commands.named(name).unwrap(), &origin, window, cx);
        })
    });
}

fn configure(
    repo: &Fixture,
    visual: &mut VisualTestContext,
    shell: &Entity<AppShell>,
    source: &str,
) {
    repo.write("init.js", source);
    let loaded = gpui::block_on(crate::scripting::load(repo.0.join("init.js"))).unwrap();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.config_path = Some(repo.0.join("init.js"));
            shell.apply_configuration(loaded, window, cx);
        })
    });
}

fn return_gate(
    visual: &mut VisualTestContext,
    shell: &Entity<AppShell>,
) -> futures_channel::oneshot::Sender<()> {
    let (send, receive) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.workflow_return_gate = Some(receive)));
    send
}

#[gpui::test]
fn default_keys_and_mx_use_real_js_handlers_and_leave_edit_buffer_unchanged(
    cx: &mut TestAppContext,
) {
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    let edit = visual.read(|cx| shell.read(cx).active_view().clone());
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    select(&mut visual, &shell, Group::Unstaged);
    visual.simulate_keystrokes("s");
    settled(&mut visual, &shell);
    assert_eq!(repo.git(&["show", ":a"]), b"disk changed\n");
    select(&mut visual, &shell, Group::Staged);
    visual.simulate_keystrokes("alt-x");
    visual.simulate_input("magit-unstage");
    visual.simulate_keystrokes("enter");
    settled(&mut visual, &shell);
    assert_eq!(repo.git(&["show", ":a"]), b"disk base\n");
    assert_eq!(repo.git(&["show", "HEAD:a"]), b"disk base\n");
    assert_eq!(std::fs::read(repo.0.join("a")).unwrap(), b"disk changed\n");
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.git_write_records().count(), 2);
        assert!(
            shell
                .git_write_records()
                .all(|record| record.outcome == Some(Outcome::Succeeded))
        );
        assert_eq!(
            edit.read(cx).buffer().text().to_string(),
            "unsaved editor text\n"
        );
        assert!(edit.read(cx).buffer().new_view().undo().unwrap().is_none());
        let history = shell.messages.buffer.text().to_string();
        assert!(history.contains("current disk content") && history.contains("no automatic undo"));
    });
}

#[gpui::test]
fn delayed_stage_uses_execution_time_disk_bytes_not_the_status_or_editor_snapshot(
    cx: &mut TestAppContext,
) {
    let (repo, mut visual, shell) = opened(cx);
    let release = return_gate(&mut visual, &shell);
    invoke(&mut visual, &shell, "magit-stage");
    repo.write("a", "newer disk content\n");
    release.send(()).unwrap();
    settled(&mut visual, &shell);
    assert_eq!(repo.git(&["show", ":a"]), b"newer disk content\n");
    assert_eq!(repo.git(&["show", "HEAD:a"]), b"disk base\n");
}

#[gpui::test]
fn a_delayed_read_cannot_rediscover_from_a_save_as_directory(cx: &mut TestAppContext) {
    let first = repo();
    let second = repo();
    let (mut visual, shell) = open(&first, cx);
    configure(
        &first,
        &mut visual,
        &shell,
        "semacs.registerWorkflowCommand('open-status',{run:()=>({operation:'magit.status.open'})});",
    );
    let release = return_gate(&mut visual, &shell);
    invoke(&mut visual, &shell, "open-status");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            shell.write_file(&origin, second.0.join("relocated"), window, cx);
        })
    });
    wait(&mut visual, &shell);
    release.send(()).unwrap();
    settled(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(
            shell
                .buffers
                .iter()
                .all(|slot| slot.buffer.mode() != ModeId::MAGIT_STATUS)
        );
        assert!(
            shell
                .messages
                .buffer
                .text()
                .to_string()
                .contains("discarded")
        );
    });
}

#[gpui::test]
fn empty_and_failed_initial_configuration_build_bundled_runtime(cx: &mut TestAppContext) {
    for source in ["", "throw Error('controlled configuration failure')"] {
        let repo = repo();
        repo.write("init.js", source);
        let loaded = gpui::block_on(crate::scripting::load(repo.0.join("init.js")));
        cx.background_executor.allow_parking();
        cx.update(crate::app::init);
        let window = cx.open_window(size(px(960.), px(640.)), |window, cx| {
            AppShell::with_configuration(
                Buffer::new(
                    Some(repo.0.join("a")),
                    DecodedFile {
                        text: "unsaved".into(),
                        format: FileFormat::default(),
                    },
                ),
                Some(repo.0.join("init.js")),
                loaded,
                window,
                cx,
            )
        });
        let shell = window.entity(cx).unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            window.activate_window();
            window.draw(cx).clear(cx);
        });
        command(&mut visual, &shell, Command::MagitStatus);
        wait(&mut visual, &shell);
        select(&mut visual, &shell, Group::Unstaged);
        invoke(&mut visual, &shell, "magit-stage");
        settled(&mut visual, &shell);
        assert_eq!(repo.git(&["show", ":a"]), b"disk changed\n");
        select(&mut visual, &shell, Group::Staged);
        invoke(&mut visual, &shell, "magit-unstage");
        settled(&mut visual, &shell);
        assert_eq!(repo.git(&["show", ":a"]), b"disk base\n");
    }
}

#[gpui::test]
fn custom_requests_cannot_bypass_heading_region_stale_or_diff_targets(cx: &mut TestAppContext) {
    for scenario in 0..5 {
        let (repo, mut visual, shell) = opened(cx);
        configure(
            &repo,
            &mut visual,
            &shell,
            "semacs.registerWorkflowCommand('force-stage',{run:()=>({operation:'magit.stage'})});",
        );
        match scenario {
            0 => visual.update(|window, cx| {
                let view = shell.read(cx).active_view().clone();
                view.update(cx, |view, cx| view.show_location(ByteOffset(0), window, cx));
            }),
            1 => visual.update(|_, cx| {
                let view = shell.read(cx).active_view().clone();
                view.read(cx)
                    .buffer()
                    .new_view()
                    .set_mark(ByteOffset(0))
                    .unwrap();
            }),
            2 => visual.update(|_, cx| {
                let view = shell.read(cx).active_view().clone();
                view.read(cx).buffer().new_view().mark_git_stale();
            }),
            3 => {
                command(&mut visual, &shell, Command::MagitDiffUnstaged);
                wait(&mut visual, &shell);
            }
            4 => {
                std::fs::create_dir(repo.0.join("dir")).unwrap();
                repo.write("dir/file", "untracked\n");
                command(&mut visual, &shell, Command::MagitRefresh);
                wait(&mut visual, &shell);
                select(&mut visual, &shell, Group::Untracked);
            }
            _ => unreachable!(),
        }
        invoke(&mut visual, &shell, "force-stage");
        settled(&mut visual, &shell);
        assert_eq!(repo.git(&["show", ":a"]), b"disk base\n");
        visual.read(|cx| assert_eq!(shell.read(cx).git_write_records().count(), 0));
    }
}

#[gpui::test]
fn delayed_js_return_cannot_retarget_after_point_projection_input_or_lifecycle_change(
    cx: &mut TestAppContext,
) {
    for scenario in 0..9 {
        let (repo, mut visual, shell) = opened(cx);
        let release = return_gate(&mut visual, &shell);
        invoke(&mut visual, &shell, "magit-stage");
        let origin = visual.read(|cx| shell.read(cx).active_view().clone());
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| match scenario {
                0 => {
                    let point = origin.read(cx).buffer().point();
                    origin.update(cx, |view, cx| {
                        view.show_location(ByteOffset(0), window, cx);
                        view.show_location(point, window, cx);
                    });
                }
                1 => {
                    origin
                        .read(cx)
                        .buffer()
                        .new_view()
                        .set_mark(ByteOffset(0))
                        .unwrap();
                }
                2 => {
                    let index = shell.buffer_index(&origin, cx).unwrap();
                    shell.buffers[index].buffer.begin_status_request().unwrap();
                }
                3 => origin.update(cx, |view, cx| {
                    view.suspend_input(cx);
                    view.restore_input(window, cx);
                }),
                4 => {
                    let index = shell.buffer_index(&origin, cx).unwrap();
                    shell.remove_buffer(index, window, cx);
                }
                5 => shell.write_file(&origin, repo.0.join("export.txt"), window, cx),
                6 => {
                    shell.window_command(Command::SplitWindowRight, window, cx);
                    shell.window_command(Command::DeleteWindow, window, cx);
                }
                7 | 8 => {}
                _ => unreachable!(),
            })
        });
        if scenario == 5 {
            wait(&mut visual, &shell);
        }
        if scenario == 7 {
            visual.simulate_keystrokes("ctrl-x");
        }
        if scenario == 8 {
            command(&mut visual, &shell, Command::ExecuteExtendedCommand);
        }
        release.send(()).unwrap();
        settled(&mut visual, &shell);
        assert_eq!(
            repo.git(&["show", ":a"]),
            b"disk base\n",
            "scenario {scenario}"
        );
        visual.read(|cx| assert_eq!(shell.read(cx).git_write_records().count(), 0));
    }
}

#[gpui::test]
fn reload_before_acceptance_discards_old_return_but_failed_reload_preserves_it(
    cx: &mut TestAppContext,
) {
    for success in [false, true] {
        let (repo, mut visual, shell) = opened(cx);
        let release = return_gate(&mut visual, &shell);
        invoke(&mut visual, &shell, "magit-stage");
        repo.write(
            "init.js",
            if success {
                ""
            } else {
                "throw Error('controlled reload failure')"
            },
        );
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.config_path = Some(repo.0.join("init.js"));
                let origin = shell.active_view().clone();
                shell.reload_configuration(&origin, window, cx);
            })
        });
        until(&mut visual, &shell, |shell| shell.reload_task.is_none());
        let _ = release.send(());
        settled(&mut visual, &shell);
        assert_eq!(
            repo.git(&["show", ":a"]),
            if success {
                &b"disk base\n"[..]
            } else {
                &b"disk changed\n"[..]
            }
        );
        visual.read(|cx| {
            assert_eq!(
                shell.read(cx).git_write_records().count(),
                usize::from(!success)
            )
        });
    }
}

#[gpui::test]
fn accepted_script_write_outlives_reload_and_origin_kill(cx: &mut TestAppContext) {
    let (repo, mut visual, shell) = opened(cx);
    let (release, gate) = futures_channel::oneshot::channel();
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.index_write_gate = Some(gate)));
    invoke(&mut visual, &shell, "magit-stage");
    until(&mut visual, &shell, |shell| shell.git_write_active());
    configure(&repo, &mut visual, &shell, "");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = shell.active_view().clone();
            let index = shell.buffer_index(&origin, cx).unwrap();
            shell.remove_buffer(index, window, cx);
        })
    });
    release.send(()).unwrap();
    settled(&mut visual, &shell);
    assert_eq!(repo.git(&["show", ":a"]), b"disk changed\n");
    visual.read(|cx| {
        let shell = shell.read(cx);
        let record = shell.git_write_records().last().unwrap();
        assert_eq!(record.phase, GitWritePhase::Complete);
        assert_eq!(record.outcome, Some(Outcome::Succeeded));
        assert!(!record.installed);
        assert!(
            shell
                .messages
                .buffer
                .text()
                .to_string()
                .contains("write succeeded")
        );
    });
}

#[gpui::test]
fn runtime_fault_busy_and_invalid_returns_have_no_native_write_fallback(cx: &mut TestAppContext) {
    for body in [
        "throw Error('controlled')",
        "while(true){}",
        "return Promise.resolve({operation:'magit.stage'})",
        "Promise.resolve().then(()=>{}); return {operation:'magit.stage'}",
        "return {operation:'magit.stage',path:'a'}",
    ] {
        let (repo, mut visual, shell) = opened(cx);
        configure(
            &repo,
            &mut visual,
            &shell,
            &format!("semacs.registerWorkflowCommand('broken',{{run:()=>{{{body}}}}});"),
        );
        invoke(&mut visual, &shell, "broken");
        settled(&mut visual, &shell);
        visual.read(|cx| assert_eq!(shell.read(cx).git_write_records().count(), 0));
        assert_eq!(repo.git(&["show", ":a"]), b"disk base\n");
    }
    let (repo, mut visual, shell) = opened(cx);
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.script_host = None));
    invoke(&mut visual, &shell, "magit-stage");
    settled(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).git_write_records().count(), 0);
        assert!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .message()
                .unwrap()
                .contains("unavailable")
        );
    });
    command(&mut visual, &shell, Command::MagitDiffUnstaged);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().mode(),
            ModeId::MAGIT_DIFF
        )
    });
    assert_eq!(repo.git(&["show", ":a"]), b"disk base\n");
}

#[gpui::test]
fn shared_script_and_git_slots_reject_without_queuing_and_mode_guards_are_consistent(
    cx: &mut TestAppContext,
) {
    let (repo, mut visual, shell) = opened(cx);
    let release = return_gate(&mut visual, &shell);
    invoke(&mut visual, &shell, "magit-stage");
    invoke(&mut visual, &shell, "magit-stage");
    visual.read(|cx| {
        assert!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .message()
                .unwrap()
                .contains("already running")
        )
    });
    let read_release = gate(&mut visual, &shell);
    command(&mut visual, &shell, Command::MagitRefresh);
    until(&mut visual, &shell, |shell| {
        shell.status_read_gate.is_none()
    });
    release.send(()).unwrap();
    until(&mut visual, &shell, |shell| shell.script_task.is_none());
    read_release.send(()).unwrap();
    settled(&mut visual, &shell);
    assert_eq!(repo.git(&["show", ":a"]), b"disk base\n");
    visual.read(|cx| assert_eq!(shell.read(cx).git_write_records().count(), 0));
    command(&mut visual, &shell, Command::MagitDiffUnstaged);
    wait(&mut visual, &shell);
    invoke(&mut visual, &shell, "magit-stage");
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(shell.script_task.is_none());
        assert!(
            shell
                .active_view()
                .read(cx)
                .message()
                .unwrap()
                .contains("unavailable")
        );
        assert!(
            !shell
                .commands
                .commands_for(ModeId::MAGIT_DIFF)
                .any(|entry| entry.name == "magit-stage")
        );
    });
}

#[gpui::test]
fn custom_read_requests_open_fixed_comparisons_and_refresh_only_status(cx: &mut TestAppContext) {
    let (repo, mut visual, shell) = opened(cx);
    configure(
        &repo,
        &mut visual,
        &shell,
        r#"
        semacs.registerWorkflowCommand('refresh-status',{run:()=>({operation:'magit.status.refresh'})});
        semacs.registerWorkflowCommand('open-status',{run:()=>({operation:'magit.status.open'})});
        semacs.registerWorkflowCommand('open-diff',{run:()=>({operation:'magit.diff.open',comparison:'unstaged'})});
    "#,
    );
    repo.write("b", "another\n");
    invoke(&mut visual, &shell, "refresh-status");
    settled(&mut visual, &shell);
    visual.read(|cx| {
        assert!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .text()
                .to_string()
                .contains("b")
        )
    });
    invoke(&mut visual, &shell, "open-diff");
    settled(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().mode(),
            ModeId::MAGIT_DIFF
        )
    });
    invoke(&mut visual, &shell, "refresh-status");
    settled(&mut visual, &shell);
    visual.read(|cx| {
        assert!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .message()
                .unwrap()
                .contains("requires a status")
        )
    });
    invoke(&mut visual, &shell, "open-status");
    settled(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).active_view().read(cx).buffer().mode(),
            ModeId::MAGIT_STATUS
        )
    });
}
