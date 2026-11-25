use super::*;
use crate::app::git_test_support::*;
use crate::command::Command;
use crate::mode::ModeId;
use gpui_kit as gpui;
use gpui_kit::{TestAppContext, VisualTestContext};

#[gpui::test]
fn status_opens_reuses_contextual_commands_and_fixed_repository_diff(cx: &mut TestAppContext) {
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    let ordinary = visual.read(|cx| shell.read(cx).active_view().clone());
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let status = visual.read(|cx| shell.read(cx).active_view().clone());
    let id = visual.read(|cx| {
        let buffer = status.read(cx).buffer();
        assert_eq!(buffer.mode(), ModeId::MAGIT_STATUS);
        assert!(buffer.is_read_only());
        assert!(!buffer.is_modified());
        assert_eq!(buffer.status_data().unwrap().entries().len(), 1);
        assert_eq!(
            ordinary.read(cx).buffer().text().to_string(),
            "unsaved editor text\n"
        );
        assert!(
            shell
                .read(cx)
                .commands
                .commands_for(ModeId::MAGIT_STATUS)
                .any(|entry| entry.name == "section-toggle")
        );
        assert!(
            shell
                .read(cx)
                .commands
                .commands_for(ModeId::MAGIT_STATUS)
                .any(|entry| entry.name == "magit-visit-thing")
        );
        assert!(
            shell
                .read(cx)
                .commands
                .commands_for(ModeId::MAGIT_STATUS)
                .any(|entry| entry.name == "magit-diff-visit-file")
        );
        assert!(
            !shell
                .read(cx)
                .commands
                .commands_for(ModeId::MAGIT_STATUS)
                .any(|entry| entry.name == "magit-diff-jump")
        );
        buffer.snapshot_id().buffer
    });
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    visual.read(|cx| assert_eq!(shell.read(cx).active_buffer_id(), id));
    visual.simulate_keystrokes("tab");
    visual.read(|cx| {
        assert!(
            status
                .read(cx)
                .buffer()
                .sections()
                .unwrap()
                .nodes()
                .iter()
                .find(|node| node.key == "unstaged")
                .unwrap()
                .collapsed
        );
    });
    command(&mut visual, &shell, Command::MagitDiffJump);
    visual.read(|cx| assert!(status.read(cx).message().unwrap().contains("unavailable")));
    command(&mut visual, &shell, Command::MagitDiffUnstaged);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let comparison = shell
            .read(cx)
            .active_view()
            .read(cx)
            .buffer()
            .comparison()
            .unwrap();
        assert_eq!(
            &comparison.repository,
            status.read(cx).buffer().status_data().unwrap().repository()
        );
    });
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    visual.read(|cx| assert_eq!(shell.read(cx).active_buffer_id(), id));
}

#[gpui::test]
fn refresh_keeps_shared_folds_and_each_view_position_without_focus_steal(cx: &mut TestAppContext) {
    let repo = repo();
    repo.write("b", "untracked\n");
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let first = visual.read(|cx| shell.read(cx).active_view().clone());
    let first_pane = visual.read(|cx| *shell.read(cx).panes.selected());
    command(&mut visual, &shell, Command::SplitWindowRight);
    command(&mut visual, &shell, Command::OtherWindow);
    let second = visual.read(|cx| shell.read(cx).active_view().clone());
    visual.update(|window, cx| {
        let sections = first.read(cx).buffer().sections().unwrap();
        let point = sections
            .nodes()
            .iter()
            .find(|node| node.key == "untracked")
            .unwrap()
            .range
            .start;
        first.update(cx, |view, cx| view.show_location(point, window, cx));
        let leaf = sections
            .nodes()
            .iter()
            .find(|node| node.key.starts_with("unstaged/file/"))
            .unwrap()
            .range
            .start;
        second.update(cx, |view, cx| view.show_location(leaf, window, cx));
    });
    let before = visual.read(|cx| {
        (
            first.read(cx).buffer().point(),
            second.read(cx).buffer().point(),
        )
    });
    let release = gate(&mut visual, &shell);
    command(&mut visual, &shell, Command::MagitRefresh);
    until(&mut visual, &shell, |shell| {
        shell.status_read_gate.is_none()
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.select_pane(first_pane, window, cx))
    });
    release.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(shell.read(cx).active_view(), &first);
        assert_eq!(
            (
                first.read(cx).buffer().point(),
                second.read(cx).buffer().point()
            ),
            before
        );
    });
    visual.simulate_keystrokes("tab");
    visual.read(|cx| {
        assert!(
            second
                .read(cx)
                .buffer()
                .sections()
                .unwrap()
                .nodes()
                .iter()
                .find(|node| node.key == "untracked")
                .unwrap()
                .collapsed
        )
    });
    command(&mut visual, &shell, Command::MagitRefresh);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert!(
            first
                .read(cx)
                .buffer()
                .sections()
                .unwrap()
                .nodes()
                .iter()
                .find(|node| node.key == "untracked")
                .unwrap()
                .collapsed
        )
    });
}

