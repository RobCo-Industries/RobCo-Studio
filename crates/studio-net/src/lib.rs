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
//! - A URL is parsed strictly by RFC 3986 and RFC 9110 and never
//!   repaired. The request line and `Host` hold only its parsed parts.
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
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpStream, ToSocketAddrs};
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
        let name = t.server_name()?;
        let conn =
            ClientConnection::new(self.tls.clone(), name).map_err(|e| format!("{at}: TLS: {e}"))?;
        let mut tls = StreamOwned::new(conn, tcp);
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
            t.path,
            t.authority(),
            self.user_agent
        );
        tls.write_all(request.as_bytes())
            .and_then(|()| tls.flush())
            .map_err(|e| io_text(&at, &e, self.stall))?;
        let mut r = BufReader::with_capacity(BUF, tls);
        let mut informational = 0;
        let head = loop {
            let bytes = read_head(&mut r, &at, self.stall)?;
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
                if loc.is_empty() {
                    return Err(format!("{at}: a redirect with an empty Location"));
                }
                return Ok(Answer::Redirect(loc));
            }
            c => return Err(format!("{at}: the server answered with status {c}")),
        }
        // The request asked for `identity`; any other coding is refused.
        if let Some(enc) = head.one("content-encoding", &at)?
            && !enc.eq_ignore_ascii_case("identity")
        {
            return Err(format!("{at}: a content coding other than identity"));
        }
        let te = head.one("transfer-encoding", &at)?;
        let cl = head.one("content-length", &at)?;
        // RFC 9112 6.1: Transfer-Encoding in an HTTP/1.0 answer means the
        // framing is faulty.
        if te.is_some() && head.version == 0 {
            return Err(format!("{at}: Transfer-Encoding in an HTTP/1.0 answer"));
        }
        // RFC 9112 6.3 point 3 lets a client either refuse this or read it
        // as chunked. It is a sign of request smuggling or of a broken
        // proxy, so it is refused.
        if te.is_some() && cl.is_some() {
            return Err(format!(
                "{at}: both Transfer-Encoding and Content-Length are present"
            ));
        }
        let chunked = match te {
            None => false,
            Some(te) if te.eq_ignore_ascii_case("chunked") => true,
            Some(_) => return Err(format!("{at}: a transfer coding other than chunked")),
        };
        let length = match cl {
            None => None,
            Some(v) => {
                // Only digits: Rust's parse takes a leading `+`, and a
                // list such as `5, 5` is refused too.
                if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(format!("{at}: Content-Length is not a number"));
                }
                Some(
                    v.parse::<u64>()
                        .map_err(|_| format!("{at}: Content-Length overflows"))?,
                )
            }
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
            // RFC 9112 6.3 point 8: no length, the body ends with the
            // connection. Only TLS close_notify ends it cleanly.
            body.until_closed()?;
            return Ok(Answer::Done);
        }
        // A framed body that the server kept writing past. Only the bytes
        // already read are looked at: waiting for more would hang on a
        // server that keeps the connection open.
        if !body.r.buffer().is_empty() {
            return Err(format!("{at}: bytes after the end of the body"));
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
    let addrs = (t.host_text().as_str(), t.port)
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
        io::ErrorKind::UnexpectedEof => {
            format!("{at}: the connection ended without TLS close_notify, so the answer may be cut")
        }
        _ => format!("{at}: {e}"),
    }
}

/// A parsed status line and header fields.
struct Head {
    /// 0 for HTTP/1.0, 1 for HTTP/1.1.
    version: u8,
    code: u16,
    fields: Vec<(String, Vec<u8>)>,
}

impl Head {
    fn parse(bytes: &[u8], at: &str) -> Result<Self, String> {
        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut resp = httparse::Response::new(&mut headers);
        match resp.parse(bytes) {
            Ok(httparse::Status::Complete(_)) => {}
            Err(httparse::Error::Version) => {
                return Err(format!("{at}: the answer is not HTTP/1.1 or HTTP/1.0"));
            }
            Err(httparse::Error::Status) => {
                return Err(format!("{at}: the status is not three digits"));
            }
            Err(httparse::Error::TooManyHeaders) => {
                return Err(format!("{at}: more than {MAX_HEADERS} header fields"));
            }
            _ => {
                return Err(format!(
                    "{at}: the status line or a header field is malformed"
                ));
            }
        }
        Ok(Self {
            version: resp.version.unwrap_or(0),
            code: resp.code.unwrap_or(0),
            fields: resp
                .headers
                .iter()
                .map(|h| (h.name.to_owned(), h.value.to_vec()))
                .collect(),
        })
    }

