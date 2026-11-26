// SPDX-License-Identifier: GPL-3.0-or-later

//! The two output streams of a command, by "Output streams" and "JSON lines"
//! of RobCo-Core `docs/spec/STUDIO_CLI.md`.
//!
//! With `--json` every line goes to standard output as one JSON object, its
//! keys in the order of the spec's tables, and nothing goes to standard
//! error. Without it, text for a person goes to standard output and the one
//! error line `<code>: <detail>` to standard error.
//!
//! The objects are written by hand: `serde_json` sorts the keys of a map,
//! and the spec fixes their order. `serde_json` only quotes the strings.

use std::io::Write;

/// One value of a JSON line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Val {
    /// A string.
    Str(String),
    /// An integer.
    Int(u64),
    /// A boolean.
    Bool(bool),
    /// An object, its keys in this order.
    Obj(Vec<(&'static str, Val)>),
    /// An array.
    Arr(Vec<Val>),
}

impl Val {
    /// A string value.
    pub fn s(s: impl Into<String>) -> Self {
        Self::Str(s.into())
    }

    fn write(&self, out: &mut String) {
        match self {
            Self::Str(s) => out.push_str(&quote(s)),
            Self::Int(n) => out.push_str(&n.to_string()),
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Obj(keys) => {
                out.push('{');
                for (i, (k, v)) in keys.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&quote(k));
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
            Self::Arr(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write(out);
                }
                out.push(']');
            }
        }
    }
}

/// A JSON string. `serde_json` escapes every control character, so a line
/// never holds a line break.
fn quote(s: &str) -> String {
    serde_json::Value::String(s.to_owned()).to_string()
}

/// One JSON line: `v`, `kind`, then `keys` in order.
pub fn json_line(kind: &str, keys: Vec<(&'static str, Val)>) -> String {
    let mut all = vec![("v", Val::Int(1)), ("kind", Val::s(kind))];
    all.extend(keys);
    let mut s = String::new();
    Val::Obj(all).write(&mut s);
    s.push('\n');
    s
}

/// The writer of one run. A write that the host refuses is dropped: the
/// exit code still tells the caller how the run ended.
pub struct Out<'a> {
    json: bool,
    stdout: &'a mut dyn Write,
    stderr: &'a mut dyn Write,
}

impl<'a> Out<'a> {
    /// A writer in JSON mode when `json` is set.
    pub fn new(json: bool, stdout: &'a mut dyn Write, stderr: &'a mut dyn Write) -> Self {
        Self {
            json,
            stdout,
            stderr,
        }
    }

    fn put(&mut self, s: &str) {
        let _ = self.stdout.write_all(s.as_bytes());
        let _ = self.stdout.flush();
    }

    /// A `step` line. Nothing without `--json`.
    pub fn step(&mut self, step: &str, item: &str, state: &str) {
        if self.json {
            let line = json_line(
                "step",
                vec![
                    ("step", Val::s(step)),
                    ("item", Val::s(item)),
                    ("state", Val::s(state)),
                ],
            );
            self.put(&line);
        }
    }

