// SPDX-License-Identifier: GPL-3.0-or-later

//! `robco sdk install`, by "robco sdk install" of RobCo-Core
//! `docs/spec/STUDIO_CLI.md`.
//!
//! `studio_core::install` downloads, checks and installs an item in one
//! call. The `step` lines come from a transport that wraps the real one:
//! `download` opens when the fetch starts and closes when it ends, and
//! `verify` opens right after. When the call returns `Ok`, `verify` closes
//! and `install` opens and closes. An item the call finds in the cache was
//! never fetched: it writes one `install` line with `skip`. A failure
//! leaves the step it happened in open, as "step" allows.

use std::cell::{Cell, RefCell};
use std::io::Write;
use std::path::Path;

use studio_core::install::{self, LocalFiles, Pin, State, Transport};
use studio_core::resolve::{self, InstallPick, PIPOS, Root};

use crate::Failure;
use crate::index::{self, Location};
use crate::lines::{Out, Val};

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

/// A transport that writes the `download` and `verify` start lines.
struct Watch<'a, 'm, 'o> {
    inner: &'a dyn Transport,
    out: &'a RefCell<&'m mut Out<'o>>,
    item: &'a str,
    fetched: Cell<bool>,
}

impl Transport for Watch<'_, '_, '_> {
    fn fetch(&self, url: &str, sink: &mut dyn Write) -> Result<(), String> {
        self.fetched.set(true);
        self.out.borrow_mut().step("download", self.item, "start");
        let r = self.inner.fetch(url, sink);
        if r.is_ok() {
            let mut out = self.out.borrow_mut();
            out.step("download", self.item, "done");
            out.step("verify", self.item, "start");
        }
        r
    }
}

/// Runs one install call with its `step` lines.
fn watched<'m, 'o>(
    out: &RefCell<&'m mut Out<'o>>,
    transport: &dyn Transport,
    item: &str,
    call: impl FnOnce(&dyn Transport) -> Result<State, install::Error>,
) -> Result<State, Failure> {
    let watch = Watch {
        inner: transport,
        out,
        item,
        fetched: Cell::new(false),
    };
    let state = call(&watch).map_err(|e| Failure::of(&e))?;
    let mut out = out.borrow_mut();
    if watch.fetched.get() {
        out.step("verify", item, "done");
        out.step("install", item, "start");
        out.step("install", item, "done");
    } else {
        out.step("install", item, "skip");
    }
    Ok(state)
}

fn state_word(s: State) -> &'static str {
    match s {
        State::Installed => "installed",
        State::Present => "present",
    }
}

/// Runs the command, writing every line but the error and the failed
/// `result` line, which the caller writes from the returned failure.
pub fn run(out: &mut Out<'_>, cache: &Path, req: &Request<'_>) -> Result<(), Failure> {
    // Step 1: the location comes before the read. `resolve` covers reading
    // the index and the picks, steps 1 to 3.
    let location = index::locate(req.index)?;
    out.step("resolve", "", "start");
    let (idx, base) = index::read(&location)?;
    let transport = match &location {
        Location::Local(_) => LocalFiles { base },
        // `index::read` already refused it.
        Location::Https(_) => {
            return Err(Failure::new("internal", "an https index was read"));
        }
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
    let cell = RefCell::new(out);
    let mut done_packages = Vec::new();
    for p in &packages {
        let item = format!("{}@{}", p.name, p.version);
        let state = watched(&cell, &transport, &item, |t| {
            install::package(cache, p, t, Pin::Index)
        })?;
        done_packages.push((p.name.clone(), p.version.to_string(), state));
    }
    let mut done_tools = Vec::new();
    for (t, host, archive) in &tools {
        let item = format!("{}@{}/{host}", t.name, t.version);
        let state = watched(&cell, &transport, &item, |tr| {
            install::tool(cache, &t.name, &t.version, host, archive, tr, Pin::Index)
        })?;
        done_tools.push((t.name.clone(), t.version.clone(), (*host).to_owned(), state));
    }
    let out = cell.into_inner();

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