    /// The value of a field the client reads, which may appear only once.
    /// The name is matched without regard to case.
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
            // RFC 9110 5.3 lets a list field repeat, and RFC 9112 6.3
            // lets a recipient merge equal Content-Length values. The
            // fields this client reads decide framing or the target, so a
            // second one is refused, equal or not.
            if found.is_some() {
                return Err(format!("{at}: the {name} field appears twice"));
            }
            found = Some(v);
        }
        Ok(found)
    }
}

/// Reads the status line and the headers, up to the empty line, by RFC
/// 9112 section 2. Each line ends in CRLF; a bare LF, a bare CR, a NUL or
/// another control byte but HTAB, and a line folded onto the next are
/// refused. At most [`MAX_HEAD_BYTES`]; [`Head::parse`] counts the fields.
fn read_head(r: &mut impl BufRead, at: &str, stall: Duration) -> Result<Vec<u8>, String> {
    let mut head = Vec::new();
    let mut lines = 0usize;
    loop {
        let left = MAX_HEAD_BYTES.saturating_sub(head.len() as u64);
        let start = head.len();
        let n = r
            .by_ref()
            .take(left)
            .read_until(b'\n', &mut head)
            .map_err(|e| io_text(at, &e, stall))?;
        let line = &head[start..];
        if !line.ends_with(b"\n") {
            return Err(if n as u64 == left {
                format!("{at}: the header block is longer than {MAX_HEAD_BYTES} bytes")
            } else {
                format!("{at}: the server closed the connection inside the header block")
            });
        }
        if !line.ends_with(b"\r\n") {
            return Err(format!("{at}: a header line ends in a bare LF, not CRLF"));
        }
        let content = &line[..line.len() - 2];
        if content.is_empty() {
            if lines == 0 {
                return Err(format!("{at}: the answer starts with an empty line"));
            }
            return Ok(head);
        }
        if lines > 0 && matches!(content[0], b' ' | b'\t') {
            return Err(format!("{at}: a header line is folded onto the next"));
        }
        if content
            .iter()
            .any(|&c| (c < 0x20 && c != b'\t') || c == 0x7F)
        {
            return Err(format!("{at}: a header line holds a control character"));
        }
        lines += 1;
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
        if n == 0 || !l.ends_with(b"\n") {
            return Err(format!(
                "{}: a chunk line is cut or longer than {MAX_CHUNK_LINE} bytes",
                self.at
            ));
        }
        if !l.ends_with(b"\r\n") {
            return Err(format!(
                "{}: a chunk line ends in a bare LF, not CRLF",
                self.at
            ));
        }
        l.truncate(l.len() - 2);
        String::from_utf8(l).map_err(|_| format!("{}: a chunk line is not text", self.at))
    }

    /// RFC 9112 section 7.1.
    fn chunked(&mut self) -> Result<(), String> {
        loop {
            let line = self.line()?;
            let n = chunk_size(&line).map_err(|e| format!("{}: {e}", self.at))?;
            if n == 0 {
                // Trailer fields, up to the empty line. They are skipped,
                // at most MAX_HEADERS of them.
                let mut trailers = 0;
                while !self.line()?.is_empty() {
                    trailers += 1;
                    if trailers > MAX_HEADERS {
                        return Err(format!(
                            "{}: more than {MAX_HEADERS} trailer fields",
                            self.at
                        ));
                    }
                }
                return Ok(());
            }
            self.exact(n)?;
            if !self.line()?.is_empty() {
                return Err(format!("{}: a chunk is longer than its size", self.at));
            }
        }
    }
}

/// The size of a chunk line: hex digits of either case, then nothing or
/// a chunk extension after optional blanks. The extension is ignored.
fn chunk_size(line: &str) -> Result<u64, &'static str> {
    let end = line
        .find(|c: char| !c.is_ascii_hexdigit())
        .unwrap_or(line.len());
    let (digits, rest) = line.split_at(end);
    if digits.is_empty() {
        return Err("a chunk size is not hex");
    }
    if !rest.is_empty() && !rest.trim_start_matches([' ', '\t']).starts_with(';') {
        return Err("a chunk size is not hex");
    }
    u64::from_str_radix(digits, 16).map_err(|_| "a chunk size overflows")
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

/// The longest path and query a URL may have, in bytes. RFC 9112 3
/// asks a server to take a request line of at least 8000 bytes.
pub const MAX_TARGET_BYTES: usize = 8000;

