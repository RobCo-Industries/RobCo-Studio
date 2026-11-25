// SPDX-License-Identifier: GPL-3.0-or-later

//! The QEMU command line of a board, read from `boot.json`.
//!
//! `boot.json` holds the boot arguments of each board as data. The format is
//! the "boot.json" section of `docs/spec/TESTING.md` in RobCo-Core, with
//! "Order of checks" of `docs/spec/STUDIO_CLI.md`, read at commit
//! `e004c73`. This
//! module reads the file and builds the program and its arguments for a
//! board, a mode, three paths and the TCP ports. It starts no process.
//!
//! The order of the command line is the spec's:
//!
//! 1. The program, `-M`, the machine.
//! 2. `common`.
//! 3. QMP, then qtest when used.
//! 4. The PL011 serial port, then the mini UART when used.
//! 5. `-S`.
//! 6. The arguments of the mode.
//! 7. `sd` when an SD image is attached.
//!
//! QMP, qtest and the serial ports use TCP on `127.0.0.1` on every system.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde_json::Value;

/// The `format` this module reads. A larger one fails.
pub const FORMAT: u64 = 1;

/// The host that QMP, qtest and the serial ports listen on or connect to.
pub const HOST: &str = "127.0.0.1";

/// A parsed `boot.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootFile {
    /// Arguments every boot passes after `-M`.
    pub common: Vec<String>,
    /// Arguments that attach an SD image.
    pub sd: Vec<String>,
    /// The boards, keyed by QEMU machine name.
    pub boards: BTreeMap<String, Board>,
}

/// One board entry of `boot.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    /// The QEMU program, such as `qemu-system-arm`.
    pub qemu: String,
    /// The `-M` value.
    pub machine: String,
    /// The mode used when the caller asks for the default.
    pub default_mode: String,
    /// The arguments of each mode, keyed by mode name.
    pub modes: BTreeMap<String, Vec<String>>,
}

/// The mode a caller asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode<'a> {
    /// The board's `default_mode`.
    Default,
    /// A mode by name, such as `svc` or `hyp`.
    Named(&'a str),
}

/// The TCP ports of one boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ports {
    /// QMP. QEMU listens on it.
    pub qmp: u16,
    /// qtest, when used. QEMU connects to it, so the tool listens first.
    pub qtest: Option<u16>,
    /// The PL011 serial port. QEMU listens on it.
    pub pl011: u16,
    /// The mini UART, when used. QEMU listens on it.
    pub mini_uart: Option<u16>,
}

/// What to boot.
#[derive(Debug, Clone, Copy)]
pub struct BootRequest<'a> {
    /// The QEMU machine name, a key of `boards`.
    pub board: &'a str,
    /// The mode.
    pub mode: Mode<'a>,
    /// The firmware stub, `fwstub.bin`, for `{stub}`.
    pub stub: &'a Path,
    /// The raw image, for `{image}`.
    pub image: &'a Path,
    /// The SD image, for `{sd}`. `sd` is added only when this is set.
    pub card: Option<&'a Path>,
    /// The TCP ports.
    pub ports: Ports,
}

/// A program and its arguments, ready for a process builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QemuCommand {
    /// The QEMU program name, such as `qemu-system-arm`.
    pub program: String,
    /// The arguments, in the spec's order.
    pub args: Vec<String>,
}

