//! File text encoding under D20.
//!
//! Text files are supported as UTF-8 with or without BOM and with uniform LF or
//! CRLF line endings. The in-memory text is always BOM-free UTF-8 with LF
//! newlines; the original format is metadata used when writing the file back.
//!
//! Mixed line endings are not supported yet. They are rejected rather than
//! normalized so opening a file can never silently rewrite it.

use std::fmt;
use std::io;
use std::path::Path;

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

pub fn read(path: &Path) -> Result<DecodedFile, ReadError> {
    let bytes = std::fs::read(path)?;
    Ok(decode(&bytes)?)
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
}
