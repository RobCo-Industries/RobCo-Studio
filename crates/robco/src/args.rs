// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads the command line, by "Options" of RobCo-Core
//! `docs/spec/STUDIO_CLI.md`. Two commands with two options need no parser
//! crate. Every wrong form gives `usage`.

use studio_core::resolve::{Abi, InstallPick, Version};

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `robco --version`, `robco -V` or `robco version`.
    Version,
    /// `robco sdk list [--index LOCATION]`.
    SdkList {
        /// The location of the index, when given.
        index: Option<String>,
    },
    /// `robco build [--index LOCATION]`.
    Build {
        /// The location of the index, when given.
        index: Option<String>,
    },
    /// `robco sdk install PACKAGE[@VERSION] [--index LOCATION]`.
    SdkInstall {
        /// The package argument as given.
        argument: String,
        /// The name before `@`.
        name: String,
        /// Which versions it allows.
        pick: InstallPick,
        /// The location of the index, when given.
        index: Option<String>,
    },
}

/// A wrong command line: the `usage` error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage {
    /// The command the line named, or the empty string.
    pub command: &'static str,
    /// What is wrong, for a person.
    pub detail: String,
}

/// Whether the tool writes JSON lines: one argument is exactly `--json`,
/// wherever it stands.
pub fn wants_json(args: &[String]) -> bool {
    args.iter().any(|a| a == "--json")
}

/// Reads the arguments after the program name.
pub fn parse(args: &[String]) -> Result<Command, Usage> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let (command, rest) = match words.as_slice() {
        ["--version"] | ["-V"] | ["version"] => return Ok(Command::Version),
        ["--version" | "-V" | "version", "--json"] | ["--json", "--version" | "-V" | "version"] => {
            return Ok(Command::Version);
        }
        ["sdk", "list", rest @ ..] => ("sdk list", rest),
        ["sdk", "install", rest @ ..] => ("sdk install", rest),
        ["build", rest @ ..] => ("build", rest),
        [] => {
            return Err(usage(
                "",
                "no command: robco build, sdk list or sdk install",
            ));
        }
        _ => {
            return Err(usage(
                "",
                "unknown command: robco build, sdk list or sdk install",
            ));
        }
    };

    let mut index = None;
    let mut json = false;
    let mut positional = Vec::new();
    let mut it = rest.iter();
    while let Some(&a) = it.next() {
        if a == "--json" {
            if json {
                return Err(usage(command, "--json is given twice"));
            }
            json = true;
        } else if a == "--index" || a.starts_with("--index=") {
            if index.is_some() {
                return Err(usage(command, "--index is given twice"));
            }
            let value = match a.strip_prefix("--index=") {
                Some(v) => v,
                // A next argument that is an option, `--json` included, is
                // no value: `--index --json` misses its value.
                None => it
                    .next()
                    .copied()
                    .filter(|v| !v.starts_with('-'))
                    .unwrap_or(""),
            };
            if value.is_empty() {
                return Err(usage(command, "--index needs a location"));
            }
            index = Some(value.to_owned());
        } else if a.len() > 1 && a.starts_with('-') {
            return Err(usage(command, &format!("unknown option {a}")));
        } else {
            positional.push(a);
        }
    }

    match command {
        "sdk list" => match positional.as_slice() {
            [] => Ok(Command::SdkList { index }),
            [extra, ..] => Err(usage(command, &format!("extra argument {extra}"))),
        },
        "build" => match positional.as_slice() {
            [] => Ok(Command::Build { index }),
            [extra, ..] => Err(usage(command, &format!("extra argument {extra}"))),
        },
        _ => match positional.as_slice() {
            [] => Err(usage(command, "missing argument PACKAGE[@VERSION]")),
            [argument] => {
                let (name, pick) = package_argument(argument).ok_or_else(|| {
                    usage(command, &format!("{argument}: after @ comes X.Y or X.Y.Z"))
                })?;
                Ok(Command::SdkInstall {
                    argument: (*argument).to_owned(),
                    name,
                    pick,
                    index,
                })
            }
            [_, extra, ..] => Err(usage(command, &format!("extra argument {extra}"))),
        },
    }
}

fn usage(command: &'static str, detail: &str) -> Usage {
    Usage {
        command,
        detail: detail.to_owned(),
    }
}

