// SPDX-License-Identifier: GPL-3.0-or-later

//! The `https` transport of the command, and the transport of one index:
//! its archives come from `https://` URLs or, for a local index, from
//! files in its folder.

use std::io::Write;
use std::sync::OnceLock;

use studio_core::install::{LocalFiles, Transport};
use studio_net::Https;

/// The `User-Agent` of every request: the program's name and version.
const USER_AGENT: &str = concat!("robco/", env!("CARGO_PKG_VERSION"));

/// Downloads from `https://` URLs. TLS starts on the first download, so a
/// command that downloads nothing never starts it.
pub struct Net {
    https: OnceLock<Result<Https, String>>,
}

impl Net {
    /// Checks certificates with the system's verifier and trust store.
    pub fn system() -> Self {
        Self {
            https: OnceLock::new(),
        }
    }

    /// For tests only: downloads with this transport, such as one that
    /// trusts a test certificate. The program never calls it.
    pub fn with(https: Https) -> Self {
        Self {
            https: OnceLock::from(Ok(https)),
        }
    }

    /// The transport, or why TLS cannot start.
    pub(crate) fn https(&self) -> Result<&Https, String> {
        self.https
            .get_or_init(|| Https::system(USER_AGENT))
            .as_ref()
            .map_err(Clone::clone)
    }
}

/// The transport of the archives of one index.
pub(crate) struct Archives<'a> {
    /// The folder of a local index. `None` for an `https` index, whose
    /// entries are all `https://`.
    pub local: Option<LocalFiles>,
    pub net: &'a Net,
}

impl Transport for Archives<'_> {
    fn fetch(&self, url: &str, out: &mut dyn Write) -> Result<(), String> {
        if url.starts_with("https://") {
            return match self.net.https() {
                Ok(h) => h.fetch(url, out),
                Err(e) => Err(format!("nothing was fetched: {e}")),
            };
        }
        match &self.local {
            Some(l) => l.fetch(url, out),
            None => Err(format!(
                "nothing was fetched from {}: not an https URL",
                studio_net::redact(url)
            )),
        }
    }
}
