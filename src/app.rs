use std::path::{Path, PathBuf};

use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::*;

use crate::echo::EchoArea;
use crate::files::{DecodedFile, FileFormat};
use crate::frame;

pub struct StartupDocument {
    path: Option<PathBuf>,
    text: String,
    format: FileFormat,
}

impl StartupDocument {
    pub fn scratch() -> Self {
        Self {
            path: None,
            text: String::new(),
            format: FileFormat::default(),
        }
    }

    pub fn visit(path: PathBuf, file: DecodedFile) -> Self {
        Self {
            path: Some(path),
            text: file.text,
            format: file.format,
        }
    }

    /// Emacs opens a new, empty buffer for a path that does not exist yet.
    pub fn new_file(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            text: String::new(),
            format: FileFormat::default(),
        }
    }

    pub fn title(&self) -> String {
        format!("{} - semacs", self.display_name())
    }

    pub fn display_name(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "*scratch*".to_owned())
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn format(&self) -> FileFormat {
        self.format
    }
}

pub struct AppShell {
    document: StartupDocument,
    echo: EchoArea,
}

impl AppShell {
    pub fn new(document: StartupDocument) -> Self {
        Self {
            document,
            echo: EchoArea::default(),
        }
    }

    pub fn document(&self) -> &StartupDocument {
        &self.document
    }
}

impl Render for AppShell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mode_line = frame::mode_line(self.document.display_name(), self.document.format(), cx);
        let echo_area = frame::echo_area(&self.echo, cx);

        v_flex()
            .id("app")
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                v_flex()
                    .id("editor-placeholder")
                    .flex_1()
                    .min_h_0()
                    .items_center()
                    .justify_center()
                    .text_color(cx.theme().muted_foreground)
                    .child("Text editing is not implemented yet"),
            )
            .child(mode_line)
            .child(echo_area)
    }
}

#[cfg(test)]
mod tests {
    use super::StartupDocument;
    use crate::files::{DecodedFile, FileFormat, LineEnding};
    use std::path::PathBuf;

    #[test]
    fn startup_documents_preserve_scratch_file_text_and_format_metadata() {
        let scratch = StartupDocument::scratch();
        assert_eq!(scratch.path(), None);
        assert_eq!(scratch.text(), "");
        assert_eq!(scratch.format(), FileFormat::default());
        assert_eq!(scratch.display_name(), "*scratch*");

        let path = PathBuf::from("notes.txt");
        let file = DecodedFile {
            text: "hello\nworld".to_owned(),
            format: FileFormat {
                bom: true,
                line_ending: LineEnding::Crlf,
            },
        };
        let expected_format = file.format;
        let document = StartupDocument::visit(path.clone(), file);
        assert_eq!(document.path(), Some(path.as_path()));
        assert_eq!(document.text(), "hello\nworld");
        assert_eq!(document.format(), expected_format);
        assert_eq!(document.display_name(), "notes.txt");
        assert_eq!(document.title(), "notes.txt - semacs");

        let new_file = StartupDocument::new_file(path.clone());
        assert_eq!(new_file.path(), Some(path.as_path()));
        assert_eq!(new_file.text(), "");
        assert_eq!(new_file.format(), FileFormat::default());
    }
}
