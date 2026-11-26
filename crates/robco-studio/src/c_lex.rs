// SPDX-License-Identifier: GPL-3.0-or-later

//! A token pass over C text for its colours. It finds comments, strings,
//! characters, numbers, keywords and preprocessor lines. It does not parse C.

use std::ops::Range;

/// The colour class of a span.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    /// Everything else.
    Plain,
    /// A C keyword.
    Keyword,
    /// A comment.
    Comment,
    /// A string or a character constant.
    Str,
    /// A number.
    Number,
    /// A preprocessor line.
    Preproc,
}

const KEYWORDS: &[&str] = &[
    "auto", "bool", "break", "case", "char", "const", "continue", "default", "do", "double",
    "else", "enum", "extern", "false", "float", "for", "goto", "if", "inline", "int", "long",
    "NULL", "register", "restrict", "return", "short", "signed", "sizeof", "static", "struct",
    "switch", "true", "typedef", "union", "unsigned", "void", "volatile", "while",
];

/// Split `text` into spans of one class each. The spans cover the text, in
/// order, with no gap.
pub fn spans(text: &str) -> Vec<(Range<usize>, Class)> {
    let b = text.as_bytes();
    let mut out: Vec<(Range<usize>, Class)> = Vec::new();
    let mut push = |r: Range<usize>, k: Class| match out.last_mut() {
        Some(last) if last.1 == k && last.0.end == r.start => last.0.end = r.end,
        _ => out.push((r, k)),
    };
    let to_eol = |mut i: usize| {
        while i < b.len() && b[i] != b'\n' {
            i += 1;
        }
        i
    };
    let mut i = 0;
    let mut line_start = true;
    while i < b.len() {
        let c = b[i];
        let start = i;
        if c.is_ascii_whitespace() {
            line_start |= c == b'\n';
            i += 1;
            push(start..i, Class::Plain);
            continue;
        }
        let first = std::mem::replace(&mut line_start, false);
        let class = if first && c == b'#' {
            while i < b.len() && b[i] != b'\n' {
                // A backslash at the end of a line carries on to the next.
                i += if b[i] == b'\\' && b.get(i + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
            }
            Class::Preproc
        } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
            i = to_eol(i);
            Class::Comment
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                i += 1;
            }
            i = (i + 2).min(b.len());
            Class::Comment
        } else if c == b'"' || c == b'\'' {
            i += 1;
            while i < b.len() && b[i] != c && b[i] != b'\n' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(b.len());
            Class::Str
        } else if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_') {
                let exp = matches!(b[i], b'e' | b'E');
                i += 1;
                if exp && matches!(b.get(i), Some(b'+' | b'-')) {
                    i += 1;
                }
            }
            Class::Number
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            if KEYWORDS.contains(&&text[start..i]) {
                Class::Keyword
            } else {
                Class::Plain
            }
        } else {
            i += text[i..].chars().next().map_or(1, char::len_utf8);
            Class::Plain
        };
        // A span never ends inside a character.
        while !text.is_char_boundary(i.min(b.len())) {
            i += 1;
        }
        i = i.min(b.len());
        push(start..i, class);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes(s: &str) -> Vec<(&str, Class)> {
        spans(s).into_iter().map(|(r, k)| (&s[r], k)).collect()
    }

    #[test]
    fn each_class() {
        let k =
            classes("#include <x.h>\nint a = 0x1F; // hi\nchar *s = \"a\\\"b\"; /* c\nd */ 1.5e-3");
        assert!(k.contains(&("#include <x.h>", Class::Preproc)));
        assert!(k.contains(&("int", Class::Keyword)));
        assert!(k.contains(&("0x1F", Class::Number)));
        assert!(k.contains(&("// hi", Class::Comment)));
        assert!(k.contains(&("\"a\\\"b\"", Class::Str)));
        assert!(k.contains(&("/* c\nd */", Class::Comment)));
        assert!(k.contains(&("1.5e-3", Class::Number)));
    }

    #[test]
    fn spans_cover_the_text() {
        for s in [
            "x\té /* ü */ 'c' #no\n  #define A \\\n  1\n",
            "\"\\é\"",
            "'\\",
        ] {
            let mut end = 0;
            for (r, _) in spans(s) {
                assert_eq!(r.start, end);
                end = r.end;
            }
            assert_eq!(end, s.len());
        }
    }
}