#[gpui::test]
fn open_bound_to_existing_status_never_resurrects_killed_or_detached_target(
    cx: &mut TestAppContext,
) {
    for detach in [false, true] {
        let repo = repo();
        let (mut visual, shell) = open(&repo, cx);
        let origin = visual.read(|cx| shell.read(cx).active_view().clone());
        let origin_pane = visual.read(|cx| *shell.read(cx).panes.selected());
        command(&mut visual, &shell, Command::MagitStatus);
        wait(&mut visual, &shell);
        let status = visual.read(|cx| shell.read(cx).active_view().clone());
        let repository = visual.read(|cx| status.read(cx).buffer().status_repository().unwrap());
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| shell.select_pane(origin_pane, window, cx))
        });
        let release = gate(&mut visual, &shell);
        command(&mut visual, &shell, Command::MagitStatus);
        until(&mut visual, &shell, |shell| {
            shell.status_read_gate.is_none()
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let index = shell.buffer_index(&status, cx).unwrap();
                if detach {
                    shell.buffers[index].buffer.detach_status();
                } else {
                    shell.remove_buffer(index, window, cx);
                }
                shell.create_buffer(Buffer::status(repository.clone()), window, cx);
            })
        });
        release.send(()).unwrap();
        wait(&mut visual, &shell);
        visual.read(|cx| {
            let shell = shell.read(cx);
            assert_eq!(shell.active_view(), &origin);
            let replacement = shell
                .buffers
                .iter()
                .find(|slot| slot.buffer.status_repository().is_some())
                .unwrap();
            assert!(replacement.buffer.text().is_empty());
        });
    }
}

#[gpui::test]
fn new_intent_conflict_and_lost_display_eligibility_discard_results(cx: &mut TestAppContext) {
    let repo = repo();
    let repository = crate::git::discover(&repo.0, &AtomicBool::new(false)).unwrap();
    let (mut visual, shell) = open(&repo, cx);
    let origin = visual.read(|cx| shell.read(cx).active_view().clone());
    let release = gate(&mut visual, &shell);
    command(&mut visual, &shell, Command::MagitStatus);
    until(&mut visual, &shell, |shell| {
        shell.status_read_gate.is_none()
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.create_buffer(Buffer::status(repository), window, cx);
        })
    });
    release.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| assert_eq!(shell.read(cx).active_view(), &origin));
    let release = gate(&mut visual, &shell);
    command(&mut visual, &shell, Command::MagitStatus);
    until(&mut visual, &shell, |shell| {
        shell.status_read_gate.is_none()
    });
    command(&mut visual, &shell, Command::ExecuteExtendedCommand);
    release.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert!(shell.minibuffer.is_some());
        assert!(
            shell
                .buffers
                .iter()
                .find(|slot| slot.buffer.status_repository().is_some())
                .unwrap()
                .buffer
                .text()
                .is_empty()
        );
    });
}

#[gpui::test]
fn pending_prefix_composition_and_focus_roundtrip_invalidate_open(cx: &mut TestAppContext) {
    use gpui_kit::InputHandler;
    for scenario in 0..3 {
        let repo = repo();
        let (mut visual, shell) = open(&repo, cx);
        let origin = visual.read(|cx| shell.read(cx).active_view().clone());
        let release = gate(&mut visual, &shell);
        command(&mut visual, &shell, Command::MagitStatus);
        until(&mut visual, &shell, |shell| {
            shell.status_read_gate.is_none()
        });
        command(&mut visual, &shell, Command::MagitDiffUnstaged);
        visual.read(|cx| {
            assert!(
                origin
                    .read(cx)
                    .message()
                    .unwrap()
                    .contains("already running")
            )
        });
        match scenario {
            0 => visual.simulate_keystrokes("ctrl-x"),
            1 => {
                let mut input = crate::view::input_tests::handler(&origin, &visual);
                visual.update(|window, cx| {
                    input.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx)
                });
            }
            _ => {
                command(&mut visual, &shell, Command::SplitWindowRight);
                command(&mut visual, &shell, Command::OtherWindow);
                command(&mut visual, &shell, Command::OtherWindow);
            }
        }
        release.send(()).unwrap();
        wait(&mut visual, &shell);
        visual.read(|cx| {
            let shell = shell.read(cx);
            assert_eq!(shell.active_view(), &origin);
            assert!(
                shell
                    .buffers
                    .iter()
                    .all(|slot| slot.buffer.status_repository().is_none())
            );
        });
    }
}

#[gpui::test]
fn export_keeps_new_projection_dirty_and_detaches_without_late_install(cx: &mut TestAppContext) {
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let status = visual.read(|cx| shell.read(cx).active_view().clone());
    let old = visual.read(|cx| status.read(cx).buffer().text().to_string());
    repo.write("new", "new\n");
    let repository = visual.read(|cx| status.read(cx).buffer().status_repository().unwrap());
    let data = Status::load(repository, &AtomicBool::new(false)).unwrap();
    let rendered = data.render().unwrap();
    let new = rendered.text.clone();
    let path = repo.0.join("export.txt");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.write_file(&status, path.clone(), window, cx);
            let index = shell.buffer_index(&status, cx).unwrap();
            let (_, request) = shell.buffers[index].buffer.begin_status_request().unwrap();
            assert!(
                shell.buffers[index]
                    .buffer
                    .install_status(request, data, rendered)
            );
            shell.refresh_buffer(shell.buffers[index].id(), cx);
        })
    });
    wait(&mut visual, &shell);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), old);
    visual.read(|cx| {
        let buffer = status.read(cx).buffer();
        assert_eq!(buffer.text().to_string(), new);
        assert!(buffer.is_modified());
        assert!(!buffer.is_read_only());
        assert_eq!(buffer.mode(), ModeId::FUNDAMENTAL);
        assert!(buffer.sections().is_none());
        assert!(buffer.status_repository().is_none());
    });
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let status = visual.read(|cx| shell.read(cx).active_view().clone());
    let release = gate(&mut visual, &shell);
    command(&mut visual, &shell, Command::MagitRefresh);
    until(&mut visual, &shell, |shell| {
        shell.status_read_gate.is_none()
    });
    let before = visual.read(|cx| status.read(cx).buffer().text().to_string());
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.write_file(&status, repo.0.join("export-first.txt"), window, cx)
        })
    });
    until(&mut visual, &shell, |shell| {
        shell.buffers.iter().all(|slot| !slot.save.is_saving())
    });
    repo.write("later", "later\n");
    release.send(()).unwrap();
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(status.read(cx).buffer().text().to_string(), before);
        assert!(!status.read(cx).buffer().is_modified());
        assert_eq!(status.read(cx).buffer().mode(), ModeId::FUNDAMENTAL);
    });
}