/// The longest host name, in bytes, by RFC 1035 2.3.4 without the
/// trailing dot.
const MAX_HOST_BYTES: usize = 253;

/// The longest label of a host name, by RFC 1035 2.3.4.
const MAX_LABEL_BYTES: usize = 63;

/// The bytes a path or a query may hold raw, besides letters, digits and
/// a `%` that starts a percent-encoded byte: RFC 3986 3.3 and 3.4,
/// `unreserved`, `sub-delims`, `:`, `@`, `/` and `?`.
const PATH_PUNCT: &[u8] = b"-._~!$&'()*+,;=:@/?";

/// The host of a URL, as it was checked.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Host {
    /// A name of letters, digits, `-` and `.`.
    Name(String),
    /// A dotted IPv4 address.
    V4(Ipv4Addr),
    /// An IPv6 address, from between brackets.
    V6(Ipv6Addr),
}

/// One `https` URL, split for a request.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    host: Host,
    port: u16,
    /// The path and the query, at least `/`. Never the fragment.
    path: String,
}

impl Target {
    /// Splits an `https://` URL by RFC 3986 and RFC 9110 4.2.2, strictly.
    /// Nothing is repaired: a URL is taken as given or refused. A refusal
    /// names the rule and prints no part of the URL but the scheme and a
    /// host that passed its checks.
    fn parse(url: &str) -> Result<Self, String> {
        let no = |rule: &str| format!("an https URL: {rule}");
        // RFC 3986 2 and 7.3: a URL is ASCII. A blank, a control byte, a
        // CR or LF would reach the request line, so none is taken.
        if !url.bytes().all(|b| (0x21..=0x7E).contains(&b)) {
            return Err(no("a byte that is not printable ASCII is refused"));
        }
        // RFC 3986 3.1: the scheme is matched without regard to case.
        let rest = match url.get(..8) {
            Some(s) if s.eq_ignore_ascii_case("https://") => &url[8..],
            _ => return Err("a URL that is not https:// is refused".to_owned()),
        };
        // RFC 3986 3.5: the fragment is for the client and never sent.
        let rest = rest.split_once('#').map_or(rest, |(r, _)| r);
        // RFC 3986 3.2: the authority ends at the first `/` or `?`.
        let end = rest.find(['/', '?']).unwrap_or(rest.len());
        let (authority, path) = rest.split_at(end);
        if authority.contains('@') {
            // RFC 9110 4.2.4: user info is deprecated in an https URL,
            // and it could hold a secret.
            return Err(no("user info is refused"));
        }
        let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
            let (h, after) = v6
                .split_once(']')
                .ok_or_else(|| no("an IPv6 host with no closing bracket"))?;
            // RFC 3986 3.2.2: a bracket holds an IPv6 address only. A zone
            // identifier, RFC 6874, and `IPvFuture` are refused.
            let a = h
                .parse::<Ipv6Addr>()
                .map_err(|_| no("the brackets do not hold a plain IPv6 address"))?;
            let port = match after {
                "" => None,
                p => Some(
                    p.strip_prefix(':')
                        .ok_or_else(|| no("text after the IPv6 host that is not a port"))?,
                ),
            };
            (Host::V6(a), port)
        } else {
            let (h, port) = match authority.split_once(':') {
                Some((h, p)) => (h, Some(p)),
                None => (authority, None),
            };
            (host_of(h).map_err(no)?, port)
        };
        let t = Self {
            host,
            port: 443,
            path: String::new(),
        };
        let shown = t.authority();
        let no = |rule: &str| format!("https://{shown}: {rule}");
        let port = match port {
            None => 443,
            Some(p) => port_of(p).ok_or_else(|| {
                no("the port is not a number from 1 to 65535 without a sign or a leading zero")
            })?,
        };
        if path.len() > MAX_TARGET_BYTES {
            return Err(no(&format!(
                "the path and query are longer than {MAX_TARGET_BYTES} bytes"
            )));
        }
        let b = path.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            if c == b'%' {
                // RFC 3986 2.1: `%` starts exactly two hex digits.
                if !b
                    .get(i + 1..i + 3)
                    .is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit))
                {
                    return Err(no("a % that is not followed by two hex digits"));
                }
                i += 3;
                continue;
            }
            if !c.is_ascii_alphanumeric() && !PATH_PUNCT.contains(&c) {
                // RFC 3986 3.3: such as a backslash, `"`, `<`, `>`, `[`,
                // `]`, `^`, a backtick, `{`, `|` or `}`.
                return Err(no("a byte the path or query may not hold raw"));
            }
            i += 1;
        }
        // RFC 9110 4.2.3: an empty path is `/`.
        let path = if path.starts_with('/') {
            path.to_owned()
        } else {
            format!("/{path}")
        };
        Ok(Self { port, path, ..t })
    }

    /// The `Host` header, RFC 9110 7.2: the host, in brackets for IPv6,
    /// and `:port` unless the port is 443.
    fn authority(&self) -> String {
        let host = match &self.host {
            Host::Name(n) => n.clone(),
            Host::V4(a) => a.to_string(),
            Host::V6(a) => format!("[{a}]"),
        };
        if self.port == 443 {
            host
        } else {
            format!("{host}:{}", self.port)
        }
    }

    /// The host to look up and to check the certificate against, without
    /// brackets.
    fn host_text(&self) -> String {
        match &self.host {
            Host::Name(n) => n.clone(),
            Host::V4(a) => a.to_string(),
            Host::V6(a) => a.to_string(),
        }
    }

    /// The name the certificate is checked against: a DNS name, or an
    /// IP address for an address host.
    fn server_name(&self) -> Result<ServerName<'static>, String> {
        Ok(match &self.host {
            Host::Name(n) => ServerName::try_from(n.clone())
                .map_err(|_| format!("https://{}: not a DNS name", self.authority()))?,
            Host::V4(a) => ServerName::IpAddress(IpAddr::V4(*a).into()),
            Host::V6(a) => ServerName::IpAddress(IpAddr::V6(*a).into()),
        })
    }

    /// The URL without its query, for an error text.
    fn shown(&self) -> String {
        redact(&format!("https://{}{}", self.authority(), self.path))
    }

    /// The target of a `Location` header: an `https://` URL, a URL that
    /// starts with `//`, or a path that starts with `/`. Every other form
    /// is refused, `http://` above all. The result passes [`Self::parse`]
    /// like any URL of an index.
    fn follow(&self, location: &str) -> Result<Self, String> {
        let next = if location
            .get(..8)
            .is_some_and(|s| s.eq_ignore_ascii_case("https://"))
        {
            Self::parse(location)
        } else if location.starts_with("//") {
            // RFC 3986 5.2.2: a network-path reference keeps the scheme.
            Self::parse(&format!("https:{location}"))
        } else if location.starts_with('/') {
            // RFC 3986 5.2.2: an absolute path keeps scheme, host and port.
            Self::parse(&format!("https://{}{location}", self.authority()))
        } else {
            // A path relative to the current one is valid HTTP. The spec
            // does not state it, and no release host is known to send it,
            // so it is refused with the other schemes.
            return Err(format!(
                "{}: a redirect to a relative path or another scheme is refused, only https:// is fetched",
                self.shown()
            ));
        };
        next.map_err(|e| format!("{}: a redirect to {e}", self.shown()))
    }
}

