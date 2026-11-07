use super::*;

#[gpui::test]
fn revisiting_changed_message_queries_before_replacing_clean_or_dirty_shared_text(
    cx: &mut TestAppContext,
) {
    for dirty in [false, true] {
        let (fixture, mut visual, shell, editor, _old_peer) = setup(cx);
        if dirty {
            replace(&mut visual, &shell, &editor, "unsaved draft\n");
        }
        command(&mut visual, &shell, Command::SplitWindowRight);
        let other = visual.read(|cx| shell.read(cx).active_view().clone());
        visual.update(|_, cx| shell.update(cx, |shell, _| shell.commit_editors.clear()));
        fs::write(fixture.path(), "new disk subject\n").unwrap();
        let (request, mut peer) = EditorRequest::test_pair(fixture.path());
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.edit_commit_message(request, "#".into(), window, cx);
            })
        });
        until(&mut visual, &shell, |shell| shell.session.is_some());
        visual.read(|cx| {
            assert!(matches!(shell.read(cx).session().unwrap().kind(),
                ReadKind::CommitReload { modified, .. } if *modified == dirty));
        });
        pending(&mut peer);
        visual.simulate_input("yes");
        visual.simulate_keystrokes("enter");
        assert_eq!(text(&visual, &editor), "new disk subject\n");
        assert_eq!(text(&visual, &other), "new disk subject\n");
        visual.read(|cx| {
            assert!(!editor.read(cx).buffer().is_modified());
            assert_eq!(
                editor.read(cx).buffer().disk_stamp(),
                crate::files::stamp(&fixture.path()).unwrap()
            );
        });
        command(&mut visual, &shell, Command::CommitFinish);
        wait(&mut visual, &shell);
        reply(&mut peer, 0);
        assert_eq!(
            fs::read_to_string(fixture.path()).unwrap(),
            "new disk subject\n"
        );
    }
}

#[gpui::test]
fn declining_reread_keeps_dirty_draft_and_existing_save_conflict_query(cx: &mut TestAppContext) {
    let (fixture, mut visual, shell, editor, _old_peer) = setup(cx);
    replace(&mut visual, &shell, &editor, "keep draft\n");
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.commit_editors.clear()));
    fs::write(fixture.path(), "changed by Git\n").unwrap();
    let (request, mut peer) = EditorRequest::test_pair(fixture.path());
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.edit_commit_message(request, "#".into(), window, cx);
        })
    });
    until(&mut visual, &shell, |shell| shell.session.is_some());
    visual.simulate_input("no");
    visual.simulate_keystrokes("enter");
    assert_eq!(text(&visual, &editor), "keep draft\n");
    command(&mut visual, &shell, Command::CommitFinish);
    until(&mut visual, &shell, |shell| {
        shell
            .session
            .as_ref()
            .is_some_and(|session| matches!(session.kind(), ReadKind::ExternalChange(_)))
    });
    pending(&mut peer);
    visual.simulate_input("yes");
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    reply(&mut peer, 0);
    assert_eq!(fs::read_to_string(fixture.path()).unwrap(), "keep draft\n");
}

#[gpui::test]
fn late_revisit_never_replaces_another_read_and_finish_can_resume_canceled_query(
    cx: &mut TestAppContext,
) {
    let (fixture, mut visual, shell, editor, _old_peer) = setup(cx);
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.commit_editors.clear()));
    fs::write(fixture.path(), "fresh message\n").unwrap();
    let (request, mut peer) = EditorRequest::test_pair(fixture.path());
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.edit_commit_message(request, "#".into(), window, cx);
            shell.begin_read(ReadKind::FindFile, &editor, "keep this", window, cx);
        })
    });
    until(&mut visual, &shell, |shell| {
        !shell.commit_editors.is_empty()
    });
    visual.read(|cx| {
        assert_eq!(
            shell.read(cx).session().unwrap().kind(),
            &ReadKind::FindFile
        );
        assert_eq!(
            shell.read(cx).session_input(cx).as_deref(),
            Some("keep this")
        );
    });
    visual.simulate_keystrokes("ctrl-g ctrl-c ctrl-c");
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().unwrap().kind(),
            ReadKind::CommitReload { .. }
        ))
    });
    visual.simulate_keystrokes("ctrl-g ctrl-c ctrl-c");
    pending(&mut peer);
    visual.simulate_input("yes");
    visual.simulate_keystrokes("enter");
    wait(&mut visual, &shell);
    reply(&mut peer, 0);
    assert_eq!(
        fs::read_to_string(fixture.path()).unwrap(),
        "fresh message\n"
    );
}

#[gpui::test]
fn reread_confirmation_does_not_discard_edits_committed_after_query(cx: &mut TestAppContext) {
    let (fixture, mut visual, shell, editor, _old_peer) = setup(cx);
    visual.update(|_, cx| shell.update(cx, |shell, _| shell.commit_editors.clear()));
    fs::write(fixture.path(), "fresh message\n").unwrap();
    let (request, mut peer) = EditorRequest::test_pair(fixture.path());
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.edit_commit_message(request, "#".into(), window, cx);
        })
    });
    until(&mut visual, &shell, |shell| shell.session.is_some());
    replace(&mut visual, &shell, &editor, "new draft during query\n");
    visual.simulate_input("yes");
    visual.simulate_keystrokes("enter");
    assert_eq!(text(&visual, &editor), "new draft during query\n");
    visual.read(|cx| {
        assert!(matches!(
            shell.read(cx).session().unwrap().kind(),
            ReadKind::CommitReload { modified: true, .. }
        ))
    });
    pending(&mut peer);
    visual.simulate_input("yes");
    visual.simulate_keystrokes("enter ctrl-c ctrl-k");
    wait(&mut visual, &shell);
    reply(&mut peer, 1);
}
