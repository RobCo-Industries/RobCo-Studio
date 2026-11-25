//! `k6 list | check | write | verify | make-image`.
//!
//! Exit codes: 0 done, 2 usage or I/O error, 3 refused, 4 read-back differs.

use card_writer::rules::{Policy, decide};
use card_writer::{live_list, write};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "usage:
  k6 list
  k6 check  --device D [--image F] [--allow-virtual] [--max-size-mib N]
  k6 write  --device D --image F [--allow-virtual] [--max-size-mib N]
  k6 verify --device D --image F
  k6 make-image --out F --size-mib N --seed S";

struct Args {
    cmd: String,
    device: Option<String>,
    image: Option<String>,
    out: Option<String>,
    size_mib: Option<u64>,
    seed: u64,
    policy: Policy,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().ok_or("no command")?;
    let mut a = Args {
        cmd,
        device: None,
        image: None,
        out: None,
        size_mib: None,
        seed: 1,
        policy: Policy::default(),
    };
    while let Some(k) = it.next() {
        let mut val = || it.next().ok_or(format!("{k} needs a value"));
        let num = |s: String| s.parse::<u64>().map_err(|_| format!("bad number {s}"));
        match k.as_str() {
            "--device" => a.device = Some(val()?),
            "--image" => a.image = Some(val()?),
            "--out" => a.out = Some(val()?),
            "--size-mib" => a.size_mib = Some(num(val()?)?),
            "--seed" => a.seed = num(val()?)?,
            "--max-size-mib" => a.policy.max_size = num(val()?)? * 1024 * 1024,
            "--allow-virtual" => a.policy.allow_virtual = true,
            _ => return Err(format!("unknown argument {k}")),
        }
    }
    Ok(a)
}

fn main() -> ExitCode {
    let a = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match a.cmd.as_str() {
        "list" => {
            let list = live_list();
            println!("{:#}", list.to_json());
            ExitCode::SUCCESS
        }
        "check" | "write" => {
            let Some(device) = a.device.as_deref() else {
                eprintln!("--device is required\n{USAGE}");
                return ExitCode::from(2);
            };
            let image_len = match a.image.as_deref().map(std::fs::metadata) {
                Some(Ok(m)) => m.len(),
                Some(Err(e)) => {
                    eprintln!("image: {e}");
                    return ExitCode::from(2);
                }
                None if a.cmd == "write" => {
                    eprintln!("--image is required\n{USAGE}");
                    return ExitCode::from(2);
                }
                None => 0,
            };
            // The list is read again here, right before the decision and the open.
            let list = live_list();
            let approved = match decide(&list, device, image_len, &a.policy) {
                Ok(t) => t,
                Err(why) => {
                    println!("REFUSED {device}");
                    for w in why {
                        println!("  - {w:?}");
                    }
                    return ExitCode::from(3);
                }
            };
            println!("APPROVED {}", approved.path());
            if a.cmd == "check" {
                return ExitCode::SUCCESS;
            }
            let image = a.image.unwrap_or_default();
            match write::write_and_verify(approved, Path::new(&image)) {
                Ok(o) => {
                    println!(
                        "wrote {} bytes to {} in {} ms, read back in {} ms",
                        o.bytes, o.raw_path, o.write_ms, o.read_ms
                    );
                    match o.mismatch {
                        None => {
                            println!("READ-BACK OK");
                            ExitCode::SUCCESS
                        }
                        Some(at) => {
                            println!("READ-BACK DIFFERS at byte {at}");
                            ExitCode::from(4)
                        }
                    }
                }
                Err(e) => {
                    eprintln!("write failed: {e}");
                    ExitCode::from(2)
                }
            }
        }
        "verify" => {
            let (Some(d), Some(i)) = (a.device, a.image) else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            let len = match std::fs::metadata(&i) {
                Ok(m) => m.len().div_ceil(4096) * 4096,
                Err(e) => {
                    eprintln!("image: {e}");
                    return ExitCode::from(2);
                }
            };
            match write::compare(&write::raw_path(&d), Path::new(&i), len) {
                Ok(None) => {
                    println!("READ-BACK OK");
                    ExitCode::SUCCESS
                }
                Ok(Some(at)) => {
                    println!("READ-BACK DIFFERS at byte {at}");
                    ExitCode::from(4)
                }
                Err(e) => {
                    eprintln!("verify failed: {e}");
                    ExitCode::from(2)
                }
            }
        }
        "make-image" => {
            let (Some(o), Some(s)) = (a.out, a.size_mib) else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            match write::make_image(Path::new(&o), (s * 1024 * 1024) as usize, a.seed) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("make-image: {e}");
                    ExitCode::from(2)
                }
            }
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
