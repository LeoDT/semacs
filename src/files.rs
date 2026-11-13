//! File text encoding under D20.
//!
//! Text files are supported as UTF-8 with or without BOM and with uniform LF or
//! CRLF line endings. The in-memory text is always BOM-free UTF-8 with LF
//! newlines; the original format is metadata used when writing the file back.
//!
//! Mixed line endings are not supported yet. They are rejected rather than
//! normalized so opening a file can never silently rewrite it.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, MAIN_SEPARATOR, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineEnding {
    #[default]
    Lf,
    Crlf,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "LF",
            LineEnding::Crlf => "CRLF",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileFormat {
    pub bom: bool,
    pub line_ending: LineEnding,
}

impl FileFormat {
    pub fn as_str(self) -> String {
        let encoding = if self.bom { "UTF-8 BOM" } else { "UTF-8" };
        format!("{encoding} {}", self.line_ending.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedFile {
    pub text: String,
    pub format: FileFormat,
}

/// Metadata used to notice that a file changed outside the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStamp {
    len: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

/// The filesystem object a visited path refers to. Two spellings of the same
/// file (a symlink, `dir/../file`, a hard link) share an identity, so visiting
/// either must reuse the same buffer. Compare with a path only as a fallback on
/// platforms that do not expose a stable object identity.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileIdentity {
    device: u64,
    inode: u64,
}

#[cfg(not(unix))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileIdentity;

impl FileStamp {
    pub fn identity(self) -> Option<FileIdentity> {
        #[cfg(unix)]
        {
            Some(FileIdentity {
                device: self.device,
                inode: self.inode,
            })
        }
        #[cfg(not(unix))]
        {
            None
        }
    }
}

/// Decoded file text plus the metadata from the same open handle.
pub struct LoadedFile {
    pub file: DecodedFile,
    pub stamp: Option<FileStamp>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// A byte-order mark for an encoding we do not support (UTF-16 / UTF-32).
    UnsupportedEncoding(&'static str),
    /// The file is not UTF-8 and has no recognized unsupported-encoding BOM.
    InvalidUtf8,
    /// NUL bytes indicate a binary file or BOM-less UTF-16 / UTF-32 text.
    NulBytes,
    /// LF and CRLF are mixed, or a lone CR appears.
    MixedLineEndings,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::UnsupportedEncoding(name) => write!(
                f,
                "unsupported encoding {name}; only UTF-8 (with or without BOM) is supported"
            ),
            DecodeError::InvalidUtf8 => {
                write!(f, "file is not valid UTF-8; only UTF-8 is supported")
            }
            DecodeError::NulBytes => write!(
                f,
                "file contains NUL bytes; binary files and BOM-less UTF-16/32 are not supported"
            ),
            DecodeError::MixedLineEndings => write!(
                f,
                "file uses mixed or unsupported line endings; only uniform LF or CRLF is supported"
            ),
        }
    }
}

impl std::error::Error for DecodeError {}

#[derive(Debug)]
pub enum ReadError {
    Io(io::Error),
    Decode(DecodeError),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::Io(error) => write!(f, "{error}"),
            ReadError::Decode(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ReadError::Io(error) => Some(error),
            ReadError::Decode(error) => Some(error),
        }
    }
}

impl From<io::Error> for ReadError {
    fn from(error: io::Error) -> Self {
        ReadError::Io(error)
    }
}

impl From<DecodeError> for ReadError {
    fn from(error: DecodeError) -> Self {
        ReadError::Decode(error)
    }
}

/// Expands `~`, resolves a relative path against `default_directory`, and
/// simplifies `.` / `..` lexically. Symbolic links are left in place; callers
/// that need file identity compare [`FileIdentity`] instead of spelling.
pub fn expand(path: &Path, default_directory: &Path) -> io::Result<PathBuf> {
    let expanded = expand_tilde(path);
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        default_directory.join(expanded)
    };
    Ok(normalize(&absolute))
}

/// Expands a path against the process working directory.
pub fn absolute(path: &Path) -> io::Result<PathBuf> {
    expand(path, &std::env::current_dir()?)
}