/// Splits `PACKAGE[@VERSION]`. `None` for any form after `@` other than
/// `X.Y` and `X.Y.Z`. The name is not checked here: a name the index does
/// not list fails later with `sdk: no package`.
fn package_argument(a: &str) -> Option<(String, InstallPick)> {
    match a.split_once('@') {
        None => Some((a.to_owned(), InstallPick::Newest)),
        Some((name, v)) => {
            let pick = if let Some(abi) = Abi::parse(v) {
                InstallPick::Minor(abi.major, abi.minor)
            } else {
                InstallPick::Exact(Version::parse(v)?)
            };
            Some((name.to_owned(), pick))
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a test; a failed unwrap is a failed test"
)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Command, Usage> {
        parse(&args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
    }

    fn list(index: Option<&str>) -> Command {
        Command::SdkList {
            index: index.map(str::to_owned),
        }
    }

    #[test]
    fn options_come_in_any_order_and_in_both_forms() {
        assert_eq!(p(&["sdk", "list"]).unwrap(), list(None));
        assert_eq!(
            p(&["build", "--json", "--index=i.json"]).unwrap(),
            Command::Build {
                index: Some("i.json".to_owned())
            }
        );
        assert_eq!(p(&["sdk", "list", "--json"]).unwrap(), list(None));
        assert_eq!(
            p(&["sdk", "list", "--index", "i.json"]).unwrap(),
            list(Some("i.json"))
        );
        assert_eq!(
            p(&["sdk", "list", "--index=i.json", "--json"]).unwrap(),
            list(Some("i.json"))
        );
        let want = Command::SdkInstall {
            argument: "pipos@1.4".to_owned(),
            name: "pipos".to_owned(),
            pick: InstallPick::Minor(1, 4),
            index: Some("i.json".to_owned()),
        };
        assert_eq!(
            p(&["sdk", "install", "pipos@1.4", "--index", "i.json"]).unwrap(),
            want
        );
        assert_eq!(
            p(&["sdk", "install", "--index", "i.json", "pipos@1.4", "--json"]).unwrap(),
            want
        );
    }

    #[test]
    fn the_three_install_forms() {
        let pick = |a: &str| match p(&["sdk", "install", a]).unwrap() {
            Command::SdkInstall { pick, name, .. } => (name, pick),
            other => unreachable!("{other:?}"),
        };
        assert_eq!(pick("pipos"), ("pipos".to_owned(), InstallPick::Newest));
        assert_eq!(
            pick("pipos@1.4"),
            ("pipos".to_owned(), InstallPick::Minor(1, 4))
        );
        assert_eq!(
            pick("pipos@1.4.0"),
            (
                "pipos".to_owned(),
                InstallPick::Exact(Version::parse("1.4.0").unwrap())
            )
        );
    }

    #[test]
    fn every_wrong_form_is_usage() {
        let cases: &[(&[&str], &str)] = &[
            (&[], ""),
            (&["bogus"], ""),
            (&["build", "extra"], "build"),
            (&["build", "--index"], "build"),
            (&["build", "--bogus"], "build"),
            (&["build", "--json", "--json"], "build"),
            (&["sdk"], ""),
            (&["sdk", "remove", "pipos"], ""),
            (&["sdk", "doctor"], ""),
            (&["--version", "extra"], ""),
            (&["--json"], ""),
            (&["sdk", "list", "extra"], "sdk list"),
            (&["sdk", "list", "--bogus"], "sdk list"),
            (&["sdk", "list", "-x"], "sdk list"),
            (&["sdk", "list", "--index"], "sdk list"),
            (&["sdk", "list", "--index="], "sdk list"),
            (&["sdk", "list", "--index", "--json"], "sdk list"),
            (&["sdk", "list", "--index", "a", "--index", "b"], "sdk list"),
            (&["sdk", "list", "--json", "--json"], "sdk list"),
            (&["sdk", "install"], "sdk install"),
            (&["sdk", "install", "a", "b"], "sdk install"),
            (&["sdk", "install", "pipos@"], "sdk install"),
            (&["sdk", "install", "pipos@1"], "sdk install"),
            (&["sdk", "install", "pipos@1.04"], "sdk install"),
            (&["sdk", "install", "pipos@1.4.0.1"], "sdk install"),
            (&["sdk", "install", "pipos@latest"], "sdk install"),
        ];
        for (args, command) in cases {
            let e = p(args).unwrap_err();
            assert_eq!(e.command, *command, "{args:?}");
            assert!(!e.detail.is_empty());
        }
    }

    #[test]
    fn json_is_found_anywhere() {
        let a = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(wants_json(&a(&["bogus", "--json"])));
        assert!(!wants_json(&a(&["sdk", "list", "--json=1"])));
    }
}