#[gpui::test]
fn queued_export_rebases_its_save_point_to_editable_history(cx: &mut TestAppContext) {
    use crate::saves::{SaveCompletion, SaveIntent, SaveRequest};
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let status = visual.read(|cx| shell.read(cx).active_view().clone());
    let repository = visual.read(|cx| status.read(cx).buffer().status_repository().unwrap());
    repo.write("new", "new\n");
    let data = Status::load(repository, &AtomicBool::new(false)).unwrap();
    let rendered = data.render().unwrap();
    let second_text = rendered.text.clone();
    let path = repo.0.join("queued-export.txt");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let index = shell.buffer_index(&status, cx).unwrap();
            let slot = &mut shell.buffers[index];
            let request = SaveRequest {
                generation: slot.save.next_generation(),
                save_point: slot.buffer.begin_save(),
                intent: SaveIntent::WriteFile(path.clone()),
                confirmed: false,
                format: slot.buffer.format(),
                text: slot.buffer.text(),
            };
            slot.save.start(&request);
            let (_, token) = slot.buffer.begin_status_request().unwrap();
            assert!(slot.buffer.install_status(token, data, rendered));
            let generation = slot.save.next_generation();
            slot.save.queue(SaveRequest {
                generation,
                save_point: slot.buffer.begin_save(),
                intent: SaveIntent::WriteFile(path.clone()),
                confirmed: true,
                format: slot.buffer.format(),
                text: slot.buffer.text(),
            });
            let completion = SaveCompletion {
                buffer: slot.id(),
                path: path.clone(),
                result: request.write(&path, None),
                request,
            };
            shell.finish_save(completion, window, cx);
            assert_eq!(status.read(cx).buffer().mode(), ModeId::FUNDAMENTAL);
            let mut buffer = status.read(cx).buffer().new_view();
            buffer
                .edit(crate::buffer::Edit {
                    snapshot: buffer.snapshot_id(),
                    range: crate::text::ByteOffset(0)..crate::text::ByteOffset(0),
                    text: "edit after detach\n",
                    source: crate::buffer::EditSource::Command,
                })
                .unwrap();
            shell.refresh_buffer(buffer.snapshot_id().buffer, cx);
        })
    });
    wait(&mut visual, &shell);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), second_text);
    visual.read(|cx| assert!(status.read(cx).buffer().is_modified()));
    command(&mut visual, &shell, Command::Undo);
    visual.read(|cx| {
        assert_eq!(status.read(cx).buffer().text().to_string(), second_text);
        assert!(!status.read(cx).buffer().is_modified());
    });
}

#[gpui::test]
fn failed_export_and_configuration_reload_preserve_status_session(cx: &mut TestAppContext) {
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let status = visual.read(|cx| shell.read(cx).active_view().clone());
    visual.simulate_keystrokes("tab");
    let before = visual.read(|cx| {
        (
            status.read(cx).buffer().snapshot_id(),
            status.read(cx).buffer().point(),
        )
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.write_file(&status, repo.0.join("missing/export.txt"), window, cx)
        })
    });
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let buffer = status.read(cx).buffer();
        assert_eq!(buffer.mode(), ModeId::MAGIT_STATUS);
        assert!(buffer.is_read_only());
        assert!(buffer.status_repository().is_some());
        assert!(status.read(cx).message().unwrap().contains("Save failed"));
    });
    let init = repo.0.join("init.js");
    std::fs::write(
        &init,
        "semacs.bindKey('n', 'section-backward', {mode:'magit-status'});",
    )
    .unwrap();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.config_path = Some(init.clone());
            shell.reload_configuration(&status, window, cx);
        })
    });
    until(&mut visual, &shell, |shell| shell.reload_task.is_none());
    visual.read(|cx| {
        assert_eq!(
            (
                status.read(cx).buffer().snapshot_id(),
                status.read(cx).buffer().point()
            ),
            before
        );
        assert!(
            status
                .read(cx)
                .buffer()
                .sections()
                .unwrap()
                .nodes()
                .iter()
                .find(|node| node.key == "unstaged")
                .unwrap()
                .collapsed
        );
    });
    visual.simulate_keystrokes("n");
    visual.read(|cx| {
        let sections = status.read(cx).buffer().sections().unwrap();
        assert_eq!(
            status.read(cx).buffer().point(),
            sections
                .nodes()
                .iter()
                .find(|node| node.key == "header/head")
                .unwrap()
                .range
                .start
        );
    });
    let generation = visual.read(|cx| shell.read(cx).config_generation);
    std::fs::write(&init, "throw new Error('failed reload');").unwrap();
    command(&mut visual, &shell, Command::ReloadConfiguration);
    until(&mut visual, &shell, |shell| shell.reload_task.is_none());
    visual.read(|cx| assert_eq!(shell.read(cx).config_generation, generation));
}

