// SPDX-License-Identifier: GPL-3.0-or-later

//! The `https` transport of RobCo Studio. It downloads an index and the
//! archives an index names, by "Where the index is" and "Hashes" of
//! RobCo-Core `docs/spec/SDK_PACKAGE.md`.
//!
//! It only moves bytes. The installer of `studio-core` counts and hashes
//! what [`Https`] writes, stops at the size the index states and decides
//! every error code.
//!
//! A small HTTP/1.1 client over `rustls`, so that every rule is in sight:
//!
//! - `https` only, also after a redirect. At most [`MAX_REDIRECTS`]
//!   redirects, to any host.
//! - Certificates are checked by the system's verifier and trust store.
//!   Nothing turns that off.
//! - [`CONNECT_TIMEOUT`] to connect and [`STALL_TIMEOUT`] for a read or a
//!   write that makes no progress. No retry.
//! - The body streams into the caller's writer. When the writer refuses a
//!   byte, the transfer ends and the connection closes.
//! - A request carries exactly the headers `Host`, `User-Agent`,
//!   `Accept-Encoding: identity` and `Connection: close`. No cookie, no
//!   token. A proxy of the environment, such as `HTTPS_PROXY`, is not
//!   used.
//! - An error names a URL without its query and fragment, and never prints
//!   a header.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};
use studio_core::install::Transport;

/// The time to open a TCP connection to one address of a host.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// The longest a read or a write may wait for one byte to move, the TLS
/// handshake included. It bounds a stalled server, not a slow download:
/// an archive of any size may take as long as it needs while bytes flow.
pub const STALL_TIMEOUT: Duration = Duration::from_secs(60);

/// How many redirects one fetch follows. Release files on GitHub take one
/// redirect to another host. Five leaves room and still ends a loop.
pub const MAX_REDIRECTS: usize = 5;

/// The largest index this tool reads. The spec states no size for an
/// index. One entry is about 500 bytes, so 8 MiB holds some 16,000
/// entries, far more than one publisher lists. A longer one is refused
/// with `index: read`.
pub const MAX_INDEX_BYTES: u64 = 8 * 1024 * 1024;

/// How many informational `1xx` answers may come before the final one.
/// A server sends one or two, such as `103 Early Hints`.
pub const MAX_INFORMATIONAL: usize = 4;

/// The longest status line and headers of a response, together.
const MAX_HEAD_BYTES: u64 = 64 * 1024;

/// The most headers a response may have.
const MAX_HEADERS: usize = 64;

/// The longest size line of a chunked body.
const MAX_CHUNK_LINE: u64 = 1024;

/// The size of one read from the connection.
const BUF: usize = 64 * 1024;

/// The `https` transport. Cheap to clone.
#[derive(Clone)]
pub struct Https {
    tls: Arc<ClientConfig>,
    user_agent: String,
    connect: Duration,
    stall: Duration,
}

impl std::fmt::Debug for Https {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Https")
            .field("user_agent", &self.user_agent)
            .field("connect", &self.connect)
            .field("stall", &self.stall)
            .finish_non_exhaustive()
    }
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

impl Https {
    /// A transport that checks certificates with the system's verifier and
    /// trust store. `user_agent` is the program's name and version, such as
    /// `robco/0.1.0`, and nothing else.
    pub fn system(user_agent: &str) -> Result<Self, String> {
        use rustls_platform_verifier::BuilderVerifierExt;
        let tls = ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .and_then(|b| b.with_platform_verifier())
            .map_err(|e| format!("TLS cannot start: {e}"))?
            .with_no_client_auth();
        Ok(Self::with(tls, user_agent, CONNECT_TIMEOUT, STALL_TIMEOUT))
    }

