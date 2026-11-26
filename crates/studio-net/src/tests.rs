// SPDX-License-Identifier: GPL-3.0-or-later

//! Every test serves on 127.0.0.1 with a certificate it made. None touches
//! the network.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test; a failed unwrap is a failed test"
)]

use std::io::{self, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

use studio_core::install::Transport;

use super::*;
use crate::testing::{Handler, Pki, Server, ok, raw, redirect};

const UA: &str = "robco/0.0.0";

fn client(pki: &Pki) -> Https {
    client_with(pki, Duration::from_secs(10))
}

fn client_with(pki: &Pki, stall: Duration) -> Https {
    Https::trusting_only(&pki.ca_der, UA, Duration::from_secs(10), stall).unwrap()
}

fn fetch(h: &Https, url: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    h.fetch(url, &mut out).map(|()| out)
}

#[test]
fn a_good_fetch_streams_the_body_and_sends_only_the_stated_headers() {
    let pki = Pki::new();
    let s = Server::start(&pki, vec![ok(b"hello".to_vec())]);
    let url = s.url("/a/i.json?x=1");
    assert_eq!(fetch(&client(&pki), &url).unwrap(), b"hello");
    let seen = s.finish();
    assert_eq!(
        seen[0].request,
        format!(
            "GET /a/i.json?x=1 HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nUser-Agent: robco/0.0.0\r\n\
             Accept-Encoding: identity\r\nConnection: close\r\n\r\n",
            url.split(':').nth(2).unwrap().split('/').next().unwrap()
        )
    );
}

#[test]
fn a_chunked_body_is_read() {
    let pki = Pki::new();
    let h: Handler = Box::new(|_, w| {
        w.write_all(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2;x=y\r\nde\r\n0\r\n\r\n",
        )
    });
    let s = Server::start(&pki, vec![h]);
    assert_eq!(fetch(&client(&pki), &s.url("/c")).unwrap(), b"abcde");
    s.finish();
}

#[test]
fn a_redirect_to_the_same_server_and_to_another_is_followed() {
    let pki = Pki::new();
    let other = Server::start(&pki, vec![ok(b"far".to_vec())]);
    let far = other.url("/release/file.zip");
    let s = Server::start(
        &pki,
        vec![redirect(301, "/next".to_owned()), redirect(302, far)],
    );
    assert_eq!(fetch(&client(&pki), &s.url("/start")).unwrap(), b"far");
    let seen = s.finish();
    assert!(
        seen[1].request.starts_with("GET /next "),
        "{}",
        seen[1].request
    );
    let far_seen = other.finish();
    assert!(far_seen[0].request.starts_with("GET /release/file.zip "));
}

#[test]
fn a_redirect_to_http_is_refused_and_never_connected() {
    let pki = Pki::new();
    let plain = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = plain.local_addr().unwrap().port();
    for loc in [
        format!("http://127.0.0.1:{port}/x"),
        format!("HTTP://127.0.0.1:{port}/x"),
        format!("ftp://127.0.0.1:{port}/x"),
        "x.zip".to_owned(),
    ] {
        let s = Server::start(&pki, vec![redirect(307, loc.clone())]);
        let e = fetch(&client(&pki), &s.url("/")).unwrap_err();
        assert!(
            e.contains("refused") || e.contains("only https"),
            "{loc}: {e}"
        );
        s.finish();
    }
    plain.set_nonblocking(true).unwrap();
    assert_eq!(
        plain.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock,
        "the http server got a connection"
    );
}

#[test]
fn too_many_redirects_fail() {
    let pki = Pki::new();
    let handlers = (0..=MAX_REDIRECTS)
        .map(|i| redirect(302, format!("/r{i}")))
        .collect();
    let s = Server::start(&pki, handlers);
    let e = fetch(&client(&pki), &s.url("/")).unwrap_err();
    assert!(e.contains("more than 5 redirects"), "{e}");
    assert_eq!(s.finish().len(), MAX_REDIRECTS + 1);
}

#[test]
fn the_last_allowed_redirect_is_followed() {
    let pki = Pki::new();
    let mut handlers: Vec<Handler> = (0..MAX_REDIRECTS)
        .map(|i| redirect(302, format!("/r{i}")))
        .collect();
    handlers.push(ok(b"end".to_vec()));
    let s = Server::start(&pki, handlers);
    assert_eq!(fetch(&client(&pki), &s.url("/")).unwrap(), b"end");
    s.finish();
}

#[test]
fn a_certificate_from_another_authority_is_refused() {
    let server_pki = Pki::new();
    let s = Server::start(&server_pki, vec![ok(b"x".to_vec())]);
    let e = fetch(&client(&Pki::new()), &s.url("/")).unwrap_err();
    assert!(e.starts_with("nothing was fetched"), "{e}");
    assert!(e.contains("certificate"), "{e}");
    assert!(s.finish()[0].result.is_err());
}

#[test]
fn a_server_that_closes_early_fails() {
    let pki = Pki::new();
    let h: Handler =
        Box::new(|_, w| w.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n0123456789"));
    let s = Server::start(&pki, vec![h]);
    let mut out = Vec::new();
    let e = client(&pki).fetch(&s.url("/"), &mut out).unwrap_err();
    assert!(e.contains("after 10 of 100 bytes"), "{e}");
    assert_eq!(out, b"0123456789");
    s.finish();
}

#[test]
fn a_server_that_stalls_times_out() {
    let pki = Pki::new();
    let (tx, rx) = mpsc::channel::<()>();
    let h: Handler = Box::new(move |_, w| {
        w.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n01234")?;
        w.flush()?;
        // Holds the connection open until the test has its answer.
        let _ = rx.recv();
        Ok(())
    });
    let s = Server::start(&pki, vec![h]);
    let e = fetch(&client_with(&pki, Duration::from_millis(200)), &s.url("/")).unwrap_err();
    assert!(e.contains("no byte moved for 200 ms"), "{e}");
    drop(tx);
    s.finish();
}

#[test]
fn an_error_status_fails() {
    let pki = Pki::new();
    let h: Handler =
        Box::new(|_, w| w.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n"));
    let s = Server::start(&pki, vec![h]);
    let e = fetch(&client(&pki), &s.url("/")).unwrap_err();
    assert!(
        e.contains("status 404") && e.starts_with("nothing was fetched"),
        "{e}"
    );
    s.finish();
}

/// A writer that refuses more than `limit` bytes, as the installer's does.
struct Refusing {
    n: usize,
    limit: usize,
}

impl Write for Refusing {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.n + buf.len() > self.limit {
            return Err(io::Error::other("longer than the index says"));
        }
        self.n += buf.len();
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_refusing_writer_ends_the_transfer() {
    let pki = Pki::new();
    // The server offers a gigabyte and writes until the connection ends.
    let h: Handler = Box::new(|_, w| {
        w.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1073741824\r\n\r\n")?;
        let block = vec![7u8; 64 * 1024];
        for _ in 0..(1024 * 1024 * 1024 / block.len()) {
            w.write_all(&block)?;
        }
        Ok(())
    });
    let s = Server::start(&pki, vec![h]);
    let mut out = Refusing { n: 0, limit: 1000 };
    let e = client(&pki).fetch(&s.url("/"), &mut out).unwrap_err();
    assert!(e.contains("took no more bytes"), "{e}");
    // The server's writes failed: the client closed the connection.
    let seen = s.finish();
    assert!(seen[0].result.is_err(), "the server sent the whole body");
}

#[test]
fn an_index_longer_than_the_bound_is_refused() {
    let pki = Pki::new();
    let big = usize::try_from(MAX_INDEX_BYTES).unwrap() + 1;
    let s = Server::start(&pki, vec![ok(vec![b' '; big])]);
    let e = client(&pki).fetch_index(&s.url("/i.json")).unwrap_err();
    assert!(e.contains("longer than"), "{e}");
    s.finish();
    let s = Server::start(&pki, vec![ok(b"{}".to_vec())]);
    assert_eq!(client(&pki).fetch_index(&s.url("/i.json")).unwrap(), b"{}");
    s.finish();
}

#[test]
fn only_https_urls_are_fetched() {
    let h = client(&Pki::new());
    for url in ["http://127.0.0.1:1/x", "x.zip", "https://u:p@127.0.0.1:1/x"] {
        let e = fetch(&h, url).unwrap_err();
        assert!(e.starts_with("nothing was fetched"), "{e}");
        assert!(!e.contains("u:p"), "{e}");
    }
}

#[test]
fn an_error_text_holds_no_query_and_no_user_info() {
    assert_eq!(
        redact("https://u:secret@h:1/p/a.zip?sig=abc#f"),
        "https://h:1/p/a.zip"
    );
    let pki = Pki::new();
    let h: Handler = Box::new(|_, w| {
        w.write_all(b"HTTP/1.1 500 Oops\r\nSet-Cookie: s=1\r\nContent-Length: 0\r\n\r\n")
    });
    let s = Server::start(&pki, vec![h]);
    let e = fetch(&client(&pki), &s.url("/a?token=secret")).unwrap_err();
    assert!(!e.contains("secret") && !e.contains("Cookie"), "{e}");
    s.finish();
}

#[test]
fn urls_split_as_expected() {
    let t = Target::parse("https://h.example:8443/a/b?q#f").unwrap();
    assert_eq!(
        (
            t.host.as_str(),
            t.port,
            t.authority.as_str(),
            t.path.as_str()
        ),
        ("h.example", 8443, "h.example:8443", "/a/b?q")
    );
    let t = Target::parse("https://[::1]/x").unwrap();
    assert_eq!((t.host.as_str(), t.port), ("::1", 443));
    assert_eq!(Target::parse("https://h?q").unwrap().path, "/?q");
    for bad in [
        "https://",
        "https://h:0/",
        "https://h:x/",
        "https://h /",
        "https://[::1/",
    ] {
        assert!(Target::parse(bad).is_err(), "{bad}");
    }
    let base = Target::parse("https://a/b/c").unwrap();
    assert_eq!(base.follow("//d/e").unwrap().authority, "d");
    assert_eq!(base.follow("/e").unwrap().path, "/e");
}

fn answer(bytes: &'static [u8]) -> Result<Vec<u8>, String> {
    let pki = Pki::new();
    let s = Server::start(&pki, vec![raw(bytes)]);
    let got = fetch(&client(&pki), &s.url("/"));
    s.finish();
    got
}

fn refused(bytes: &'static [u8], rule: &str) {
    let e = answer(bytes).unwrap_err();
    assert!(e.contains(rule), "wanted {rule:?}, got {e}");
}

#[test]
fn informational_answers_are_read_past() {
    let got = answer(
        b"HTTP/1.1 103 Early Hints\r\nLink: </a>; rel=preload\r\n\r\n\
          HTTP/1.1 100 Continue\r\n\r\n\
          HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
    );
    assert_eq!(got.unwrap(), b"ok");
}

#[test]
fn too_many_informational_answers_fail() {
    refused(
        b"HTTP/1.1 103 A\r\n\r\nHTTP/1.1 103 B\r\n\r\nHTTP/1.1 103 C\r\n\r\n\
          HTTP/1.1 103 D\r\n\r\nHTTP/1.1 103 E\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n",
        "more than 4 informational answers",
    );
}

#[test]
fn a_protocol_switch_fails() {
    refused(
        b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: h2c\r\n\r\n",
        "status 101",
    );
}
