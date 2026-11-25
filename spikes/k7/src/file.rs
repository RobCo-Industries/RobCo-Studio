//! Bytes on disk to editor text and back.
//!
//! The editor holds LF only. A file with CRLF on every line is read as LF and
//! written back as CRLF. A file that mixes the two, or is not UTF-8, is refused.
//! Tabs and every other byte stay as they are.

use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum LoadError {
    NotUtf8 { valid_up_to: usize },
    MixedLineEndings { crlf: usize, lf: usize },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::NotUtf8 { valid_up_to } => write!(
                f,
                "Not opened: the file is not valid UTF-8 (first bad byte at offset {valid_up_to})."
            ),
            LoadError::MixedLineEndings { crlf, lf } => write!(
                f,
                "Not opened: the file mixes line endings ({crlf} CRLF, {lf} LF)."
            ),
        }
    }
}

/// Text in the editor, and whether to write CRLF.
pub fn decode(bytes: &[u8]) -> Result<(String, bool), LoadError> {
    let s = std::str::from_utf8(bytes).map_err(|e| LoadError::NotUtf8 {
        valid_up_to: e.valid_up_to(),
    })?;
    let crlf = s.matches("\r\n").count();
    let lf = s.matches('\n').count() - crlf;
    match (crlf, lf) {
        (0, _) => Ok((s.to_owned(), false)),
        (_, 0) => Ok((s.replace("\r\n", "\n"), true)),
        _ => Err(LoadError::MixedLineEndings { crlf, lf }),
    }
}

pub fn encode(text: &str, crlf: bool) -> Vec<u8> {
    if crlf {
        text.replace('\n', "\r\n").into_bytes()
    } else {
        text.as_bytes().to_vec()
    }
}