    /// A line of another kind with `--json`, or `text` and a line feed
    /// without it.
    pub fn line(&mut self, kind: &str, keys: Vec<(&'static str, Val)>, text: &str) {
        let line = if self.json {
            json_line(kind, keys)
        } else {
            format!("{text}\n")
        };
        self.put(&line);
    }

    /// A line for a person, without `--json` only.
    pub fn text(&mut self, text: &str) {
        if !self.json {
            self.put(&format!("{text}\n"));
        }
    }

    /// The end of a run that passed: the `result` line with `extra` after
    /// `exit`. Nothing without `--json`.
    pub fn ok(&mut self, command: &str, extra: Vec<(&'static str, Val)>) {
        if self.json {
            let mut keys = vec![
                ("command", Val::s(command)),
                ("ok", Val::Bool(true)),
                ("exit", Val::Int(0)),
            ];
            keys.extend(extra);
            let line = json_line("result", keys);
            self.put(&line);
        }
    }

    /// The end of a run that failed with `code`: the `error` line and the
    /// `result` line with `--json`, or one line on standard error without
    /// it. Returns the exit code.
    pub fn fail(&mut self, command: &str, code: &str, detail: &str, exit: u8) -> u8 {
        // The detail is one line, whatever an error message held.
        let detail = detail.replace(['\r', '\n'], " ");
        if self.json {
            let error = json_line(
                "error",
                vec![
                    ("code", Val::s(code)),
                    ("detail", Val::s(detail)),
                    ("exit", Val::Int(u64::from(exit))),
                ],
            );
            let result = json_line(
                "result",
                vec![
                    ("command", Val::s(command)),
                    ("ok", Val::Bool(false)),
                    ("exit", Val::Int(u64::from(exit))),
                ],
            );
            self.put(&error);
            self.put(&result);
        } else {
            let _ = writeln!(self.stderr, "{code}: {detail}");
            let _ = self.stderr.flush();
        }
        exit
    }
}

/// The detail of an error whose text is `<code>: <detail>`.
pub fn detail_of(code: &str, text: &str) -> String {
    text.strip_prefix(code)
        .and_then(|t| t.strip_prefix(": "))
        .unwrap_or(text)
        .to_owned()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a test; a failed unwrap is a failed test"
)]
mod tests {
    use super::*;

    fn run(json: bool, f: impl FnOnce(&mut Out<'_>)) -> (String, String) {
        let (mut o, mut e) = (Vec::new(), Vec::new());
        f(&mut Out::new(json, &mut o, &mut e));
        (String::from_utf8(o).unwrap(), String::from_utf8(e).unwrap())
    }

    #[test]
    fn keys_come_in_the_order_of_the_spec() {
        let (o, e) = run(true, |out| out.step("download", "pipos@1.4.0", "start"));
        assert_eq!(
            o,
            "{\"v\":1,\"kind\":\"step\",\"step\":\"download\",\"item\":\"pipos@1.4.0\",\"state\":\"start\"}\n"
        );
        assert!(e.is_empty());
    }

    #[test]
    fn a_failure_writes_an_error_and_a_result_line() {
        let (o, e) = run(true, |out| {
            assert_eq!(out.fail("sdk list", "usage", "a \"b\"\nc", 2), 2);
        });
        assert_eq!(
            o,
            "{\"v\":1,\"kind\":\"error\",\"code\":\"usage\",\"detail\":\"a \\\"b\\\" c\",\"exit\":2}\n\
             {\"v\":1,\"kind\":\"result\",\"command\":\"sdk list\",\"ok\":false,\"exit\":2}\n"
        );
        assert!(e.is_empty());
    }

    #[test]
    fn without_json_a_failure_is_one_line_on_standard_error() {
        let (o, e) = run(false, |out| {
            out.step("resolve", "", "start");
            out.ok("sdk list", vec![]);
            out.fail("sdk install", "index: read", "x\ny", 4);
        });
        assert!(o.is_empty());
        assert_eq!(e, "index: read: x y\n");
    }

    #[test]
    fn arrays_and_objects_nest() {
        let (o, _) = run(true, |out| {
            out.ok(
                "sdk install",
                vec![(
                    "packages",
                    Val::Arr(vec![Val::Obj(vec![("name", Val::s("pipos"))])]),
                )],
            );
        });
        assert_eq!(
            o,
            "{\"v\":1,\"kind\":\"result\",\"command\":\"sdk install\",\"ok\":true,\"exit\":0,\"packages\":[{\"name\":\"pipos\"}]}\n"
        );
    }

    #[test]
    fn detail_strips_the_code() {
        assert_eq!(detail_of("index: read", "index: read: gone"), "gone");
        assert_eq!(detail_of("index: read", "other"), "other");
    }
}