    /// For tests only: a transport that trusts exactly one certificate,
    /// given in DER, with the timeouts given. Only the `test-trust`
    /// feature, which only dev-dependencies turn on, builds it.
    #[cfg(any(test, feature = "test-trust"))]
    pub fn trusting_only(
        cert_der: &[u8],
        user_agent: &str,
        connect: Duration,
        stall: Duration,
    ) -> Result<Self, String> {
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(rustls::pki_types::CertificateDer::from(cert_der.to_vec()))
            .map_err(|e| format!("the test certificate: {e}"))?;
        let tls = ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .map_err(|e| format!("TLS cannot start: {e}"))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(Self::with(tls, user_agent, connect, stall))
    }

    fn with(tls: ClientConfig, user_agent: &str, connect: Duration, stall: Duration) -> Self {
        Self {
            tls: Arc::new(tls),
            user_agent: user_agent.to_owned(),
            connect,
            stall,
        }
    }

    /// Downloads an index, at most [`MAX_INDEX_BYTES`]. The error is the
    /// detail of `index: read`.
    pub fn fetch_index(&self, url: &str) -> Result<Vec<u8>, String> {
        let mut out = Bounded {
            bytes: Vec::new(),
            limit: MAX_INDEX_BYTES,
            over: false,
        };
        let got = self.fetch(url, &mut out);
        if out.over {
            return Err(format!(
                "{}: the index is longer than {MAX_INDEX_BYTES} bytes",
                redact(url)
            ));
        }
        got.map(|()| out.bytes)
    }

    /// Follows redirects and streams the body of the final answer.
    fn get(&self, url: &str, out: &mut dyn Write) -> Result<(), String> {
        let mut target = Target::parse(url)?;
        let mut hops = 0;
        loop {
            match self.once(&target, out)? {
                Answer::Done => return Ok(()),
                Answer::Redirect(location) => {
                    hops += 1;
                    if hops > MAX_REDIRECTS {
                        return Err(format!(
                            "{}: more than {MAX_REDIRECTS} redirects",
                            redact(url)
                        ));
                    }
                    target = target.follow(&location)?;
                }
            }
        }
    }

    /// One request and its answer.
    fn once(&self, t: &Target, out: &mut dyn Write) -> Result<Answer, String> {
        let at = t.shown();
        let tcp = connect(t, self.connect, &at)?;
        let set = |e: io::Error| format!("{at}: {e}");
        tcp.set_read_timeout(Some(self.stall)).map_err(set)?;
        tcp.set_write_timeout(Some(self.stall)).map_err(set)?;
        let name = ServerName::try_from(t.host.clone())
            .map_err(|_| format!("{at}: the host is not a DNS name or an address"))?;
        let conn =
            ClientConnection::new(self.tls.clone(), name).map_err(|e| format!("{at}: TLS: {e}"))?;
        let mut tls = StreamOwned::new(conn, tcp);
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
            t.path, t.authority, self.user_agent
        );
        tls.write_all(request.as_bytes())
            .and_then(|()| tls.flush())
            .map_err(|e| io_text(&at, &e, self.stall))?;
        let mut r = BufReader::with_capacity(BUF, tls);
        let mut informational = 0;
        let head = loop {
            let bytes = read_head(&mut r).map_err(|e| io_text(&at, &e, self.stall))?;
            let head = Head::parse(&bytes, &at)?;
            match head.code {
                // RFC 9110 15.2.2: the client asked for no upgrade.
                101 => {
                    return Err(format!(
                        "{at}: status 101, a protocol switch nobody asked for"
                    ));
                }
                // RFC 9110 15.2: an informational answer comes before the
                // final one and has no body.
                100..=199 => {
                    informational += 1;
                    if informational > MAX_INFORMATIONAL {
                        return Err(format!(
                            "{at}: more than {MAX_INFORMATIONAL} informational answers"
                        ));
                    }
                }
                _ => break head,
            }
        };
        match head.code {
            200 => {}
            301 | 302 | 303 | 307 | 308 => {
                let loc = head
                    .one("location", &at)?
                    .ok_or_else(|| format!("{at}: a redirect with no Location"))?;
                return Ok(Answer::Redirect(loc));
            }
            c => return Err(format!("{at}: the server answered with status {c}")),
        }
        if let Some(enc) = head.one("content-encoding", &at)?
            && !enc.eq_ignore_ascii_case("identity")
        {
            return Err(format!("{at}: the body is encoded, {enc}"));
        }
        let chunked = match head.one("transfer-encoding", &at)? {
            None => false,
            Some(te) if te.eq_ignore_ascii_case("chunked") => true,
            Some(te) => return Err(format!("{at}: transfer encoding {te} is not chunked")),
        };
        let length = match head.one("content-length", &at)? {
            None => None,
            Some(v) => Some(
                v.parse::<u64>()
                    .map_err(|_| format!("{at}: Content-Length is not a number"))?,
            ),
        };
        let mut body = Body {
            r,
            out,
            at: &at,
            stall: self.stall,
            moved: 0,
        };
        if chunked {
            body.chunked()?;
        } else if let Some(n) = length {
            body.exact(n)?;
        } else {
            body.until_closed()?;
        }
        Ok(Answer::Done)
    }
}

