use super::*;
use crate::app::git_test_support::*;
use crate::command::Command;
use crate::mode::ModeId;
use gpui_kit as gpui;
use gpui_kit::TestAppContext;

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
            !shell
                .read(cx)
                .commands
                .commands_for(ModeId::MAGIT_STATUS)
                .any(|entry| entry.name == "magit-diff-visit-file")
        );
        buffer.snapshot_id().buffer
    });
    command(&mut visual, &shell, Command::MagitStatus);
    wait(&mut visual, &shell);
    visual.read(|cx| assert_eq!(shell.read(cx).active_buffer_id(), id));
    visual.simulate_keystrokes("n n tab");
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
    command(&mut visual, &shell, Command::MagitDiffVisit);
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
    visual.simulate_keystrokes("n n tab");
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
                .find(|node| node.key == "untracked")
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