/// `$HOME` at a whole-component prefix replaced by `~`, the spelling
/// `file-name-history` entries and the minibuffer use.
pub fn abbreviate(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home
        .as_deref()
        .and_then(|home| path.strip_prefix(home).ok())
    {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => Path::new("~").join(rest).to_string_lossy().into_owned(),
        None => path.to_string_lossy().into_owned(),
    }
}

/// A directory as it starts a file read: [`abbreviate`]d and always ending with
/// a separator.
pub fn directory_input(directory: &Path) -> String {
    let mut text = abbreviate(directory);
    if !text.ends_with(MAIN_SEPARATOR) {
        text.push(MAIN_SEPARATOR);
    }
    text
}

fn expand_tilde(path: &Path) -> PathBuf {
    let mut components = path.components();
    let Some(Component::Normal(first)) = components.next() else {
        return path.to_path_buf();
    };
    if first != "~" {
        return path.to_path_buf();
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return path.to_path_buf();
    };
    let rest = components.as_path();
    if rest.as_os_str().is_empty() {
        home
    } else {
        home.join(rest)
    }
}

fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

/// The file a `write-file` or `save-buffer` answer writes to. An unedited
/// answer uses the visited file, or the buffer name inside the default
/// directory; an answer naming a directory appends the same base name.
pub fn write_target(
    input: &str,
    initial: &str,
    directory: &Path,
    current: Option<&Path>,
    display_name: &str,
) -> io::Result<PathBuf> {
    let base_name = current
        .and_then(Path::file_name)
        .or_else(|| Path::new(display_name).file_name())
        .unwrap_or_default();
    let path = if input == initial {
        current.map_or_else(|| directory.join(base_name), Path::to_path_buf)
    } else {
        expand(Path::new(input), directory)?
    };
    Ok(if path.is_dir() {
        path.join(base_name)
    } else {
        path
    })
}

/// Metadata for an existing path, or `None` when it does not exist.
pub fn stamp(path: &Path) -> io::Result<Option<FileStamp>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(Some(file_stamp(&metadata)))
}

/// Identity of an existing path, or `None` when it does not exist.
pub fn identity(path: &Path) -> io::Result<Option<FileIdentity>> {
    Ok(stamp(path)?.and_then(FileStamp::identity))
}

fn file_stamp(metadata: &fs::Metadata) -> FileStamp {
    FileStamp {
        len: metadata.len(),
        modified: metadata.modified().ok(),
        #[cfg(unix)]
        device: {
            use std::os::unix::fs::MetadataExt;
            metadata.dev()
        },
        #[cfg(unix)]
        inode: {
            use std::os::unix::fs::MetadataExt;
            metadata.ino()
        },
    }
}

pub struct PathCandidates {
    pub candidates: Vec<crate::completion::Candidate>,
    pub query: String,
    pub typed_directory: String,
    pub directory_input: bool,
    pub raw_directory: bool,
}

