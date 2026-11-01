#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gpui_kit::{
    Entity, Focusable, Keystroke, Modifiers, TestAppContext, VisualTestContext, px, size,
};
use semacs::app::AppShell;
use semacs::buffer::Buffer;
use semacs::files::{self, DecodedFile, FileFormat};
use semacs::view::BufferView;

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "semacs-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn decoded(text: &str) -> DecodedFile {
    DecodedFile {
        text: text.into(),
        format: FileFormat::default(),
    }
}

pub fn open(
    buffer: Buffer,
    cx: &mut TestAppContext,
) -> (VisualTestContext, Entity<AppShell>, Entity<BufferView>) {
    let buffer = match buffer.path().map(Path::to_path_buf) {
        Some(path) => buffer.with_disk_stamp(files::stamp(&path).unwrap()),
        None => buffer,
    };
    cx.update(semacs::app::init);
    let window = cx.open_window(size(px(400.), px(240.)), |window, cx| {
        AppShell::new(buffer, window, cx)
    });
    let shell = window.entity(cx).unwrap();
    let view = shell.read_with(cx, |shell, _| shell.active_view().clone());
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, _| window.activate_window());
    redraw(&mut visual);
    (visual, shell, view)
}

pub fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

pub fn dispatch_keys(visual: &mut VisualTestContext, keys: &[&'static str]) {
    visual.update(|window, cx| {
        for key in keys {
            window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
        }
    });
}

pub fn dispatch_text(visual: &mut VisualTestContext, text: &str) {
    visual.update(|window, cx| {
        for ch in text.chars() {
            let key = ch.to_string();
            window.dispatch_keystroke(
                Keystroke {
                    modifiers: Modifiers::default(),
                    key: key.clone(),
                    key_char: Some(key),
                },
                cx,
            );
        }
    });
}

pub fn active_view(visual: &mut VisualTestContext, shell: &Entity<AppShell>) -> Entity<BufferView> {
    redraw(visual);
    visual.read(|cx| shell.read(cx).active_view().clone())
}

pub fn create(
    visual: &mut VisualTestContext,
    shell: &Entity<AppShell>,
    text: &str,
    path: Option<&Path>,
) -> usize {
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let buffer = path.map_or_else(
                || Buffer::new(None, decoded(text)),
                |path| {
                    Buffer::visiting(
                        path.to_path_buf(),
                        decoded(text),
                        files::stamp(path).unwrap(),
                    )
                },
            );
            shell.create_buffer(buffer, window, cx)
        })
    })
}

pub fn activate(visual: &mut VisualTestContext, shell: &Entity<AppShell>, index: usize) {
    let activated =
        visual.update(|window, cx| shell.update(cx, |shell, cx| shell.activate(index, window, cx)));
    assert!(activated, "buffer {index} should activate");
    redraw(visual);
}

pub fn find_file(visual: &mut VisualTestContext, path: &Path) {
    visual.simulate_keystrokes("ctrl-x ctrl-f");
    visual.simulate_input(path.to_str().unwrap());
    visual.simulate_keystrokes("enter");
    redraw(visual);
}

pub fn buffer_at(path: &Path, text: &str) -> Buffer {
    Buffer::visiting(
        path.to_path_buf(),
        decoded(text),
        files::stamp(path).unwrap(),
    )
}

pub fn assert_focused(visual: &mut VisualTestContext, view: &Entity<BufferView>) {
    visual.update(|window, cx| assert!(view.read(cx).focus_handle(cx).is_focused(window)));
}

pub fn prompt(visual: &mut VisualTestContext, shell: &Entity<AppShell>) -> String {
    visual.read(|cx| {
        shell
            .read(cx)
            .minibuffer()
            .expect("open minibuffer")
            .read(cx)
            .prompt(cx)
    })
}

pub fn contents(visual: &mut VisualTestContext, shell: &Entity<AppShell>) -> String {
    visual.read(|cx| {
        shell
            .read(cx)
            .minibuffer()
            .expect("open minibuffer")
            .read(cx)
            .contents(cx)
    })
}

pub fn type_path(visual: &mut VisualTestContext, path: &Path) {
    visual.simulate_input(path.to_str().unwrap());
}