impl Transport for Https {
    /// Fetches an `https://` URL. Any other `url`, such as a relative path,
    /// is refused and nothing is fetched.
    fn fetch(&self, url: &str, out: &mut dyn Write) -> Result<(), String> {
        let mut counted = Counted { out, n: 0 };
        self.get(url, &mut counted).map_err(|e| {
            if counted.n == 0 {
                format!("nothing was fetched: {e}")
            } else {
                e
            }
        })
    }
}

enum Answer {
    Done,
    Redirect(String),
}

/// Opens TCP to the first address of the host that answers.
fn connect(t: &Target, timeout: Duration, at: &str) -> Result<TcpStream, String> {
    // The spec does not state a time limit for the name lookup; the
    // system's resolver applies its own.
    let addrs = (t.host.as_str(), t.port)
        .to_socket_addrs()
        .map_err(|e| format!("{at}: the host name was not found: {}", e.kind()))?;
    let mut last = None;
    for a in addrs {
        match TcpStream::connect_timeout(&a, timeout) {
            Ok(s) => return Ok(s),
            Err(e) => last = Some(e),
        }
    }
    Err(match last {
        Some(e) => format!("{at}: no connection: {}", e.kind()),
        None => format!("{at}: the host has no address"),
    })
}

/// The text of an I/O error of the connection. A timeout says how long.
fn io_text(at: &str, e: &io::Error, stall: Duration) -> String {
    match e.kind() {
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => {
            format!("{at}: no byte moved for {} ms", stall.as_millis())
        }
        io::ErrorKind::UnexpectedEof => format!("{at}: the server closed the connection early"),
        _ => format!("{at}: {e}"),
    }
}

/// A parsed status line and header fields.
struct Head {
    code: u16,
    fields: Vec<(String, Vec<u8>)>,
}

impl Head {
    fn parse(bytes: &[u8], at: &str) -> Result<Self, String> {
        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut resp = httparse::Response::new(&mut headers);
        match resp.parse(bytes) {
            Ok(httparse::Status::Complete(_)) => {}
            _ => return Err(format!("{at}: the answer is not HTTP/1.1")),
        }
        Ok(Self {
            code: resp.code.unwrap_or(0),
            fields: resp
                .headers
                .iter()
                .map(|h| (h.name.to_owned(), h.value.to_vec()))
                .collect(),
        })
    }

    /// The value of a field the client reads, which may appear once.
    fn one(&self, name: &str, at: &str) -> Result<Option<String>, String> {
        let mut found: Option<String> = None;
        for (_, value) in self
            .fields
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
        {
            let v = std::str::from_utf8(value)
                .map_err(|_| format!("{at}: a {name} header is not text"))?
                .trim()
                .to_owned();
            if found.as_ref().is_some_and(|f| *f != v) {
                return Err(format!("{at}: two {name} headers differ"));
            }
            found = Some(v);
        }
        Ok(found)
    }
}