#[gpui::test]
fn refresh_failure_preserves_content_and_old_completion_cannot_clear_new_slot(
    cx: &mut TestAppContext,
) {
    let repo = repo();
    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let status = visual.read(|cx| shell.read(cx).active_view().clone());
    let old = visual.read(|cx| status.read(cx).buffer().text().to_string());
    std::fs::rename(repo.0.join(".git"), repo.0.join("hidden-git")).unwrap();
    command(&mut visual, &shell, Command::MagitRefresh);
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(status.read(cx).buffer().text().to_string(), old);
        assert!(
            status
                .read(cx)
                .message()
                .unwrap()
                .contains("Status unchanged")
        );
    });
    std::fs::rename(repo.0.join("hidden-git"), repo.0.join(".git")).unwrap();
    let release = gate(&mut visual, &shell);
    command(&mut visual, &shell, Command::MagitRefresh);
    until(&mut visual, &shell, |shell| {
        shell.status_read_gate.is_none()
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let operation = shell.git_task.as_ref().unwrap().operation();
            assert!(!shell.finish_git_read(operation + 1, window, cx));
            assert!(shell.git_task.is_some());
        })
    });
    release.send(()).unwrap();
    wait(&mut visual, &shell);
}

#[gpui::test]
fn status_ret_visits_untracked_unstaged_staged_and_group_diffs_like_magit(cx: &mut TestAppContext) {
    let repo = repo();
    repo.write("both.txt", "line1\nline2\nold\nline4\n");
    repo.write("del.txt", "gone\n");
    repo.commit();
    repo.write("both.txt", "line1\nline2\nindexed\nline4\n");
    repo.git(&["add", "both.txt"]);
    repo.write("both.txt", "line1\nline2\nworktree\nline4\n");
    std::fs::remove_file(repo.0.join("del.txt")).unwrap();
    repo.write("new.txt", "untracked body\n");
    std::fs::create_dir_all(repo.0.join("plain_dir")).unwrap();
    repo.write("plain_dir/child.txt", "nested\n");

    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let status = visual.read(|cx| shell.read(cx).active_view().clone());
    let status_index = visual.read(|cx| shell.read(cx).buffer_index(&status, cx).unwrap());

    let move_to = |visual: &mut VisualTestContext, needle: &str, delta: usize| {
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.activate(status_index, window, cx);
            });
            let active = shell.read(cx).active_view().clone();
            active.update(cx, |view, cx| {
                let text = view.buffer().text().to_string();
                let offset = text
                    .find(needle)
                    .unwrap_or_else(|| panic!("missing {needle:?} in:\n{text}"));
                view.show_location(crate::text::ByteOffset(offset + delta), window, cx);
            });
        });
    };

    // Head header opens HEAD revision buffer, and q closes it back to status.
    move_to(&mut visual, "Head:     ", 0);
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert!(matches!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .history_query(),
            Some(crate::history::Query::Revision { .. })
        ));
    });
    visual.simulate_keystrokes("q");

    // Untracked group heading has nothing to visit.
    move_to(&mut visual, "Untracked files", 0);
    visual.simulate_keystrokes("enter");
    visual.read(|cx| {
        assert_eq!(
            status.read(cx).message(),
            Some("There is no thing at point that could be visited")
        );
    });

    // Untracked regular file opens editable worktree buffer in the same pane.
    let panes_before = visual.read(|cx| shell.read(cx).panes.leaves().len());
    move_to(&mut visual, "new.txt", 0);
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.panes.leaves().len(), panes_before);
        let buf = shell.active_view().read(cx).buffer();
        assert_eq!(
            buf.path().as_deref(),
            Some(repo.0.join("new.txt").as_path())
        );
        assert_eq!(buf.text().to_string(), "untracked body\n");
        assert!(!buf.is_read_only());
    });

    // Untracked plain directory reports missing dired cleanly.
    move_to(&mut visual, "plain_dir/", 0);
    visual.simulate_keystrokes("enter");
    visual.read(|cx| {
        assert!(
            status
                .read(cx)
                .message()
                .unwrap()
                .contains("Cannot visit directory")
        );
    });

    // Collapsed unstaged file heading opens editable worktree file at the first changed line (line 3).
    move_to(&mut visual, "modified   both.txt", 0);
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(
            buf.path().as_deref(),
            Some(repo.0.join("both.txt").as_path())
        );
        assert!(!buf.is_read_only());
        assert_eq!(buf.line_column(), (3, 0));
    });

    // Expand unstaged both.txt: RET on +worktree visits worktree at line 3 col 2;
    // RET on -indexed visits read-only index blob at line 3 col 2.
    move_to(&mut visual, "modified   both.txt", 0);
    visual.simulate_keystrokes("tab");
    move_to(&mut visual, "+worktree", 3);
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(
            buf.path().as_deref(),
            Some(repo.0.join("both.txt").as_path())
        );
        assert!(!buf.is_read_only());
        assert_eq!(buf.line_column(), (3, 2));
    });

    move_to(&mut visual, "-indexed", 3);
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        assert!(buf.is_read_only());
        assert_eq!(buf.text().to_string(), "line1\nline2\nindexed\nline4\n");
        assert_eq!(buf.line_column(), (3, 2));
    });

    // Unstaged deleted file visits read-only index blob.
    move_to(&mut visual, "deleted    del.txt", 0);
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        assert!(buf.is_read_only());
        assert_eq!(buf.text().to_string(), "gone\n");
    });

    // Staged file heading opens read-only index blob at first changed line (line 3);
    // expanding staged file and pressing RET on -old opens HEAD blob at line 3 col 1.
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.activate(status_index, window, cx);
        });
        status.update(cx, |view, cx| {
            let text = view.buffer().text().to_string();
            let staged_pos = text.find("Staged changes").unwrap();
            let file_pos = staged_pos + text[staged_pos..].find("modified   both.txt").unwrap();
            view.show_location(crate::text::ByteOffset(file_pos), window, cx);
        });
    });
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        assert!(buf.is_read_only());
        assert_eq!(buf.text().to_string(), "line1\nline2\nindexed\nline4\n");
        assert_eq!(buf.line_column(), (3, 0));
    });

    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.activate(status_index, window, cx);
        });
        status.update(cx, |view, cx| {
            let text = view.buffer().text().to_string();
            let staged_pos = text.find("Staged changes").unwrap();
            let file_pos = staged_pos + text[staged_pos..].find("modified   both.txt").unwrap();
            view.show_location(crate::text::ByteOffset(file_pos), window, cx);
        });
    });
    visual.simulate_keystrokes("tab");
    move_to(&mut visual, "-old", 2);
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        assert!(buf.is_read_only());
        assert_eq!(buf.text().to_string(), "line1\nline2\nold\nline4\n");
        assert_eq!(buf.line_column(), (3, 1));
    });

    // RET on Unstaged changes heading opens magit-diff-unstaged; RET on Staged changes opens magit-diff-staged.
    move_to(&mut visual, "Unstaged changes", 0);
    command(&mut visual, &shell, Command::MagitDiffVisit);
    visual.read(|cx| {
        assert_eq!(
            status.read(cx).message(),
            Some("No supported file location here")
        );
    });
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .comparison()
                .unwrap()
                .kind,
            crate::diff::DiffKind::Unstaged
        );
    });

    move_to(&mut visual, "Staged changes", 0);
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert_eq!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .comparison()
                .unwrap()
                .kind,
            crate::diff::DiffKind::Staged
        );
    });
}

