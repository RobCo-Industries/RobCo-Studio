//! A small C tokenizer for colours. It knows comments, strings, characters,
//! numbers, keywords and preprocessor lines. It does not parse C.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Plain,
    Keyword,
    Comment,
    Str,
    Number,
    Preproc,
}

const KEYWORDS: &[&str] = &[
    "auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else",
    "enum", "extern", "float", "for", "goto", "if", "inline", "int", "long", "register",
    "restrict", "return", "short", "signed", "sizeof", "static", "struct", "switch", "typedef",
    "union", "unsigned", "void", "volatile", "while", "bool", "true", "false", "NULL",
];

/// Split `text` into spans of one kind each. Spans cover the text with no gap.
pub fn spans(text: &str) -> Vec<(std::ops::Range<usize>, Kind)> {
    let b = text.as_bytes();
    let mut out: Vec<(std::ops::Range<usize>, Kind)> = Vec::new();
    let mut push = |r: std::ops::Range<usize>, k: Kind| {
        if let Some(last) = out.last_mut()
            && last.1 == k
            && last.0.end == r.start
        {
            last.0.end = r.end;
            return;
        }
        out.push((r, k));
    };
    let mut i = 0;
    let mut line_start = true;
    while i < b.len() {
        let c = b[i];
        let start = i;
        if c == b'\n' {
            i += 1;
            line_start = true;
            push(start..i, Kind::Plain);
            continue;
        }
        if c == b' ' || c == b'\t' || c == b'\r' {
            i += 1;
            push(start..i, Kind::Plain);
            continue;
        }
        let at_line_start = line_start;
        line_start = false;
        if at_line_start && c == b'#' {
            // To the end of the line, following backslash continuations.
            while i < b.len() && b[i] != b'\n' {
                if b[i] == b'\\' && i + 1 < b.len() && b[i + 1] == b'\n' {
                    i += 1;
                }
                i += 1;
            }
            push(start..i, Kind::Preproc);
        } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            push(start..i, Kind::Comment);
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                i += 1;
            }
            i = (i + 2).min(b.len());
            push(start..i, Kind::Comment);
        } else if c == b'"' || c == b'\'' {
            i += 1;
            while i < b.len() && b[i] != c && b[i] != b'\n' {
                if b[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(b.len());
            push(start..i, Kind::Str);
        } else if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_') {
                let e = b[i] == b'e' || b[i] == b'E';
                i += 1;
                if e && i < b.len() && (b[i] == b'+' || b[i] == b'-') {
                    i += 1;
                }
            }
            push(start..i, Kind::Number);
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let k = if KEYWORDS.contains(&&text[start..i]) {
                Kind::Keyword
            } else {
                Kind::Plain
            };
            push(start..i, k);
        } else {
            // One character, which may be several bytes of UTF-8.
            i += text[i..].chars().next().map_or(1, char::len_utf8);
            push(start..i, Kind::Plain);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(s: &str) -> Vec<(&str, Kind)> {
        spans(s).into_iter().map(|(r, k)| (&s[r], k)).collect()
    }

    #[test]
    fn each_kind() {
        let k =
            kinds("#include <x.h>\nint a = 0x1F; // hi\nchar *s = \"a\\\"b\"; /* c\nd */ 1.5e-3");
        assert!(k.contains(&("#include <x.h>", Kind::Preproc)));
        assert!(k.contains(&("int", Kind::Keyword)));
        assert!(k.contains(&("0x1F", Kind::Number)));
        assert!(k.contains(&("// hi", Kind::Comment)));
        assert!(k.contains(&("\"a\\\"b\"", Kind::Str)));
        assert!(k.contains(&("/* c\nd */", Kind::Comment)));
        assert!(k.contains(&("1.5e-3", Kind::Number)));
    }

    #[test]
    fn spans_cover_text() {
        let s = "x\té /* ü */ 'c' #no\n  #define A \\\n  1\n";
        let mut end = 0;
        for (r, _) in spans(s) {
            assert_eq!(r.start, end);
            end = r.end;
        }
        assert_eq!(end, s.len());
    }
}