/// Reads the status line and the headers, up to the empty line.
fn read_head(r: &mut impl BufRead) -> io::Result<Vec<u8>> {
    let mut head = Vec::new();
    loop {
        let left = MAX_HEAD_BYTES.saturating_sub(head.len() as u64);
        let n = r.by_ref().take(left).read_until(b'\n', &mut head)?;
        if n == 0 || !head.ends_with(b"\n") {
            return Err(if left == 0 || n as u64 == left {
                io::Error::other("the headers are too long")
            } else {
                io::Error::from(io::ErrorKind::UnexpectedEof)
            });
        }
        if head.ends_with(b"\r\n\r\n") || head == b"\r\n" {
            return Ok(head);
        }
    }
}

/// The body of an answer, streamed into the caller's writer.
struct Body<'a, R> {
    r: R,
    out: &'a mut dyn Write,
    at: &'a str,
    stall: Duration,
    moved: u64,
}

impl<R: BufRead> Body<'_, R> {
    fn io(&self, e: &io::Error) -> String {
        io_text(self.at, e, self.stall)
    }

    fn give(&mut self, buf: &[u8]) -> Result<(), String> {
        // The writer refused: the transfer ends here, and dropping the
        // connection closes it.
        self.out
            .write_all(buf)
            .map_err(|e| format!("{}: the receiver took no more bytes: {e}", self.at))?;
        self.moved += buf.len() as u64;
        Ok(())
    }

    /// Exactly `n` bytes.
    fn exact(&mut self, n: u64) -> Result<(), String> {
        let mut buf = vec![0u8; BUF];
        let mut left = n;
        while left > 0 {
            let want = usize::try_from(left.min(BUF as u64)).unwrap_or(BUF);
            let got = match self.r.read(&mut buf[..want]) {
                Ok(0) => {
                    return Err(format!(
                        "{}: the server closed the connection after {} of {n} bytes",
                        self.at,
                        n - left
                    ));
                }
                Ok(k) => k,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(self.io(&e)),
            };
            self.give(&buf[..got])?;
            left -= got as u64;
        }
        Ok(())
    }

    /// Everything up to the end of the connection. TLS tells a clean end
    /// from a cut one: a close without TLS's `close_notify` is an error.
    fn until_closed(&mut self) -> Result<(), String> {
        let mut buf = vec![0u8; BUF];
        loop {
            match self.r.read(&mut buf) {
                Ok(0) => return Ok(()),
                Ok(k) => self.give(&buf[..k])?,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(self.io(&e)),
            }
        }
    }

    /// A line of a chunked body, without its line end.
    fn line(&mut self) -> Result<String, String> {
        let mut l = Vec::new();
        let n = self
            .r
            .by_ref()
            .take(MAX_CHUNK_LINE)
            .read_until(b'\n', &mut l)
            .map_err(|e| self.io(&e))?;
        if n == 0 || !l.ends_with(b"\r\n") {
            return Err(format!("{}: a chunked body is cut or broken", self.at));
        }
        l.truncate(l.len() - 2);
        String::from_utf8(l).map_err(|_| format!("{}: a chunk line is not text", self.at))
    }

    fn chunked(&mut self) -> Result<(), String> {
        loop {
            let line = self.line()?;
            let size = line.split(';').next().unwrap_or("").trim();
            let n = u64::from_str_radix(size, 16)
                .map_err(|_| format!("{}: a chunk size is not hex", self.at))?;
            if n == 0 {
                // Trailers, up to the empty line. They are not read.
                while !self.line()?.is_empty() {}
                return Ok(());
            }
            self.exact(n)?;
            if !self.line()?.is_empty() {
                return Err(format!("{}: a chunk is longer than its size", self.at));
            }
        }
    }
}

/// A writer that counts what it passes on.
struct Counted<'a> {
    out: &'a mut dyn Write,
    n: u64,
}

impl Write for Counted<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let k = self.out.write(buf)?;
        self.n += k as u64;
        Ok(k)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