#[gpui::test]
fn visit_file_other_window_keeps_status_visible_and_supports_init_js_rebinding(
    cx: &mut TestAppContext,
) {
    let repo = repo();
    repo.write("staged.txt", "before\n");
    repo.commit();
    repo.write("staged.txt", "after\n");
    repo.git(&["add", "staged.txt"]);
    repo.write("untracked.txt", "hello other window\n");

    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let status_pane = visual.read(|cx| *shell.read(cx).panes.selected());
    let status_id = visual.read(|cx| shell.read(cx).active_buffer_id());
    assert_eq!(visual.read(|cx| shell.read(cx).panes.leaves()), vec![0, 1]);

    // C-x 4 RET on untracked file opens it in pane 0 and keeps status visible in pane 1.
    visual.update(|window, cx| {
        let active = shell.read(cx).active_view().clone();
        active.update(cx, |view, cx| {
            let offset = view
                .buffer()
                .text()
                .to_string()
                .find("untracked.txt")
                .unwrap();
            view.show_location(crate::text::ByteOffset(offset), window, cx);
        });
    });
    visual.simulate_keystrokes("ctrl-x 4 enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.panes.leaves(), vec![0, 1]);
        assert_eq!(*shell.panes.selected(), 0);
        assert_eq!(shell.body_panes[&status_pane].current, status_id);
        let buf = shell.active_view().read(cx).buffer();
        assert_eq!(
            buf.path().as_deref(),
            Some(repo.0.join("untracked.txt").as_path())
        );
    });

    // Global C-x bindings still fall through from the status mode layer, and C-x 4 RET on a staged file
    // opens the read-only index blob in pane 0 while status stays in pane 1.
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.select_pane(status_pane, window, cx));
        let active = shell.read(cx).active_view().clone();
        active.update(cx, |view, cx| {
            let text = view.buffer().text().to_string();
            let staged_pos = text.find("Staged changes").unwrap();
            let offset = staged_pos + text[staged_pos..].find("modified   staged.txt").unwrap();
            view.show_location(crate::text::ByteOffset(offset), window, cx);
        });
    });
    visual.simulate_keystrokes("ctrl-x 4 enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.panes.leaves(), vec![0, 1]);
        assert_eq!(*shell.panes.selected(), 0);
        assert_eq!(shell.body_panes[&status_pane].current, status_id);
        let buf = shell.active_view().read(cx).buffer();
        assert!(buf.is_read_only());
        assert_eq!(buf.text().to_string(), "after\n");
    });

    // When status is the only pane (after C-x 1), C-x 4 RET splits and keeps status visible.
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_pane(status_pane, window, cx);
            shell.window_command(Command::DeleteOtherWindows, window, cx);
        });
        window.draw(cx).clear(cx);
    });
    visual.read(|cx| assert_eq!(shell.read(cx).panes.leaves().len(), 1));
    visual.update(|window, cx| {
        let active = shell.read(cx).active_view().clone();
        active.update(cx, |view, cx| {
            let offset = view
                .buffer()
                .text()
                .to_string()
                .find("untracked.txt")
                .unwrap();
            view.show_location(crate::text::ByteOffset(offset), window, cx);
        });
    });
    visual.simulate_keystrokes("ctrl-x 4 enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.panes.leaves().len(), 2);
        assert_ne!(*shell.panes.selected(), status_pane);
        assert_eq!(shell.body_panes[&status_pane].current, status_id);
    });

    // Rebinding RET to magit-diff-visit-file-other-window via init.js works in magit-status.
    let init = repo.0.join("init.js");
    std::fs::write(
        &init,
        "semacs.bindKey('enter', 'magit-diff-visit-file-other-window', { mode: 'magit-status' });",
    )
    .unwrap();
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_pane(status_pane, window, cx);
            shell.config_path = Some(init.clone());
            let origin = shell.active_view().clone();
            shell.reload_configuration(&origin, window, cx);
        });
    });
    until(&mut visual, &shell, |shell| shell.reload_task.is_none());
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_ne!(*shell.panes.selected(), status_pane);
        assert_eq!(shell.body_panes[&status_pane].current, status_id);
        assert_eq!(
            shell.active_view().read(cx).buffer().path().as_deref(),
            Some(repo.0.join("untracked.txt").as_path())
        );
    });
}