/// Checks a host that is not in brackets: a dotted IPv4 address, or a
/// name by RFC 1123 2.1. The cautious reading of RFC 3986 3.2.2: no
/// percent-encoding, no byte outside letters, digits, `-` and `.`, and no
/// conversion of a name that is not ASCII.
fn host_of(h: &str) -> Result<Host, &'static str> {
    if h.is_empty() {
        return Err("no host");
    }
    if let Ok(a) = h.parse::<Ipv4Addr>() {
        return Ok(Host::V4(a));
    }
    if h.len() > MAX_HOST_BYTES {
        return Err("a host name longer than 253 bytes");
    }
    for label in h.split('.') {
        // An empty label is also a trailing dot or a doubled one.
        if label.is_empty() || label.len() > MAX_LABEL_BYTES {
            return Err("a host name with an empty label or one over 63 bytes");
        }
        if !label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || label.starts_with('-')
            || label.ends_with('-')
        {
            return Err("a host name with a byte other than a letter, a digit, - or .");
        }
    }
    // RFC 3986 3.2.2 and RFC 1123 2.1: a top label is never numeric. The
    // system's resolver would read `127.1` or `0x7f000001` as an address.
    if !h
        .rsplit('.')
        .next()
        .is_some_and(|l| l.starts_with(|c: char| c.is_ascii_alphabetic()))
    {
        return Err("a host that is neither a dotted IPv4 address nor a name");
    }
    Ok(Host::Name(h.to_owned()))
}

/// A port by RFC 3986 3.2.3, strictly: 1 to 65535 in plain digits, no
/// sign, no leading zero. An empty port fails the number parse.
fn port_of(p: &str) -> Option<u16> {
    if p.len() > 5 || p.starts_with('0') || !p.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    p.parse().ok()
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