/// A writer into memory that refuses more than `limit` bytes.
struct Bounded {
    bytes: Vec<u8>,
    limit: u64,
    over: bool,
}

impl Write for Bounded {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.bytes.len() as u64 + buf.len() as u64 > self.limit {
            self.over = true;
            return Err(io::Error::other("longer than the limit"));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// One `https` URL, split for a request.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    /// The host, without the brackets of an IPv6 address.
    host: String,
    port: u16,
    /// The host and port as the URL gives them, for the `Host` header.
    authority: String,
    /// The path and the query, at least `/`.
    path: String,
}

impl Target {
    /// Splits an `https://` URL. Refuses every other scheme, user info,
    /// and any byte that is not printable ASCII.
    fn parse(url: &str) -> Result<Self, String> {
        let shown = redact(url);
        if !url.bytes().all(|b| (0x21..=0x7E).contains(&b)) {
            return Err(format!("{shown}: not a URL of printable ASCII"));
        }
        let Some(rest) = url.strip_prefix("https://") else {
            return Err(format!("{shown}: only https:// is fetched"));
        };
        let rest = rest.split('#').next().unwrap_or("");
        let end = rest.find(['/', '?']).unwrap_or(rest.len());
        let (authority, path) = rest.split_at(end);
        if authority.contains('@') {
            // The spec does not state user info in a URL. It could hold a
            // secret, so it is refused.
            return Err(format!("{shown}: a URL with user info is refused"));
        }
        let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
            let (h, after) = v6
                .split_once(']')
                .ok_or_else(|| format!("{shown}: a broken IPv6 host"))?;
            (h, after.strip_prefix(':'))
        } else {
            match authority.rsplit_once(':') {
                Some((h, p)) => (h, Some(p)),
                None => (authority, None),
            }
        };
        let port = match port {
            None => 443,
            Some(p) => p
                .parse::<u16>()
                .ok()
                .filter(|&p| p != 0)
                .ok_or_else(|| format!("{shown}: a bad port"))?,
        };
        if host.is_empty() {
            return Err(format!("{shown}: no host"));
        }
        let path = if path.starts_with('/') {
            path.to_owned()
        } else {
            format!("/{path}")
        };
        Ok(Self {
            host: host.to_owned(),
            port,
            authority: authority.to_owned(),
            path,
        })
    }

    /// The URL without its query, for an error text.
    fn shown(&self) -> String {
        redact(&format!("https://{}{}", self.authority, self.path))
    }

    /// The target of a `Location` header: an `https://` URL, a URL that
    /// starts with `//`, or a path that starts with `/`. Every other form
    /// is refused, `http://` above all.
    fn follow(&self, location: &str) -> Result<Self, String> {
        if location.starts_with("https://") {
            Self::parse(location)
        } else if location.starts_with("//") {
            Self::parse(&format!("https:{location}"))
        } else if location.starts_with('/') {
            Self::parse(&format!("https://{}{location}", self.authority))
        } else {
            // A path relative to the current one is valid HTTP. The spec
            // does not state it, and no release host is known to send it,
            // so it is refused with the other schemes.
            Err(format!(
                "{}: a redirect to {} is refused, only https:// is fetched",
                self.shown(),
                redact(location)
            ))
        }
    }
}

/// A URL for an error text: no query, no fragment and no user info, since
/// any of them may hold a secret.
pub fn redact(url: &str) -> String {
    let cut = url.find(['?', '#']).unwrap_or(url.len());
    let url = &url[..cut];
    match url.split_once("://") {
        Some((scheme, rest)) => {
            let end = rest.find('/').unwrap_or(rest.len());
            let (auth, path) = rest.split_at(end);
            let host = auth.rsplit_once('@').map_or(auth, |(_, h)| h);
            format!("{scheme}://{host}{path}")
        }
        None => url.to_owned(),
    }
}

#[cfg(any(test, feature = "test-trust"))]
pub mod testing;

#[cfg(test)]
mod tests;