/// Enumerates only the directory denoted by the typed prefix. Filtering is
/// separate so fuzzy matching never changes a directory component.
pub fn path_candidates(
    input: &str,
    default_directory: &Path,
    cancelled: &std::sync::atomic::AtomicBool,
) -> PathCandidates {
    let now = SystemTime::now();
    if input == "~" && std::env::var_os("HOME").is_some() {
        let annotation = expand(Path::new(input), default_directory)
            .ok()
            .and_then(|home| fs::symlink_metadata(home).ok())
            .map_or_else(String::new, |metadata| {
                crate::file_annotation::file(
                    &metadata,
                    now,
                    &mut crate::file_annotation::OwnerNames::default(),
                )
            });
        return PathCandidates {
            candidates: vec![crate::completion::Candidate {
                value: "~/".into(),
                label: "~/".into(),
                annotation,
                directory: true,
            }],
            query: "~".into(),
            typed_directory: String::new(),
            directory_input: false,
            raw_directory: true,
        };
    }
    let typed_directory = input
        .rfind(std::path::MAIN_SEPARATOR)
        .map_or("", |index| &input[..=index]);
    let query = input[typed_directory.len()..].to_owned();
    let mut result = PathCandidates {
        candidates: Vec::new(),
        query,
        typed_directory: typed_directory.to_owned(),
        directory_input: false,
        raw_directory: false,
    };
    let Ok(expanded) = expand(Path::new(input), default_directory) else {
        return result;
    };
    result.raw_directory = expanded.is_dir();
    let directory = if input.is_empty() || input.ends_with(std::path::MAIN_SEPARATOR) {
        expanded
    } else {
        expanded
            .parent()
            .unwrap_or(Path::new(std::path::MAIN_SEPARATOR_STR))
            .to_path_buf()
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return result;
    };
    result.directory_input = input.is_empty() || input.ends_with(std::path::MAIN_SEPARATOR);
    let mut names = crate::file_annotation::OwnerNames::default();
    for entry in entries.flatten() {
        if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        // `DirEntry::metadata` leaves symlinks unresolved like `file-attributes`,
        // while the trailing separator still follows a link to a directory.
        let metadata = entry.metadata().ok();
        let directory = match &metadata {
            Some(metadata) if metadata.file_type().is_symlink() => entry.path().is_dir(),
            Some(metadata) => metadata.is_dir(),
            None => false,
        };
        let annotation = metadata.as_ref().map_or_else(String::new, |metadata| {
            crate::file_annotation::file(metadata, now, &mut names)
        });
        let label = if directory { format!("{name}/") } else { name };
        result.candidates.push(crate::completion::Candidate {
            value: format!("{typed_directory}{label}"),
            label,
            annotation,
            directory,
        });
    }
    result
}

/// Ranks file candidates like Vertico's `vertico-sort-history-length-alpha` over
/// `file-name-history`: history first, then character length and name. Matching
/// stays separate, so fuzzy filtering never changes this order.
pub fn rank_candidates(
    items: &mut [crate::completion::MatchedCandidate],
    history: &[String],
    prefix: &str,
    current: Option<&str>,
) {
    // Vertico keys history entries by the segment after the typed directory, so
    // a history entry of `dir/file` ranks the `dir/` candidate, and the file of
    // the current buffer never counts.
    let ranks = crate::completion::history_ranks(history, |entry| {
        if current == Some(entry) {
            return None;
        }
        let rest = if prefix.is_empty() {
            Some(entry)
        } else {
            entry.strip_prefix(prefix)
        }?;
        Some(
            rest.find(std::path::MAIN_SEPARATOR)
                .map_or(rest, |end| &rest[..=end]),
        )
    });
    items.sort_by(|a, b| {
        let rank = |item: &crate::completion::MatchedCandidate| {
            ranks
                .get(item.candidate.label.as_str())
                .copied()
                .unwrap_or(i64::MAX)
        };
        rank(a)
            .cmp(&rank(b))
            .then_with(|| {
                a.candidate
                    .label
                    .chars()
                    .count()
                    .cmp(&b.candidate.label.chars().count())
            })
            .then_with(|| a.candidate.label.cmp(&b.candidate.label))
    });
}

pub fn read(path: &Path) -> Result<DecodedFile, ReadError> {
    Ok(load(path)?.file)
}

/// Reads and decodes a file, returning metadata from the same open handle so a
/// buffer starts with the identity and stamp of the content it actually read.
pub fn load(path: &Path) -> Result<LoadedFile, ReadError> {
    let mut file = fs::File::open(path)?;
    let metadata = file.metadata()?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
    Ok(LoadedFile {
        file: decode(&bytes)?,
        stamp: Some(file_stamp(&metadata)),
    })
}

