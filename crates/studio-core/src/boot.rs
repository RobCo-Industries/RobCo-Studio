// SPDX-License-Identifier: GPL-3.0-or-later

//! The QEMU command line of a board, read from `boot.json`.
//!
//! `boot.json` holds the boot arguments of each board as data. The format is
//! the "boot.json" section of `docs/spec/TESTING.md` in RobCo-Core. This
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
        }
    }
}

impl std::error::Error for BootError {}

/// Reads the text of a `boot.json`. Fields it does not know are ignored.
pub fn parse(text: &str) -> Result<BootFile, BootError> {
    let root: Value = serde_json::from_str(text).map_err(|e| BootError::Json(e.to_string()))?;
    let format = root
        .get("format")
        .and_then(Value::as_u64)
        .ok_or_else(|| BootError::Field("format".into()))?;
    if format > FORMAT {
        return Err(BootError::NewerFormat(format));
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
        let modes_value = entry
            .get("modes")
            .and_then(Value::as_object)
            .ok_or_else(|| BootError::Field(format!("{at}.modes")))?;
        let mut modes = BTreeMap::new();
        for mode in modes_value.keys() {
            let list = strings(&entry["modes"], mode, &format!("{at}.modes.{mode}"))?;
            modes.insert(mode.clone(), list);
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
    Ok(BootFile { common, sd, boards })
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
    let fill = |arg: &String| -> String {
        let mut out = String::with_capacity(arg.len());
        let mut rest = arg.as_str();
        while let Some(i) = rest.find('{') {
            out.push_str(&rest[..i]);
            let tail = &rest[i..];
            let (text, len) = if tail.starts_with("{stub}") {
                (Some(stub.as_str()), 6)
            } else if tail.starts_with("{image}") {
                (Some(image.as_str()), 7)
            } else if tail.starts_with("{sd}") {
                (card.as_deref(), 4)
            } else {
                (None, 1)
            };
            match text {
                Some(t) => out.push_str(t),
                None => out.push_str(&tail[..len]),
            }
            rest = &tail[len..];
        }
        out.push_str(rest);
        out
    };

    let ports = request.ports;
    let mut args = vec!["-M".to_owned(), board.machine.clone()];
    args.extend(file.common.iter().cloned());
    args.push("-qmp".into());
    args.push(format!("tcp:{HOST}:{},server=on,wait=off", ports.qmp));
    if let Some(port) = ports.qtest {
        args.push("-qtest".into());
        args.push(format!("tcp:{HOST}:{port}"));
    }
    let mut serial = |id: &str, port: u16| {
        args.push("-chardev".into());
        args.push(format!(
            "socket,id={id},host={HOST},port={port},server=on,wait=off"
        ));
        args.push("-serial".into());
        args.push(format!("chardev:{id}"));
    };
    serial("ser0", ports.pl011);
    if let Some(port) = ports.mini_uart {
        serial("ser1", port);
    }
    args.push("-S".into());
    args.extend(mode_args.iter().map(fill));
    if card.is_some() {
        args.extend(file.sd.iter().map(fill));
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
            ("\"format\": 1,", "", "format"),
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
    fn a_wrong_type_fails() {
        let text = FIXTURE.replacen("\"format\": 1", "\"format\": \"1\"", 1);
        assert_eq!(parse(&text).unwrap_err(), BootError::Field("format".into()));
        assert!(matches!(parse("{").unwrap_err(), BootError::Json(_)));
    }
}