#[gpui::test]
fn status_cx_g_initial_point_jump_menu_sibling_and_level_folding(cx: &mut TestAppContext) {
    let repo = repo();
    repo.write("staged.txt", "v1\n");
    repo.commit();
    repo.git(&["tag", "v1"]);
    repo.write("stash.txt", "wip\n");
    repo.git(&["add", "stash.txt"]);
    repo.git(&["stash", "push", "-q", "-m", "wip stash"]);
    repo.write("a", "disk modified\n");
    repo.write("staged.txt", "v2\n");
    repo.git(&["add", "staged.txt"]);
    repo.write("untracked.txt", "new\n");

    let (mut visual, shell) = open(&repo, cx);
    visual.simulate_keystrokes("ctrl-x g");
    wait(&mut visual, &shell);

    let status = visual.read(|cx| shell.read(cx).active_view().clone());
    visual.read(|cx| {
        let buf = status.read(cx).buffer();
        assert_eq!(buf.mode(), ModeId::MAGIT_STATUS);
        assert!(buf.display_name().starts_with("magit: "), "{}", buf.display_name());
        let text = buf.text().to_string();
        assert!(text.contains("Head:     "));
        assert!(text.contains("Tag:      v1"));
        let sections = buf.sections().unwrap();
        let untracked = sections.nodes().iter().find(|n| n.key == "untracked").unwrap();
        assert!(!untracked.collapsed);
        assert_eq!(buf.point(), untracked.range.start);
    });

    // `j` jump menu jumps to sections or echoes missing section.
    visual.simulate_keystrokes("j z");
    visual.read(|cx| {
        let buf = status.read(cx).buffer();
        let stashes = buf
            .sections()
            .unwrap()
            .nodes()
            .iter()
            .find(|n| n.key == "stashes")
            .unwrap()
            .range
            .start;
        assert_eq!(buf.point(), stashes);
    });
    visual.simulate_keystrokes("j u");
    visual.read(|cx| {
        let buf = status.read(cx).buffer();
        let unstaged = buf
            .sections()
            .unwrap()
            .nodes()
            .iter()
            .find(|n| n.key == "unstaged")
            .unwrap()
            .range
            .start;
        assert_eq!(buf.point(), unstaged);
    });
    visual.simulate_keystrokes("j n");
    visual.read(|cx| {
        let buf = status.read(cx).buffer();
        let untracked = buf
            .sections()
            .unwrap()
            .nodes()
            .iter()
            .find(|n| n.key == "untracked")
            .unwrap()
            .range
            .start;
        assert_eq!(buf.point(), untracked);
    });
    visual.simulate_keystrokes("j m");
    visual.read(|cx| {
        assert!(status.read(cx).message().unwrap().contains("Unmerged"));
    });

    // `M-n` / `M-p` skip children to sibling/next section, `1`..`4` / `M-1` / `C-tab` / `M-tab` control visibility.
    visual.simulate_keystrokes("alt-n");
    visual.read(|cx| {
        let buf = status.read(cx).buffer();
        let unstaged = buf
            .sections()
            .unwrap()
            .nodes()
            .iter()
            .find(|n| n.key == "unstaged")
            .unwrap()
            .range
            .start;
        assert_eq!(buf.point(), unstaged);
    });
    visual.simulate_keystrokes("alt-p");
    visual.read(|cx| {
        let buf = status.read(cx).buffer();
        let untracked = buf
            .sections()
            .unwrap()
            .nodes()
            .iter()
            .find(|n| n.key == "untracked")
            .unwrap()
            .range
            .start;
        assert_eq!(buf.point(), untracked);
    });
    visual.simulate_keystrokes("alt-n 2");
    visual.read(|cx| {
        let sections = status.read(cx).buffer().sections().unwrap();
        assert!(!sections.nodes().iter().find(|n| n.key == "unstaged").unwrap().collapsed);
        assert!(
            sections
                .nodes()
                .iter()
                .find(|n| n.key.starts_with("unstaged/file/"))
                .unwrap()
                .collapsed
        );
    });
    visual.simulate_keystrokes("3");
    visual.read(|cx| {
        let sections = status.read(cx).buffer().sections().unwrap();
        assert!(
            !sections
                .nodes()
                .iter()
                .find(|n| n.key.starts_with("unstaged/file/"))
                .unwrap()
                .collapsed
        );
        assert!(
            sections
                .nodes()
                .iter()
                .find(|n| n.key.contains("/@@ "))
                .unwrap()
                .collapsed
        );
    });
    visual.simulate_keystrokes("4");
    visual.read(|cx| {
        let sections = status.read(cx).buffer().sections().unwrap();
        assert!(
            !sections
                .nodes()
                .iter()
                .find(|n| n.key.contains("/@@ "))
                .unwrap()
                .collapsed
        );
    });
    visual.simulate_keystrokes("alt-1");
    visual.read(|cx| {
        let sections = status.read(cx).buffer().sections().unwrap();
        assert!(sections.nodes().iter().find(|n| n.key == "unstaged").unwrap().collapsed);
        assert!(sections.nodes().iter().find(|n| n.key == "untracked").unwrap().collapsed);
    });
    visual.simulate_keystrokes("ctrl-tab");
    visual.read(|cx| {
        let sections = status.read(cx).buffer().sections().unwrap();
        assert!(!sections.nodes().iter().find(|n| n.key == "unstaged").unwrap().collapsed);
        assert!(
            sections
                .nodes()
                .iter()
                .find(|n| n.key.starts_with("unstaged/file/"))
                .unwrap()
                .collapsed
        );
    });
    visual.simulate_keystrokes("alt-tab");
    visual.read(|cx| {
        let sections = status.read(cx).buffer().sections().unwrap();
        assert!(
            !sections
                .nodes()
                .iter()
                .find(|n| n.key.starts_with("unstaged/file/"))
                .unwrap()
                .collapsed
        );
    });
}