/// Why `boot.json` could not be read or a board could not be booted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootError {
    /// The text is not JSON.
    Json(String),
    /// A required field is missing or has the wrong type. Holds its path.
    Field(String),
    /// The file's `format` is larger than this module reads.
    NewerFormat(u64),
    /// The file does not list the board.
    UnknownBoard(String),
    /// The board does not list the mode.
    UnknownMode {
        /// The board.
        board: String,
        /// The mode asked for.
        mode: String,
    },
    /// A path is not valid UTF-8, so it cannot go into an argument.
    PathNotUtf8(String),
    /// The file's `format` is below 1.
    OlderFormat(i64),
    /// The file's `format` is missing or not an integer.
    BadFormat,
    /// A board's `default_mode` names a mode the board does not list.
    /// Holds the field's path and the mode.
    UnknownDefaultMode {
        /// The path of the field, such as `boards.raspi0.default_mode`.
        at: String,
        /// The mode it names.
        mode: String,
    },
    /// A string holds a `{` that does not start `{stub}`, `{image}` or `{sd}`.
    UnknownPlaceholder {
        /// The path of the string, such as `boards.raspi0.modes.svc[3]`.
        at: String,
        /// The text from the `{` to the next `}`, or to the end.
        name: String,
    },
    /// A name field, `qemu` or `machine`, holds a `{`. Holds the field's path.
    PlaceholderInName(String),
    /// A board's arguments use `{sd}` and no SD image was given.
    MissingCard {
        /// The board.
        board: String,
        /// The placeholder, `{sd}`.
        placeholder: String,
    },
}

impl fmt::Display for BootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(e) => write!(f, "boot.json is not valid JSON: {e}"),
            Self::Field(p) => write!(f, "boot.json: `{p}` is missing or has the wrong type"),
            Self::NewerFormat(n) => write!(
                f,
                "boot.json has format {n}, and this version of Studio reads format {FORMAT}: update Studio"
            ),
            Self::UnknownBoard(b) => write!(f, "boot.json lists no board `{b}`, so it cannot boot"),
            Self::UnknownMode { board, mode } => write!(
                f,
                "boot.json lists no mode `{mode}` for board `{board}`, so it cannot boot"
            ),
            Self::PathNotUtf8(p) => write!(f, "the path {p} is not valid UTF-8"),
            Self::BadFormat => write!(
                f,
                "boot.json: format is missing or not a positive integer: Studio reads format {FORMAT}"
            ),
            Self::OlderFormat(n) => write!(
                f,
                "boot.json has format {n}, which is not a valid format: Studio reads format {FORMAT}"
            ),
            Self::UnknownDefaultMode { at, mode } => write!(
                f,
                "boot.json: `{at}` names mode `{mode}`, which the board does not list"
            ),
            Self::UnknownPlaceholder { at, name } => write!(
                f,
                "boot.json: `{at}` holds the unknown placeholder `{name}`; the known ones are {{stub}}, {{image}} and {{sd}}"
            ),
            Self::PlaceholderInName(at) => write!(
                f,
                "boot.json: `{at}` holds a `{{`, and placeholders are not allowed there: it is a name, not a path"
            ),
            Self::MissingCard { board, placeholder } => write!(
                f,
                "board `{board}` uses `{placeholder}` and no SD image was given, so it cannot boot"
            ),
        }
    }
}

impl std::error::Error for BootError {}

impl crate::codes::ErrorCode for BootError {
    /// Maps each error to its code, following "Order of checks" in
    /// `STUDIO_CLI.md`: a file that does not parse or a bad field is
    /// `package: manifest`, a bad `format` is `package: format`, and a board
    /// the file does not list is `run: board`. A mode the file does not
    /// list takes the code of a board it does not list. A path that is not
    /// UTF-8 and a missing SD image have no code in the spec, so they take
    /// `internal`.
    fn error_code(&self) -> &'static str {
        match self {
            Self::Json(_)
            | Self::Field(_)
            | Self::UnknownDefaultMode { .. }
            | Self::UnknownPlaceholder { .. }
            | Self::PlaceholderInName(_) => "package: manifest",
            Self::NewerFormat(_) | Self::OlderFormat(_) | Self::BadFormat => "package: format",
            Self::UnknownBoard(_) | Self::UnknownMode { .. } => "run: board",
            Self::PathNotUtf8(_) | Self::MissingCard { .. } => "internal",
        }
    }
}