pub fn decode(bytes: &[u8]) -> Result<DecodedFile, DecodeError> {
    if let Some(name) = unsupported_encoding_bom(bytes) {
        return Err(DecodeError::UnsupportedEncoding(name));
    }

    let (body, bom) = match bytes.strip_prefix(UTF8_BOM) {
        Some(body) => (body, true),
        None => (bytes, false),
    };
    let text = std::str::from_utf8(body).map_err(|_| DecodeError::InvalidUtf8)?;
    if text.as_bytes().contains(&0) {
        return Err(DecodeError::NulBytes);
    }
    let line_ending = detect_line_ending(text)?;
    let text = match line_ending {
        LineEnding::Lf => text.to_owned(),
        LineEnding::Crlf => text.replace("\r\n", "\n"),
    };

    Ok(DecodedFile {
        text,
        format: FileFormat { bom, line_ending },
    })
}

pub fn encode(text: &str, format: FileFormat) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 3);
    if format.bom {
        out.extend_from_slice(UTF8_BOM);
    }
    match format.line_ending {
        LineEnding::Lf => out.extend_from_slice(text.as_bytes()),
        LineEnding::Crlf => {
            for (index, line) in text.split('\n').enumerate() {
                if index > 0 {
                    out.extend_from_slice(b"\r\n");
                }
                out.extend_from_slice(line.as_bytes());
            }
        }
    }
    out
}

/// Writes through a same-directory temporary file, flushes it to disk and
/// renames it over the target. Returns the stamp of the file actually written;
/// a failure leaves the original file untouched.
///
/// Following a final symlink writes the real target and keeps the link. The
/// rename replaces the directory entry, so hard links to the old inode are not
/// updated; that is the deliberate cost of not truncating the original file
/// before the new bytes are safely on disk.
pub fn write_atomic(path: &Path, text: &str, format: FileFormat) -> io::Result<FileStamp> {
    let target = resolve_write_target(path)?;
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file name is empty"))?
        .to_string_lossy();
    let permissions = fs::metadata(&target)
        .ok()
        .map(|metadata| metadata.permissions());
    let bytes = encode(text, format);

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    for _ in 0..100 {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(".{name}.semacs-save-{}-{id}", std::process::id()));
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };

        let result = (|| {
            if let Some(permissions) = permissions {
                file.set_permissions(permissions)?;
            }
            file.write_all(&bytes)?;
            file.sync_all()?;
            let stamp = file_stamp(&file.metadata()?);
            drop(file);
            fs::rename(&temp, &target)?;
            Ok(stamp)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        return result;
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a temporary save file",
    ))
}

/// Resolves a final symlink chain without requiring the target to exist, so a
/// dangling link still writes its target instead of replacing the link.
fn resolve_write_target(path: &Path) -> io::Result<PathBuf> {
    const MAX_SYMLINKS: usize = 40;
    let mut current = path.to_path_buf();
    for _ in 0..MAX_SYMLINKS {
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let link = fs::read_link(&current)?;
                current = if link.is_absolute() {
                    link
                } else {
                    current
                        .parent()
                        .filter(|parent| !parent.as_os_str().is_empty())
                        .unwrap_or_else(|| Path::new("."))
                        .join(link)
                };
            }
            Ok(_) => return Ok(current),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(current),
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "too many levels of symbolic links",
    ))
}

fn unsupported_encoding_bom(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        Some("UTF-32 BE")
    } else if bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
        Some("UTF-32 LE")
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        Some("UTF-16 BE")
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        Some("UTF-16 LE")
    } else {
        None
    }
}