#[gpui::test]
fn status_successor_worktree_visit_copy_preview_scroll_and_double_click(cx: &mut TestAppContext) {
    let repo = repo();
    repo.write(
        "long.txt",
        format!(
            "{}\n",
            (1..=120)
                .map(|n| format!("line {n}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    );
    repo.commit();
    repo.write("b.txt", "b0\n");
    repo.write("c.txt", "c0\n");
    repo.commit();
    repo.write("a", "disk modified\n");
    repo.write("b.txt", "b1\n");
    repo.write("c.txt", "c1\n");
    repo.git(&["add", "c.txt"]);

    let (mut visual, shell) = open(&repo, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let status = visual.read(|cx| shell.read(cx).active_view().clone());
    let status_pane = visual.read(|cx| *shell.read(cx).panes.selected());

    // Staging first unstaged file (`a`) moves point to next sibling (`b.txt`);
    // staging last unstaged file (`b.txt`) moves point to paired `Staged changes`.
    visual.simulate_keystrokes("n");
    visual.simulate_keystrokes("s");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let buf = status.read(cx).buffer();
        let sections = buf.sections().unwrap();
        assert_eq!(
            sections.nodes()[sections.current(buf.point())].key,
            sections
                .nodes()
                .iter()
                .find(|n| n.key.starts_with("unstaged/file/") && n.key.ends_with("622e747874"))
                .unwrap()
                .key
        );
    });
    visual.simulate_keystrokes("s");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let buf = status.read(cx).buffer();
        let sections = buf.sections().unwrap();
        assert_eq!(sections.nodes()[sections.current(buf.point())].key, "staged");
    });

    // Double-click heading toggles collapsed state without leaving a mark/region.
    let heading_pos = visual.read(|cx| {
        let view = status.read(cx);
        let staged = view
            .buffer()
            .sections()
            .unwrap()
            .nodes()
            .iter()
            .find(|n| n.key == "staged")
            .unwrap()
            .range
            .start;
        let bounds = view
            .viewport_layout()
            .unwrap()
            .bounds_for_range(staged..staged, view.wrap_index().unwrap())
            .unwrap();
        gpui_kit::point(bounds.left() + gpui_kit::px(4.), bounds.top() + gpui_kit::px(4.))
    });
    visual.simulate_event(gpui_kit::MouseDownEvent {
        position: heading_pos,
        button: gpui_kit::MouseButton::Left,
        modifiers: gpui_kit::Modifiers::default(),
        click_count: 2,
        first_mouse: false,
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.read(|cx| {
        let buf = status.read(cx).buffer();
        assert!(
            buf.sections()
                .unwrap()
                .nodes()
                .iter()
                .find(|n| n.key == "staged")
                .unwrap()
                .collapsed
        );
        assert!(!buf.mark_active());
    });
    visual.simulate_keystrokes("tab");

    // `C-w` copies current branch / file path to KillRing.
    visual.update(|window, cx| {
        status.update(cx, |view, cx| {
            let head = view
                .buffer()
                .sections()
                .unwrap()
                .nodes()
                .iter()
                .find(|n| n.key == "header/head")
                .unwrap()
                .range
                .start;
            view.show_location(head, window, cx);
        });
    });
    visual.simulate_keystrokes("ctrl-w");
    visual.read(|cx| {
        let copied = cx
            .global::<crate::editing::KillRing>()
            .current_kill(None)
            .unwrap()
            .to_string();
        assert!(!copied.is_empty());
        assert_eq!(status.read(cx).message(), Some(copied.as_str()));
    });

    // `C-<return>` on staged `c.txt` visits editable worktree buffer instead of index blob.
    visual.update(|window, cx| {
        status.update(cx, |view, cx| {
            let offset = view
                .buffer()
                .text()
                .to_string()
                .find("modified   c.txt")
                .unwrap();
            view.show_location(crate::text::ByteOffset(offset), window, cx);
        });
    });
    visual.simulate_keystrokes("ctrl-enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        let buf = shell.read(cx).active_view().read(cx).buffer();
        assert_eq!(buf.path().as_deref(), Some(repo.0.join("c.txt").as_path()));
        assert!(!buf.is_read_only());
    });

    // Switch back to status, expand `Recent commits`, move to the commit touching `long.txt`,
    // and press SPC twice: opens commit preview in the other pane without stealing focus,
    // and second SPC scrolls the preview forward.
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let idx = shell.buffer_index(&status, cx).unwrap();
            shell.activate(idx, window, cx);
        });
        status.update(cx, |view, cx| {
            let text = view.buffer().text().to_string();
            let recent = text.find("Recent commits").unwrap();
            view.show_location(crate::text::ByteOffset(recent), window, cx);
        });
    });
    visual.simulate_keystrokes("tab");
    visual.update(|window, cx| {
        status.update(cx, |view, cx| {
            let text = view.buffer().text().to_string();
            let recent = text.find("Recent commits").unwrap();
            let next_line = recent + text[recent..].find('\n').unwrap() + 1;
            let second_commit = next_line + text[next_line..].find('\n').unwrap() + 1;
            view.show_location(crate::text::ByteOffset(second_commit), window, cx);
        });
    });
    visual.simulate_keystrokes("space");
    wait(&mut visual, &shell);
    let preview_scroll_1 = visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(*shell.panes.selected(), status_pane);
        let preview = shell
            .all_views()
            .into_iter()
            .find(|v| {
                *v != status
                    && matches!(
                        v.read(cx).buffer().history_query(),
                        Some(crate::history::Query::Revision { .. })
                    )
            })
            .expect("preview revision view open");
        preview.read(cx).buffer().point()
    });
    visual.simulate_keystrokes("space");
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.read(|cx| {
        let shell = shell.read(cx);
        assert_eq!(*shell.panes.selected(), status_pane);
        let preview = shell
            .all_views()
            .into_iter()
            .find(|v| {
                *v != status
                    && matches!(
                        v.read(cx).buffer().history_query(),
                        Some(crate::history::Query::Revision { .. })
                    )
            })
            .unwrap();
        assert!(preview.read(cx).buffer().point() > preview_scroll_1);
    });
}

