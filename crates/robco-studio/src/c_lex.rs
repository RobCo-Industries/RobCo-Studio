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
    // The end of a line, past any backslash-newline splice.
    let to_eol = |mut i: usize| {
        while i < b.len() && b[i] != b'\n' {
            i += if b[i] == b'\\' && b.get(i + 1) == Some(&b'\n') {
                2
            } else {
                1
            };
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
        // A splice joins two lines into one: it keeps the line-start state.
        if c == b'\\' && b.get(i + 1) == Some(&b'\n') {
            i += 2;
            push(start..i, Class::Plain);
            continue;
        }
        let first = std::mem::replace(&mut line_start, false);
        let class = if first && c == b'#' {
            i = to_eol(i);
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
            // A comment counts as white space: a `#` after it still starts a
            // directive if only white space and comments came before on its
            // line. A line end inside it, not spliced, starts a new line.
            let body = &b[start..i];
            line_start = first
                || body
                    .iter()
                    .enumerate()
                    .any(|(k, &x)| x == b'\n' && (k == 0 || body[k - 1] != b'\\'));
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

    #[test]
    fn directive_after_comments() {
        let k = classes("/* note\n*/ #define X 1\n");
        assert!(k.contains(&("#define X 1", Class::Preproc)), "{k:?}");
        let k = classes("int a; /* x */ #define Y\n");
        assert!(!k.iter().any(|&(_, c)| c == Class::Preproc), "{k:?}");
        let k = classes("/* a */ /* b */ #if 1\n");
        assert!(k.contains(&("#if 1", Class::Preproc)), "{k:?}");
        // A spliced line end inside a comment does not start a line.
        let k = classes("int a; /* x \\\n */ #define Z\n");
        assert!(!k.iter().any(|&(_, c)| c == Class::Preproc), "{k:?}");
    }

    #[test]
    fn splices() {
        let k = classes("// one \\\nstill comment\nint");
        assert!(
            k.contains(&("// one \\\nstill comment", Class::Comment)),
            "{k:?}"
        );
        assert!(k.contains(&("int", Class::Keyword)), "{k:?}");
        let k = classes("#define A \\\n  1\nint");
        assert!(k.contains(&("#define A \\\n  1", Class::Preproc)), "{k:?}");
        let k = classes("\\\n#define B\n");
        assert!(k.contains(&("#define B", Class::Preproc)), "{k:?}");
    }
}