/// Reads the text of a `boot.json`. Fields it does not know are ignored.
pub fn parse(text: &str) -> Result<BootFile, BootError> {
    let root: Value = serde_json::from_str(text).map_err(|e| BootError::Json(e.to_string()))?;
    if !root.is_object() {
        return Err(BootError::Json("the top value is not an object".into()));
    }
    // Step 3 of "Order of checks": `format` before every other field.
    let format = root
        .get("format")
        .and_then(Value::as_number)
        .ok_or(BootError::BadFormat)?;
    match (format.as_u64(), format.as_i64()) {
        (Some(FORMAT), _) => {}
        (Some(n), _) if n > FORMAT => return Err(BootError::NewerFormat(n)),
        (_, Some(n)) => return Err(BootError::OlderFormat(n)),
        // A fraction or a number too large for an integer.
        _ => return Err(BootError::BadFormat),
    }
    let common = strings(&root, "common", "common")?;
    let sd = strings(&root, "sd", "sd")?;
    let boards_value = root
        .get("boards")
        .and_then(Value::as_object)
        .ok_or_else(|| BootError::Field("boards".into()))?;
    let mut boards = BTreeMap::new();
    for (name, entry) in boards_value {
        let at = format!("boards.{name}");
        let text = |key: &str| {
            entry
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| BootError::Field(format!("{at}.{key}")))
        };
        let qemu = text("qemu")?;
        let machine = text("machine")?;
        let default_mode = text("default_mode")?;
        for (key, value) in [("qemu", &qemu), ("machine", &machine)] {
            if value.contains('{') {
                return Err(BootError::PlaceholderInName(format!("{at}.{key}")));
            }
        }
        let modes_value = entry
            .get("modes")
            .and_then(Value::as_object)
            .ok_or_else(|| BootError::Field(format!("{at}.modes")))?;
        let mut modes = BTreeMap::new();
        for mode in modes_value.keys() {
            let list = strings(&entry["modes"], mode, &format!("{at}.modes.{mode}"))?;
            modes.insert(mode.clone(), list);
        }
        if !modes.contains_key(&default_mode) {
            return Err(BootError::UnknownDefaultMode {
                at: format!("{at}.default_mode"),
                mode: default_mode,
            });
        }
        for (mode, list) in &modes {
            check_placeholders(list, &format!("{at}.modes.{mode}"))?;
        }
        boards.insert(
            name.clone(),
            Board {
                qemu,
                machine,
                default_mode,
                modes,
            },
        );
    }
    check_placeholders(&common, "common")?;
    check_placeholders(&sd, "sd")?;
    Ok(BootFile { common, sd, boards })
}

const PLACEHOLDERS: [&str; 3] = ["{stub}", "{image}", "{sd}"];

/// The placeholder that starts `tail`, which starts with `{`, or the
/// unknown text from the `{` to the next `}`, or to the end.
fn placeholder_at(tail: &str) -> Result<&'static str, String> {
    PLACEHOLDERS
        .into_iter()
        .find(|p| tail.starts_with(p))
        .ok_or_else(|| match tail.find('}') {
            Some(end) => tail[..=end].to_owned(),
            None => tail.to_owned(),
        })
}

fn check_placeholders(list: &[String], at: &str) -> Result<(), BootError> {
    for (i, arg) in list.iter().enumerate() {
        let mut rest = arg.as_str();
        while let Some(start) = rest.find('{') {
            let tail = &rest[start..];
            let p = placeholder_at(tail).map_err(|name| BootError::UnknownPlaceholder {
                at: format!("{at}[{i}]"),
                name,
            })?;
            rest = &tail[p.len()..];
        }
    }
    Ok(())
}

fn strings(parent: &Value, key: &str, at: &str) -> Result<Vec<String>, BootError> {
    let bad = || BootError::Field(at.to_owned());
    parent
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(bad)?
        .iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or_else(bad))
        .collect()
}