#[gpui::test]
fn status_remote_jump_keys_and_range_heading_open_diff_range(cx: &mut TestAppContext) {
    let remote = crate::diff::tests::Repository::new();
    remote.write("base.txt", "1\n");
    remote.commit();
    let branch = String::from_utf8(remote.git(&["branch", "--show-current"]))
        .unwrap()
        .trim()
        .to_owned();
    remote.git(&["config", "receive.denyCurrentBranch", "ignore"]);

    let local = crate::diff::tests::Repository::new();
    local.git(&["remote", "add", "origin", remote.0.to_str().unwrap()]);
    local.git(&["fetch", "-q", "origin"]);
    local.git(&["checkout", "-qb", &branch, &format!("origin/{branch}")]);
    local.git(&["branch", "--set-upstream-to", &format!("origin/{branch}")]);
    local.git(&["config", &format!("branch.{branch}.pushRemote"), "origin"]);

    remote.write("up.txt", "remote\n");
    remote.commit();
    local.git(&["fetch", "-q", "origin"]);
    local.write("loc.txt", "local\n");
    local.commit();

    let (mut visual, shell) = open(&local, cx);
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    let status = visual.read(|cx| shell.read(cx).active_view().clone());

    visual.simulate_keystrokes("j p u");
    visual.read(|cx| {
        let buf = status.read(cx).buffer();
        let sections = buf.sections().unwrap();
        assert_eq!(
            sections.nodes()[sections.current(buf.point())].key,
            "unpushed-upstream"
        );
    });
    visual.simulate_keystrokes("j f u");
    visual.read(|cx| {
        let buf = status.read(cx).buffer();
        let sections = buf.sections().unwrap();
        assert_eq!(
            sections.nodes()[sections.current(buf.point())].key,
            "unpulled-upstream"
        );
    });
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    visual.read(|cx| {
        assert!(matches!(
            shell
                .read(cx)
                .active_view()
                .read(cx)
                .buffer()
                .comparison()
                .unwrap()
                .kind,
            crate::diff::DiffKind::Range { .. }
        ));
    });
}