fn detect_line_ending(text: &str) -> Result<LineEnding, DecodeError> {
    let bytes = text.as_bytes();
    let mut lf = 0usize;
    let mut crlf = 0usize;
    let mut index = 0usize;

    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                crlf += 1;
                index += 2;
            }
            b'\r' => return Err(DecodeError::MixedLineEndings),
            b'\n' => {
                lf += 1;
                index += 1;
            }
            _ => index += 1,
        }
    }

    match (lf, crlf) {
        (_, 0) => Ok(LineEnding::Lf),
        (0, _) => Ok(LineEnding::Crlf),
        _ => Err(DecodeError::MixedLineEndings),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crlf(text: &str) -> Vec<u8> {
        text.replace('\n', "\r\n").into_bytes()
    }

    fn assert_roundtrip(bytes: &[u8], text: &str, format: FileFormat) {
        let decoded = decode(bytes).expect("decode");
        assert_eq!(decoded.text, text);
        assert_eq!(decoded.format, format);
        assert_eq!(encode(&decoded.text, decoded.format), bytes);
    }

    #[test]
    fn roundtrips_every_supported_format() {
        let content = "第一行\nsecond line\n\nemoji 😀\nlast";
        for bom in [false, true] {
            for trailing_newline in [false, true] {
                let mut text = content.to_owned();
                if trailing_newline {
                    text.push('\n');
                }

                for line_ending in [LineEnding::Lf, LineEnding::Crlf] {
                    let mut bytes = Vec::new();
                    if bom {
                        bytes.extend_from_slice(UTF8_BOM);
                    }
                    bytes.extend(match line_ending {
                        LineEnding::Lf => text.as_bytes().to_vec(),
                        LineEnding::Crlf => crlf(&text),
                    });

                    assert_roundtrip(&bytes, &text, FileFormat { bom, line_ending });
                }
            }
        }
    }

    #[test]
    fn empty_file_has_no_bom_and_lf() {
        assert_roundtrip(b"", "", FileFormat::default());
    }

    #[test]
    fn bom_only_file_keeps_the_bom_out_of_the_text() {
        let bytes = UTF8_BOM;
        assert_roundtrip(
            bytes,
            "",
            FileFormat {
                bom: true,
                line_ending: LineEnding::Lf,
            },
        );
    }

    #[test]
    fn crlf_roundtrips_blank_lines_and_trailing_newline() {
        let bytes = b"a\r\n\r\nb\r\n";
        assert_roundtrip(
            bytes,
            "a\n\nb\n",
            FileFormat {
                bom: false,
                line_ending: LineEnding::Crlf,
            },
        );
    }

    #[test]
    fn new_lines_use_the_buffer_format_and_do_not_duplicate_the_bom() {
        let decoded = decode(&[UTF8_BOM, b"a\r\nb"].concat()).expect("decode");
        let edited = format!("{}\nc\nd", decoded.text);
        let bytes = encode(&edited, decoded.format);
        assert_eq!(
            bytes,
            [UTF8_BOM, b"a\r\nb\r\nc\r\nd"].concat(),
            "existing and newly added line breaks keep CRLF, BOM is written once"
        );
    }

    #[test]
    fn lf_text_encoded_without_trailing_newline_stays_without_one() {
        let format = FileFormat {
            bom: false,
            line_ending: LineEnding::Lf,
        };
        assert_eq!(encode("a\nb", format), b"a\nb");
    }

    #[test]
    fn rejects_invalid_utf8() {
        assert_eq!(decode(&[0xFF, 0x00, 0x41]), Err(DecodeError::InvalidUtf8));
    }

    #[test]
    fn rejects_utf16_and_utf32_boms_explicitly() {
        assert_eq!(
            decode(&[0xFF, 0xFE, b'a', 0x00]),
            Err(DecodeError::UnsupportedEncoding("UTF-16 LE"))
        );
        assert_eq!(
            decode(&[0xFE, 0xFF, 0x00, b'a']),
            Err(DecodeError::UnsupportedEncoding("UTF-16 BE"))
        );
        assert_eq!(
            decode(&[0xFF, 0xFE, 0x00, 0x00, b'a', 0x00, 0x00, 0x00]),
            Err(DecodeError::UnsupportedEncoding("UTF-32 LE"))
        );
        assert_eq!(
            decode(&[0x00, 0x00, 0xFE, 0xFF, 0x00, 0x00, 0x00, b'a']),
            Err(DecodeError::UnsupportedEncoding("UTF-32 BE"))
        );
    }

    #[test]
    fn rejects_bomless_utf16_and_binary_nul_bytes() {
        assert_eq!(
            decode(b"a\0b\0"),
            Err(DecodeError::NulBytes),
            "BOM-less UTF-16 LE"
        );
        assert_eq!(
            decode(b"\0a\0b"),
            Err(DecodeError::NulBytes),
            "BOM-less UTF-16 BE"
        );
    }

    #[test]
    fn rejects_mixed_line_endings_and_lone_cr() {
        assert_eq!(decode(b"a\nb\r\nc"), Err(DecodeError::MixedLineEndings));
        assert_eq!(decode(b"a\rb"), Err(DecodeError::MixedLineEndings));
        assert_eq!(decode(b"a\r"), Err(DecodeError::MixedLineEndings));
    }

    #[test]
    fn format_labels_are_stable() {
        assert_eq!(FileFormat::default().as_str(), "UTF-8 LF");
        assert_eq!(
            FileFormat {
                bom: true,
                line_ending: LineEnding::Crlf,
            }
            .as_str(),
            "UTF-8 BOM CRLF"
        );
    }

    #[test]
    fn expand_uses_the_default_directory_and_simplifies_dots() {
        let default = Path::new("/tmp/semacs/default");
        assert_eq!(
            expand(Path::new("a/../b"), default).unwrap(),
            PathBuf::from("/tmp/semacs/default/b")
        );
        assert_eq!(
            expand(Path::new("./a/./b"), default).unwrap(),
            PathBuf::from("/tmp/semacs/default/a/b")
        );
        assert_eq!(
            expand(Path::new("/a/../b"), default).unwrap(),
            PathBuf::from("/b")
        );
    }

    #[test]
    fn expand_home_and_leave_missing_users_literal() {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return;
        };
        assert_eq!(
            expand(Path::new("~/notes"), Path::new("/tmp")).unwrap(),
            home.join("notes")
        );
        assert_eq!(
            expand(Path::new("~nobody/notes"), Path::new("/tmp")).unwrap(),
            PathBuf::from("/tmp/~nobody/notes")
        );
    }

    #[test]
    fn directory_input_abbreviates_only_whole_home_components() {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return;
        };
        assert_eq!(directory_input(&home), "~/");
        assert_eq!(
            directory_input(&home.join("src/semacs")),
            format!("~{MAIN_SEPARATOR}src{MAIN_SEPARATOR}semacs{MAIN_SEPARATOR}")
        );
        let similar = home.with_file_name(format!(
            "{}-elsewhere",
            home.file_name().unwrap().to_string_lossy()
        ));
        assert_eq!(
            directory_input(&similar),
            format!("{}{MAIN_SEPARATOR}", similar.display())
        );
        assert_eq!(
            directory_input(Path::new(std::path::MAIN_SEPARATOR_STR)),
            std::path::MAIN_SEPARATOR_STR
        );
    }

    #[test]
    fn write_target_uses_the_default_name_for_unedited_and_directory_answers() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "semacs-write-target-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let nested = dir.join("sub");
        fs::create_dir_all(&nested).unwrap();
        let current = dir.join("current.txt");
        let initial = directory_input(&dir);

        assert_eq!(
            write_target(&initial, &initial, &dir, Some(&current), "current.txt").unwrap(),
            current
        );
        assert_eq!(
            write_target(&initial, &initial, &dir, None, "draft").unwrap(),
            dir.join("draft")
        );
        assert_eq!(
            write_target(&initial, &initial, &dir, None, "sub/draft.txt").unwrap(),
            dir.join("draft.txt"),
            "a buffer name is reduced to its base name"
        );
        let nested_input = directory_input(&nested);
        assert_eq!(
            write_target(&nested_input, &initial, &dir, Some(&current), "current.txt").unwrap(),
            nested.join("current.txt"),
            "a directory answer appends the visited file name"
        );
        assert_eq!(
            write_target(
                &format!("{nested_input}other.txt"),
                &initial,
                &dir,
                None,
                "draft"
            )
            .unwrap(),
            nested.join("other.txt")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    fn ranked(
        labels: &[&str],
        history: &[&str],
        prefix: &str,
        current: Option<&str>,
    ) -> Vec<String> {
        let mut items: Vec<_> = labels
            .iter()
            .map(|label| crate::completion::MatchedCandidate {
                candidate: crate::completion::Candidate {
                    value: format!("{prefix}{label}"),
                    label: (*label).into(),
                    annotation: String::new(),
                    directory: label.ends_with('/'),
                },
                highlights: Vec::new(),
            })
            .collect();
        let history: Vec<String> = history.iter().map(|entry| (*entry).to_owned()).collect();
        rank_candidates(&mut items, &history, prefix, current);
        items.into_iter().map(|item| item.candidate.label).collect()
    }

    /// Order reproduced with Vertico's `vertico-sort-history-length-alpha`.
    #[test]
    fn ranks_like_vertico_history_length_alpha() {
        let history = [
            "/home/u/src/proj/README.md",
            "/home/u/src/proj/src/main.rs",
            "/home/u/src/proj/src/lib.rs",
            "/home/u/src/proj/Cargo.toml",
            "/home/u/src/proj/src/main.rs",
            "/home/u/src/other/x.rs",
        ];
        let labels = [
            "README.md",
            "src/",
            "Cargo.toml",
            "zzz.txt",
            "aa.txt",
            "a-very-long-file-name-that-exceeds-forty-eight-characters.rs",
            "b",
            "LICENSE",
            "build.rs",
            "src2/",
        ];
        assert_eq!(
            ranked(&labels, &history, "/home/u/src/proj/", None),
            [
                "README.md",
                "src/",
                "Cargo.toml",
                "b",
                "src2/",
                "aa.txt",
                "LICENSE",
                "zzz.txt",
                "build.rs",
                "a-very-long-file-name-that-exceeds-forty-eight-characters.rs",
            ]
        );
    }

    #[test]
    fn history_only_matches_the_typed_directory_and_skips_the_current_file() {
        let history = [
            "~/src/proj/README.md",
            "~/src/proj/deep/nested.rs",
            "~/elsewhere/README.md",
            "/abs/README.md",
        ];
        let labels = ["README.md", "deep/", "src/"];
        assert_eq!(
            ranked(&labels, &history, "~/src/proj/", None),
            ["README.md", "deep/", "src/"]
        );
        assert_eq!(
            ranked(&labels, &history, "", None),
            ["src/", "deep/", "README.md"],
            "without a typed directory the history segment is never a label"
        );
        assert_eq!(
            ranked(
                &labels,
                &history,
                "~/src/proj/",
                Some("~/src/proj/README.md")
            ),
            ["deep/", "src/", "README.md"],
            "the current file and unlisted names fall back to length then name"
        );
    }

    #[test]
    fn complete_path_preserves_the_input_spelling_and_marks_directories() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "semacs-complete-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("alpha.txt"), b"").unwrap();
        fs::write(dir.join("alpine.txt"), b"").unwrap();
        fs::write(dir.join("beta.txt"), b"").unwrap();

        let cancelled = std::sync::atomic::AtomicBool::new(false);
        let listing = path_candidates("al", &dir, &cancelled);
        let candidates = crate::completion::filter(listing.candidates, &listing.query, &cancelled);
        let mut names: Vec<_> = candidates
            .iter()
            .map(|item| item.candidate.value.as_str())
            .collect();
        names.sort_unstable();
        assert_eq!(names, vec!["alpha.txt", "alpine.txt"]);

        for prefix in ["./".to_owned(), format!("{}/", dir.display())] {
            let input = format!("{prefix}aa");
            let listing = path_candidates(&input, &dir, &cancelled);
            let matches = crate::completion::filter(listing.candidates, &listing.query, &cancelled);
            assert_eq!(matches.len(), 1);
            assert_eq!(matches[0].candidate.value, format!("{prefix}alpha.txt"));
        }
        let listing = path_candidates("su", &dir, &cancelled);
        let sub = crate::completion::filter(listing.candidates, &listing.query, &cancelled);
        assert_eq!(sub[0].candidate.value, "sub/");
        assert!(sub[0].candidate.directory);
        let listing = path_candidates("sub/", &dir, &cancelled);
        assert!(listing.directory_input);
        assert!(listing.raw_directory);
        fs::remove_dir_all(dir).unwrap();
    }
}