/// Doubles every comma, as QEMU reads a doubled comma in an option value
/// as one comma.
pub fn escape_commas(path: &str) -> String {
    path.replace(',', ",,")
}

fn path_text(path: &Path) -> Result<String, BootError> {
    path.to_str()
        .map(escape_commas)
        .ok_or_else(|| BootError::PathNotUtf8(path.display().to_string()))
}

/// Builds the QEMU program and arguments for a request.
pub fn command(file: &BootFile, request: &BootRequest<'_>) -> Result<QemuCommand, BootError> {
    let board = file
        .boards
        .get(request.board)
        .ok_or_else(|| BootError::UnknownBoard(request.board.to_owned()))?;
    let mode = match request.mode {
        Mode::Default => board.default_mode.as_str(),
        Mode::Named(m) => m,
    };
    let mode_args = board
        .modes
        .get(mode)
        .ok_or_else(|| BootError::UnknownMode {
            board: request.board.to_owned(),
            mode: mode.to_owned(),
        })?;

    let stub = path_text(request.stub)?;
    let image = path_text(request.image)?;
    let card = request.card.map(path_text).transpose()?;
    // One pass over the string, so a path that holds a placeholder's text is
    // never replaced again.
    let fill = |at: &str, arg: &String| -> Result<String, BootError> {
        let mut out = String::with_capacity(arg.len());
        let mut rest = arg.as_str();
        while let Some(i) = rest.find('{') {
            out.push_str(&rest[..i]);
            let tail = &rest[i..];
            let p = placeholder_at(tail).map_err(|name| BootError::UnknownPlaceholder {
                at: at.to_owned(),
                name,
            })?;
            let text = match p {
                "{stub}" => stub.as_str(),
                "{image}" => image.as_str(),
                _ => card.as_deref().ok_or_else(|| BootError::MissingCard {
                    board: request.board.to_owned(),
                    placeholder: p.to_owned(),
                })?,
            };
            out.push_str(text);
            rest = &tail[p.len()..];
        }
        out.push_str(rest);
        Ok(out)
    };
    let mode_at = format!("boards.{}.modes.{mode}", request.board);

    // Every source of the command line, in the spec's order. Each string,
    // from the file or built here, passes through `fill` on its way out.
    let ports = request.ports;
    let mut parts: Vec<(String, Vec<String>)> = Vec::new();
    parts.push((
        format!("boards.{}.machine", request.board),
        vec!["-M".to_owned(), board.machine.clone()],
    ));
    parts.push(("common".to_owned(), file.common.clone()));
    let mut ours = vec![
        "-qmp".to_owned(),
        format!("tcp:{HOST}:{},server=on,wait=off", ports.qmp),
    ];
    if let Some(port) = ports.qtest {
        ours.push("-qtest".into());
        ours.push(format!("tcp:{HOST}:{port}"));
    }
    let mut serial = |id: &str, port: u16| {
        ours.push("-chardev".into());
        ours.push(format!(
            "socket,id={id},host={HOST},port={port},server=on,wait=off"
        ));
        ours.push("-serial".into());
        ours.push(format!("chardev:{id}"));
    };
    serial("ser0", ports.pl011);
    if let Some(port) = ports.mini_uart {
        serial("ser1", port);
    }
    ours.push("-S".into());
    parts.push(("ports".to_owned(), ours));
    parts.push((mode_at, mode_args.clone()));
    if card.is_some() {
        parts.push(("sd".to_owned(), file.sd.clone()));
    }

    let mut args = Vec::new();
    for (at, list) in &parts {
        for (i, arg) in list.iter().enumerate() {
            args.push(fill(&format!("{at}[{i}]"), arg)?);
        }
    }
    Ok(QemuCommand {
        program: board.qemu.clone(),
        args,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/boot.json");

    const PORTS: Ports = Ports {
        qmp: 4001,
        qtest: None,
        pl011: 4002,
        mini_uart: None,
    };

    fn file() -> BootFile {
        parse(FIXTURE).unwrap()
    }

    fn req<'a>(board: &'a str, mode: Mode<'a>) -> BootRequest<'a> {
        BootRequest {
            board,
            mode,
            stub: Path::new("S"),
            image: Path::new("I"),
            card: None,
            ports: PORTS,
        }
    }

    fn head(machine: &str) -> Vec<String> {
        [
            "-M",
            machine,
            "-display",
            "none",
            "-monitor",
            "none",
            "-no-user-config",
            "-qmp",
            "tcp:127.0.0.1:4001,server=on,wait=off",
            "-chardev",
            "socket,id=ser0,host=127.0.0.1,port=4002,server=on,wait=off",
            "-serial",
            "chardev:ser0",
            "-S",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    fn loaders(w1: &str, w2: &str, w3: &str) -> Vec<String> {
        let mut v = vec![
            "-device".to_string(),
            "loader,file=S,addr=0x0,force-raw=on".into(),
            "-device".into(),
            "loader,file=I,addr=0x8000,force-raw=on".into(),
        ];
        for (addr, w) in [
            ("0x7f00", "0x57464352"),
            ("0x7f04", w1),
            ("0x7f08", w2),
            ("0x7f0c", w3),
        ] {
            v.push("-device".into());
            v.push(format!("loader,addr={addr},data={w},data-len=4"));
        }
        v
    }

    fn shim(third: &str) -> Vec<String> {
        let words = [
            "0xd2802620",
            "0xd51e1100",
            third,
            "0xd51e4000",
            "0xd2800000",
            "0xd51e4020",
            "0xd69f03e0",
        ];
        let mut v = Vec::new();
        for (i, w) in words.iter().enumerate() {
            v.push("-device".to_string());
            v.push(format!(
                "loader,addr=0x{:x},data={w},data-len=4",
                0x80000 + 4 * i
            ));
        }
        for cpu in 0..4 {
            v.push("-device".into());
            v.push(format!("loader,addr=0x80000,cpu-num={cpu}"));
        }
        v
    }

    fn check(board: &str, mode: Mode<'_>, program: &str, tail: Vec<String>) {
        let cmd = command(&file(), &req(board, mode)).unwrap();
        assert_eq!(cmd.program, program);
        let mut want = head(board);
        want.extend(tail);
        assert_eq!(cmd.args, want, "{board} {mode:?}");
    }

    #[test]
    fn every_board_and_mode_matches_the_spec() {
        let pi1 = loaders("0x00000c42", "0x20000000", "0x00000000");
        for b in ["raspi0", "raspi1ap"] {
            check(b, Mode::Named("svc"), "qemu-system-arm", pi1.clone());
            check(b, Mode::Default, "qemu-system-arm", pi1.clone());
        }
        let svc2 = loaders("0x00000c43", "0x40000000", "0x00000000");
        let hyp2 = loaders("0x00000c43", "0x40000000", "0x00000001");
        check(
            "raspi2b",
            Mode::Named("svc"),
            "qemu-system-arm",
            svc2.clone(),
        );
        check(
            "raspi2b",
            Mode::Named("hyp"),
            "qemu-system-arm",
            hyp2.clone(),
        );
        check("raspi2b", Mode::Default, "qemu-system-arm", hyp2);
        let mut svc3 = svc2.clone();
        svc3.extend(shim("0xd2803a60"));
        let mut hyp3 = svc2;
        hyp3.extend(shim("0xd2803b40"));
        check("raspi3b", Mode::Named("svc"), "qemu-system-aarch64", svc3);
        check(
            "raspi3b",
            Mode::Named("hyp"),
            "qemu-system-aarch64",
            hyp3.clone(),
        );
        check("raspi3b", Mode::Default, "qemu-system-aarch64", hyp3);
        assert_eq!(file().boards.len(), 4);
    }

    #[test]
    fn qtest_mini_uart_and_card_take_their_places() {
        let mut r = req("raspi0", Mode::Default);
        r.ports = Ports {
            qmp: 1,
            qtest: Some(2),
            pl011: 3,
            mini_uart: Some(4),
        };
        r.card = Some(Path::new("card.img"));
        let args = command(&file(), &r).unwrap().args;
        let at = |s: &str| args.iter().position(|a| a == s).unwrap();
        assert!(at("-qmp") < at("-qtest"));
        assert!(at("-qtest") < at("chardev:ser0"));
        assert!(at("chardev:ser0") < at("chardev:ser1"));
        assert!(at("chardev:ser1") < at("-S"));
        assert_eq!(args[at("-qtest") + 1], "tcp:127.0.0.1:2");
        assert_eq!(
            args[at("chardev:ser1") - 2],
            "socket,id=ser1,host=127.0.0.1,port=4,server=on,wait=off"
        );
        assert_eq!(
            &args[args.len() - 2..],
            ["-drive", "if=sd,format=raw,file=card.img"]
        );
    }

    #[test]
    fn sd_is_left_out_without_a_card() {
        let args = command(&file(), &req("raspi0", Mode::Default))
            .unwrap()
            .args;
        assert!(!args.iter().any(|a| a.contains("if=sd")));
    }

    fn image_arg(path: &str) -> String {
        let mut r = req("raspi0", Mode::Default);
        r.image = Path::new(path);
        r.card = Some(Path::new(path));
        let args = command(&file(), &r).unwrap().args;
        assert_eq!(
            args.last().unwrap(),
            &format!("if=sd,format=raw,file={}", escape_commas(path))
        );
        args.iter()
            .find(|a| a.contains("addr=0x8000,"))
            .unwrap()
            .clone()
    }

    #[test]
    fn a_comma_in_a_path_is_doubled() {
        assert_eq!(
            image_arg("a,b/c,,d.img"),
            "loader,file=a,,b/c,,,,d.img,addr=0x8000,force-raw=on"
        );
    }

    #[test]
    fn a_space_is_kept() {
        assert_eq!(
            image_arg("my dir/t.img"),
            "loader,file=my dir/t.img,addr=0x8000,force-raw=on"
        );
    }

    #[test]
    fn non_ascii_is_kept() {
        assert_eq!(
            image_arg("Ünïcødé/テープ.img"),
            "loader,file=Ünïcødé/テープ.img,addr=0x8000,force-raw=on"
        );
    }

    #[test]
    fn a_windows_path_is_kept() {
        assert_eq!(
            image_arg(r"C:\Users\x y\a,b.img"),
            r"loader,file=C:\Users\x y\a,,b.img,addr=0x8000,force-raw=on"
        );
    }

    #[test]
    fn a_path_holding_a_placeholder_is_not_replaced_again() {
        let mut r = req("raspi0", Mode::Default);
        r.image = Path::new("{stub}");
        let args = command(&file(), &r).unwrap().args;
        assert!(args.contains(&"loader,file={stub},addr=0x8000,force-raw=on".to_string()));
    }

    #[test]
    fn an_unknown_board_fails_with_a_message() {
        let e = command(&file(), &req("raspi4b", Mode::Default)).unwrap_err();
        assert_eq!(e, BootError::UnknownBoard("raspi4b".into()));
        assert!(e.to_string().contains("raspi4b"));
    }

    #[test]
    fn an_unknown_mode_fails_with_a_message() {
        let e = command(&file(), &req("raspi0", Mode::Named("hyp"))).unwrap_err();
        assert!(matches!(e, BootError::UnknownMode { .. }));
        assert!(e.to_string().contains("hyp") && e.to_string().contains("raspi0"));
    }

    #[test]
    fn a_larger_format_fails_with_a_message() {
        let text = FIXTURE.replacen("\"format\": 1", "\"format\": 2", 1);
        let e = parse(&text).unwrap_err();
        assert_eq!(e, BootError::NewerFormat(2));
        assert!(e.to_string().contains("format 2"));
    }

    #[test]
    fn an_unknown_field_is_ignored() {
        let text = FIXTURE
            .replacen(
                "\"format\": 1,",
                "\"format\": 1, \"extra\": {\"x\": [1]},",
                1,
            )
            .replacen(
                "\"qemu\": \"qemu-system-arm\",",
                "\"qemu\": \"qemu-system-arm\", \"note\": 5,",
                1,
            );
        assert_eq!(parse(&text).unwrap(), file());
    }

    #[test]
    fn a_missing_field_fails_with_its_path() {
        let cases = [
            ("\"common\":", "\"x_common\":", "common"),
            ("\"sd\":", "\"x_sd\":", "sd"),
            (
                "\"default_mode\": \"svc\",",
                "",
                "boards.raspi0.default_mode",
            ),
        ];
        for (from, to, path) in cases {
            let e = parse(&FIXTURE.replacen(from, to, 1)).unwrap_err();
            assert_eq!(e, BootError::Field(path.into()));
            assert!(e.to_string().contains(path));
        }
    }

    #[test]
    fn only_a_larger_format_asks_for_a_newer_studio() {
        use crate::codes::ErrorCode;
        let text = FIXTURE.replacen("\"format\": 1", "\"format\": 2", 1);
        assert!(
            parse(&text)
                .unwrap_err()
                .to_string()
                .contains("update Studio")
        );
        for f in ["0", "-1", "\"1\"", "1.5", "null", "{}"] {
            let text = FIXTURE.replacen("\"format\": 1", &format!("\"format\": {f}"), 1);
            let e = parse(&text).unwrap_err();
            assert_eq!(e.error_code(), "package: format", "{f}");
            assert!(!e.to_string().contains("update"), "{f}: {e}");
        }
        let e = parse(&FIXTURE.replacen("\"format\": 1,", "", 1)).unwrap_err();
        assert!(!e.to_string().contains("update"), "{e}");
    }

    #[test]
    fn a_format_below_one_fails_with_a_message() {
        for (n, v) in [("0", 0), ("-3", -3)] {
            let text = FIXTURE.replacen("\"format\": 1", &format!("\"format\": {n}"), 1);
            let e = parse(&text).unwrap_err();
            assert_eq!(e, BootError::OlderFormat(v));
            assert!(e.to_string().contains(&format!("format {n}")));
        }
        let text = FIXTURE.replacen("\"format\": 1", "\"format\": 1.5", 1);
        assert_eq!(parse(&text).unwrap_err(), BootError::BadFormat);
    }

    #[test]
    fn a_default_mode_the_board_lacks_fails_at_parse() {
        let text = FIXTURE.replacen("\"default_mode\": \"svc\"", "\"default_mode\": \"hyp\"", 1);
        let e = parse(&text).unwrap_err();
        assert_eq!(
            e,
            BootError::UnknownDefaultMode {
                at: "boards.raspi0.default_mode".into(),
                mode: "hyp".into()
            }
        );
        assert!(e.to_string().contains("boards.raspi0.default_mode"));
    }

    #[test]
    fn an_unknown_placeholder_fails_at_parse() {
        let text = FIXTURE.replacen("file={stub}", "file={kernel}", 1);
        let e = parse(&text).unwrap_err();
        assert_eq!(
            e,
            BootError::UnknownPlaceholder {
                at: "boards.raspi0.modes.svc[1]".into(),
                name: "{kernel}".into()
            }
        );
        assert!(e.to_string().contains("{kernel}"));
        let text = FIXTURE.replacen("file={sd}", "file={sd", 1);
        assert!(matches!(
            parse(&text).unwrap_err(),
            BootError::UnknownPlaceholder { at, .. } if at == "sd[1]"
        ));
        let text = FIXTURE.replacen("\"none\",", "\"{x}\",", 1);
        assert!(matches!(
            parse(&text).unwrap_err(),
            BootError::UnknownPlaceholder { at, .. } if at == "common[1]"
        ));
    }

    #[test]
    fn sd_in_a_mode_without_a_card_fails() {
        let text = FIXTURE.replacen("file={image}", "file={sd}", 1);
        let e = command(&parse(&text).unwrap(), &req("raspi0", Mode::Default)).unwrap_err();
        assert_eq!(
            e,
            BootError::MissingCard {
                board: "raspi0".into(),
                placeholder: "{sd}".into()
            }
        );
        assert!(e.to_string().contains("raspi0") && e.to_string().contains("{sd}"));
    }

    #[test]
    fn placeholders_in_common_are_filled() {
        for (ph, want) in [
            ("{stub}", "x=s,,1"),
            ("{image}", "x=i,,2"),
            ("{sd}", "x=c,,3"),
        ] {
            let text = FIXTURE.replacen("\"none\",", &format!("\"x={ph}\","), 1);
            let mut r = req("raspi0", Mode::Default);
            r.stub = Path::new("s,1");
            r.image = Path::new("i,2");
            r.card = Some(Path::new("c,3"));
            let args = command(&parse(&text).unwrap(), &r).unwrap().args;
            assert_eq!(args[3], want, "{ph}");
            assert!(!args.iter().any(|a| a.contains(ph)), "{ph}");
        }
    }

    #[test]
    fn sd_in_common_without_a_card_fails() {
        let text = FIXTURE.replacen("\"none\",", "\"x={sd}\",", 1);
        let e = command(&parse(&text).unwrap(), &req("raspi0", Mode::Default)).unwrap_err();
        assert_eq!(
            e,
            BootError::MissingCard {
                board: "raspi0".into(),
                placeholder: "{sd}".into()
            }
        );
    }

    #[test]
    fn a_placeholder_in_a_name_fails_at_parse() {
        for (from, to, at) in [
            (
                "\"qemu\": \"qemu-system-arm\"",
                "\"qemu\": \"{stub}\"",
                "boards.raspi0.qemu",
            ),
            (
                "\"machine\": \"raspi0\"",
                "\"machine\": \"{image}\"",
                "boards.raspi0.machine",
            ),
        ] {
            let e = parse(&FIXTURE.replacen(from, to, 1)).unwrap_err();
            assert_eq!(e, BootError::PlaceholderInName(at.into()));
            assert!(e.to_string().contains(at));
        }
    }

    #[test]
    fn a_wrong_type_fails() {
        let text = FIXTURE.replacen("\"format\": 1", "\"format\": \"1\"", 1);
        assert_eq!(parse(&text).unwrap_err(), BootError::BadFormat);
        assert!(matches!(parse("{").unwrap_err(), BootError::Json(_)));
    }

    /// "Order of checks": the file parses, with an object at the top, then
    /// `format`, missing or not, before every other field.
    #[test]
    fn format_comes_first_with_its_code() {
        use crate::codes::ErrorCode;
        assert!(matches!(parse("[1]").unwrap_err(), BootError::Json(_)));
        assert_eq!(parse("[1]").unwrap_err().error_code(), "package: manifest");
        let text = FIXTURE.replacen("\"format\": 1,", "", 1);
        assert_eq!(parse(&text).unwrap_err(), BootError::BadFormat);
        let text =
            FIXTURE
                .replacen("\"format\": 1,", "", 1)
                .replacen("\"common\":", "\"x_common\":", 1);
        let e = parse(&text).unwrap_err();
        assert_eq!(e.error_code(), "package: format");
        assert_eq!(e.exit_code(), 4);
        let e = parse(&FIXTURE.replacen("\"format\": 1", "\"format\": 2", 1)).unwrap_err();
        assert_eq!(e.error_code(), "package: format");
    }
}
