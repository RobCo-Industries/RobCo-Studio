// SPDX-License-Identifier: GPL-3.0-or-later

//! No colour, size or font is written outside the tokens module. A view asks
//! the theme.

use std::path::Path;

/// The modules that may hold colour and size literals: the tokens and the
/// palette rule.
const TOKEN_MODULES: [&str; 2] = ["theme.rs", "palette.rs"];
/// Float literals a view may write: none, one, and two, for halves.
const ALLOWED_FLOATS: [&str; 3] = ["0.0", "1.0", "2.0"];
/// Colours that are not part of the look: nothing, and egui's own marker.
const ALLOWED_COLOURS: [&str; 2] = ["Color32::TRANSPARENT", "Color32::PLACEHOLDER"];

/// Every literal colour, float or font in `src`, outside test modules.
fn offences(name: &str, src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let body = src.split("#[cfg(test)]").next().unwrap_or(src);
    for (n, line) in body.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("");
        let at = |what: &str| format!("{name}:{}: {what}: {}", n + 1, line.trim());
        // Colours. The art module makes white masks that the theme tints.
        let mut rest = code;
        while let Some(i) = rest.find("Color32::") {
            let tail = &rest[i..];
            let word: String = tail
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == ':' || *c == '_')
                .collect();
            let mask = name == "art.rs" && word == "Color32::WHITE";
            if !ALLOWED_COLOURS.contains(&word.as_str()) && !mask {
                out.push(at("colour"));
            }
            rest = &tail[9..];
        }
        if code.contains("FontId::new") || code.contains("FontFamily::") {
            out.push(at("font"));
        }
        if code.contains("\"#")
            && code.chars().filter(char::is_ascii_hexdigit).count() >= 6
            && code.contains("#0")
        {
            out.push(at("hex colour"));
        }
        // Floats: digits, a dot, digits, not part of a name or a range.
        let b = code.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i].is_ascii_digit()
                && (i == 0
                    || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_' || b[i - 1] == b'.'))
            {
                let s = i;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
                    i += 1;
                    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'_') {
                        i += 1;
                    }
                    let lit = &code[s..i];
                    if !ALLOWED_FLOATS.contains(&lit) {
                        out.push(at(&format!("size {lit}")));
                    }
                }
            } else {
                i += 1;
            }
        }
    }
    out
}

#[test]
fn views_hold_no_literal_colour_or_size() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut all = Vec::new();
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        if TOKEN_MODULES.contains(&name.as_str()) || p.extension().is_none_or(|x| x != "rs") {
            continue;
        }
        all.extend(offences(&name, &std::fs::read_to_string(&p).unwrap()));
    }
    assert!(
        all.is_empty(),
        "literals outside the tokens module:\n{}",
        all.join("\n")
    );
}

#[test]
fn the_search_finds_what_it_must() {
    assert_eq!(
        offences("v.rs", "let c = Color32::from_rgb(1, 2, 3);").len(),
        1
    );
    assert_eq!(offences("v.rs", "let w = 12.5;").len(), 1);
    assert_eq!(offences("v.rs", "let f = FontId::new(13.0, x);").len(), 2);
    assert!(offences("v.rs", "let w = x * 2.0 + 1.0; // 3.5 in a comment").is_empty());
    assert!(offences("v.rs", "let t = Color32::TRANSPARENT; v0.37").is_empty());
}
