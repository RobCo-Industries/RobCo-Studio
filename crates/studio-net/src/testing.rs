// SPDX-License-Identifier: GPL-3.0-or-later

//! For tests only: a TLS server on 127.0.0.1 with a certificate the test
//! makes. Built with the `test-trust` feature, which only dev-dependencies
//! turn on. Nothing here touches the network.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test support; a failed unwrap is a failed test"
)]

use std::io::{self, Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread::JoinHandle;

use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};

/// A certificate authority made for one test, and a server certificate
/// for `127.0.0.1` that it signed.
pub struct Pki {
    /// The authority's certificate, in DER: what a client trusts.
    pub ca_der: Vec<u8>,
    leaf_der: Vec<u8>,
    leaf_key: Vec<u8>,
}

impl Pki {
    /// A new authority and server certificate.
    pub fn new() -> Self {
        let mut ca_params = CertificateParams::new(Vec::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = CertifiedIssuer::self_signed(ca_params, KeyPair::generate().unwrap()).unwrap();
        let leaf_key = KeyPair::generate().unwrap();
        let leaf = CertificateParams::new(vec!["127.0.0.1".to_owned()])
            .unwrap()
            .signed_by(&leaf_key, &ca)
            .unwrap();
        Self {
            ca_der: ca.der().to_vec(),
            leaf_der: leaf.der().to_vec(),
            leaf_key: leaf_key.serialize_der(),
        }
    }
}

impl Default for Pki {
    fn default() -> Self {
        Self::new()
    }
}

/// Answers one connection: gets the request head as text and writes the
/// raw answer.
pub type Handler = Box<dyn FnOnce(&str, &mut dyn Write) -> io::Result<()> + Send>;

/// What one connection saw.
#[derive(Debug)]
pub struct Served {
    /// The request head, up to the empty line. Empty when the handshake
    /// failed.
    pub request: String,
    /// The handler's result, or the handshake's error.
    pub result: io::Result<()>,
}

/// A TLS server that answers one connection per handler, in order.
pub struct Server {
    /// The port on 127.0.0.1.
    pub port: u16,
    join: JoinHandle<Vec<Served>>,
}

impl Server {
    /// Starts the server in a thread.
    pub fn start(pki: &Pki, handlers: Vec<Handler>) -> Self {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(pki.leaf_der.clone())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(pki.leaf_key.clone())),
            )
            .unwrap();
        let config = Arc::new(config);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let join = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for h in handlers {
                let (tcp, _) = listener.accept().unwrap();
                let conn = ServerConnection::new(config.clone()).unwrap();
                let mut s = StreamOwned::new(conn, tcp);
                let request = match read_request(&mut s) {
                    Ok(r) => r,
                    Err(e) => {
                        seen.push(Served {
                            request: String::new(),
                            result: Err(e),
                        });
                        continue;
                    }
                };
                let result = h(&request, &mut s);
                s.conn.send_close_notify();
                let _ = s.flush();
                seen.push(Served { request, result });
            }
            seen
        });
        Self { port, join }
    }

    /// A URL on this server.
    pub fn url(&self, path: &str) -> String {
        format!("https://127.0.0.1:{}{path}", self.port)
    }

    /// Waits for every connection to be served and returns what each saw.
    pub fn finish(self) -> Vec<Served> {
        self.join.join().unwrap()
    }
}

fn read_request(s: &mut impl Read) -> io::Result<String> {
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if s.read(&mut b)? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        head.push(b[0]);
    }
    Ok(String::from_utf8_lossy(&head).into_owned())
}

/// A handler that answers 200 with this body and its length.
pub fn ok(body: Vec<u8>) -> Handler {
    Box::new(move |_, w| {
        write!(
            w,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )?;
        w.write_all(&body)
    })
}

/// A handler that writes exactly these bytes.
pub fn raw(bytes: &'static [u8]) -> Handler {
    Box::new(move |_, w| w.write_all(bytes))
}

/// A handler that redirects with this status and `Location`.
pub fn redirect(status: u16, location: String) -> Handler {
    Box::new(move |_, w| {
        write!(
            w,
            "HTTP/1.1 {status} Moved\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n"
        )
    })
}
