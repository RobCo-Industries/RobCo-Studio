// SPDX-License-Identifier: GPL-3.0-or-later

//! `robco sdk install`, by "robco sdk install" of RobCo-Core
//! `docs/spec/STUDIO_CLI.md`.
//!
//! `studio_core::install` tells the command each step as it begins and
//! ends, and the command writes one `step` line per transition. A failure
//! leaves the step it happened in open, a `start` with no `done`, as
//! "step" allows, and the error line follows. An item the cache holds
//! writes one `install` line with `skip`.

use std::path::Path;

use studio_core::install::{self, LocalFiles, Phase, Pin, Progress, State};
use studio_core::resolve::{self, InstallPick, PIPOS, Root};

use crate::index;
use crate::lines::{Out, Val};
use crate::net::Archives;
use crate::{Failure, Net};

/// What `robco sdk install` was asked for.
pub struct Request<'a> {
    /// The package argument as given, such as `pipos@1.4`.
    pub argument: &'a str,
    /// The name before `@`.
    pub name: &'a str,
    /// Which versions the argument allows.
    pub pick: InstallPick,
    /// The `--index` location, when given.
    pub index: Option<&'a str>,
    /// The running host, or `None` on a system the index has no name for.
    pub host: Option<&'a str>,
}

/// Writes the `step` lines of one item.
struct Lines<'a, 'o> {
    out: &'a mut Out<'o>,
    item: &'a str,
}

fn word(phase: Phase) -> &'static str {
    match phase {
        Phase::Download => "download",
        Phase::Verify => "verify",
        Phase::Install => "install",
    }
}

impl Progress for Lines<'_, '_> {
    fn start(&mut self, phase: Phase) {
        self.out.step(word(phase), self.item, "start");
    }
    fn done(&mut self, phase: Phase) {
        self.out.step(word(phase), self.item, "done");
    }
    fn skip(&mut self) {
        self.out.step("install", self.item, "skip");
    }
}

fn state_word(s: State) -> &'static str {
    match s {
        State::Installed => "installed",
        State::Present => "present",
    }
}

/// Runs the command, writing every line but the error and the failed
/// `result` line, which the caller writes from the returned failure.
pub fn run(out: &mut Out<'_>, cache: &Path, net: &Net, req: &Request<'_>) -> Result<(), Failure> {
    // Step 1: the location comes before the read. `resolve` covers reading
    // the index and the picks, steps 1 to 3.
    let location = index::locate(req.index)?;
    out.step("resolve", "", "start");
    let (idx, base) = index::read(&location, net)?;
    let transport = Archives {
        local: base.map(|base| LocalFiles { base }),
        net,
    };

    // Step 2.
    let root = Root::Install {
        name: req.name.to_owned(),
        pick: req.pick.clone(),
        argument: req.argument.to_owned(),
    };
    let packages = resolve::resolve_requires(&idx.packages, &root).map_err(|e| Failure::of(&e))?;

    // Step 3.
    let mut tools = Vec::new();
    if let Some(pipos) = packages.iter().find(|p| p.name == PIPOS) {
        let host = req.host.unwrap_or("unknown");
        for t in resolve::pick_tools(&idx, pipos, host).map_err(|e| Failure::of(&e))? {
            // `pick_tools` checked that the host has an archive.
            let Some(archive) = t.archives.get(host) else {
                return Err(Failure::new("internal", "a picked tool lost its archive"));
            };
            tools.push((t, host, archive));
        }
    }
    out.step("resolve", "", "done");

    // Step 4. `resolve_requires` sorts the packages by name in byte order.
    let mut done_packages = Vec::new();
    for p in &packages {
        let item = format!("{}@{}", p.name, p.version);
        let mut lines = Lines { out, item: &item };
        let state = install::package_watched(cache, p, &transport, Pin::Index, &mut lines)
            .map_err(|e| Failure::of(&e))?;
        done_packages.push((p.name.clone(), p.version.to_string(), state));
    }
    let mut done_tools = Vec::new();
    for (t, host, archive) in &tools {
        let item = format!("{}@{}/{host}", t.name, t.version);
        let mut lines = Lines { out, item: &item };
        let state = install::tool_watched(
            cache,
            [&t.name, &t.version, host],
            archive,
            &transport,
            Pin::Index,
            &mut lines,
        )
        .map_err(|e| Failure::of(&e))?;
        done_tools.push((t.name.clone(), t.version.clone(), (*host).to_owned(), state));
    }

    for (name, version, state) in &done_packages {
        out.text(&format!("package {name} {version} {}", state_word(*state)));
    }
    for (name, version, host, state) in &done_tools {
        out.text(&format!(
            "tool {name} {version} {host} {}",
            state_word(*state)
        ));
    }
    let packages = done_packages
        .into_iter()
        .map(|(name, version, state)| {
            Val::Obj(vec![
                ("name", Val::s(name)),
                ("version", Val::s(version)),
                ("state", Val::s(state_word(state))),
            ])
        })
        .collect();
    let tools = done_tools
        .into_iter()
        .map(|(name, version, host, state)| {
            Val::Obj(vec![
                ("name", Val::s(name)),
                ("version", Val::s(version)),
                ("host", Val::s(host)),
                ("state", Val::s(state_word(state))),
            ])
        })
        .collect();
    out.ok(
        "sdk install",
        vec![("packages", Val::Arr(packages)), ("tools", Val::Arr(tools))],
    );
    Ok(())
}
