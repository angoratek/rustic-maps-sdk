//! A small HTTP/1.1 server for a built walkthrough directory.
//!
//! It serves only `GET` and `HEAD`, resolves paths beneath one directory, and
//! injects the authoring overlay into the player document at request time. The
//! built directory is never modified, so the overlay cannot reach a published
//! build.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::build::DEFAULT_OUTPUT_DIRECTORY;
use crate::error::WalkthroughError;
use crate::site::{AUTHORING_SCRIPT, AUTHORING_SCRIPT_ROUTE};

/// The default address the local server binds to.
pub const DEFAULT_SERVE_ADDRESS: ([u8; 4], u16) = ([127, 0, 0, 1], 8080);

/// The pause between polls while waiting for a connection.
pub const SERVER_POLL_INTERVAL_MILLISECONDS: u64 = 10;

const STATUS_OK: &str = "200 OK";
const STATUS_BAD_REQUEST: &str = "400 Bad Request";
const STATUS_NOT_FOUND: &str = "404 Not Found";
const STATUS_METHOD_NOT_ALLOWED: &str = "405 Method Not Allowed";

const CONTENT_TYPE_HTML: &str = "text/html; charset=utf-8";
const CONTENT_TYPE_CSS: &str = "text/css; charset=utf-8";
const CONTENT_TYPE_JAVASCRIPT: &str = "text/javascript; charset=utf-8";
const CONTENT_TYPE_JSON: &str = "application/json; charset=utf-8";
const CONTENT_TYPE_PNG: &str = "image/png";
const CONTENT_TYPE_MP4: &str = "video/mp4";
const CONTENT_TYPE_WEBM: &str = "video/webm";
const CONTENT_TYPE_SVG: &str = "image/svg+xml";
const CONTENT_TYPE_OCTET_STREAM: &str = "application/octet-stream";

const AUTHORING_SCRIPT_ELEMENT: &str = "<script src=\"/authoring.js\" defer></script>";
const INDEX_FILE_NAME: &str = "index.html";
const BODY_END_TAG: &str = "</body>";

/// The options for [`serve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeOptions {
    /// The built directory to serve.
    pub directory: PathBuf,
    /// The address to bind.
    pub address: SocketAddr,
    /// Whether to inject the hotspot authoring overlay.
    pub authoring_enabled: bool,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            directory: PathBuf::from(DEFAULT_OUTPUT_DIRECTORY),
            address: SocketAddr::from(DEFAULT_SERVE_ADDRESS),
            authoring_enabled: true,
        }
    }
}

/// Binds the local server socket.
///
/// # Errors
/// Returns [`WalkthroughError::Network`] when the address cannot be bound.
pub fn bind(address: SocketAddr) -> Result<TcpListener, WalkthroughError> {
    TcpListener::bind(address).map_err(network_error)
}

/// Serves `options.directory` until the process stops.
///
/// # Errors
/// Returns [`WalkthroughError::Network`] when the socket cannot be bound or a
/// connection fails.
pub fn serve(options: &ServeOptions) -> Result<(), WalkthroughError> {
    let listener = bind(options.address)?;
    serve_on(
        listener,
        options.directory.clone(),
        options.authoring_enabled,
        Arc::new(AtomicBool::new(false)),
    )
}

/// Serves `options.directory` until `shutdown` becomes true.
///
/// # Errors
/// Returns [`WalkthroughError::Network`] when the socket cannot be bound or a
/// connection fails.
pub fn serve_with_shutdown(
    options: &ServeOptions,
    shutdown: Arc<AtomicBool>,
) -> Result<(), WalkthroughError> {
    let listener = bind(options.address)?;
    serve_on(
        listener,
        options.directory.clone(),
        options.authoring_enabled,
        shutdown,
    )
}

/// Serves `directory` on an already bound listener until `shutdown` is set.
///
/// # Errors
/// Returns [`WalkthroughError::Network`] when the listener cannot be polled or
/// a connection fails.
pub fn serve_on(
    listener: TcpListener,
    directory: PathBuf,
    authoring_enabled: bool,
    shutdown: Arc<AtomicBool>,
) -> Result<(), WalkthroughError> {
    listener.set_nonblocking(true).map_err(network_error)?;
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false).map_err(network_error)?;
                handle_connection(&mut stream, &directory, authoring_enabled)?;
            }
            Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(SERVER_POLL_INTERVAL_MILLISECONDS));
            }
            Err(source) => return Err(network_error(source)),
        }
    }
    Ok(())
}

fn handle_connection(
    stream: &mut TcpStream,
    directory: &Path,
    authoring_enabled: bool,
) -> Result<(), WalkthroughError> {
    let reader_stream = stream.try_clone().map_err(network_error)?;
    let mut reader = BufReader::new(reader_stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).map_err(network_error)?;
    drain_headers(&mut reader)?;

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let is_head = method == "HEAD";
    if method != "GET" && !is_head {
        return write_response(
            stream,
            STATUS_METHOD_NOT_ALLOWED,
            CONTENT_TYPE_HTML,
            b"method not allowed",
            false,
        );
    }

    let Some(request_path) = decode_request_path(target) else {
        return write_response(
            stream,
            STATUS_BAD_REQUEST,
            CONTENT_TYPE_HTML,
            b"bad request",
            is_head,
        );
    };

    if authoring_enabled && request_path == AUTHORING_SCRIPT_ROUTE {
        return write_response(
            stream,
            STATUS_OK,
            CONTENT_TYPE_JAVASCRIPT,
            AUTHORING_SCRIPT.as_bytes(),
            is_head,
        );
    }

    let Some(file_path) = resolve_file_path(directory, &request_path) else {
        return write_response(
            stream,
            STATUS_NOT_FOUND,
            CONTENT_TYPE_HTML,
            b"not found",
            is_head,
        );
    };
    let is_index = file_path.file_name().and_then(|name| name.to_str()) == Some(INDEX_FILE_NAME);
    let bytes = std::fs::read(&file_path).map_err(|source| WalkthroughError::Read {
        path: file_path.clone(),
        source,
    })?;
    let body = if is_index && authoring_enabled {
        inject_authoring_overlay(&bytes)
    } else {
        bytes
    };
    write_response(
        stream,
        STATUS_OK,
        content_type_for(&file_path),
        &body,
        is_head,
    )
}

fn drain_headers(reader: &mut BufReader<TcpStream>) -> Result<(), WalkthroughError> {
    loop {
        let mut header_line = String::new();
        let read = reader.read_line(&mut header_line).map_err(network_error)?;
        if read == 0 || header_line == "\r\n" || header_line == "\n" {
            return Ok(());
        }
    }
}

fn decode_request_path(target: &str) -> Option<String> {
    let path = target.split('?').next().unwrap_or_default();
    let decoded = percent_decode(path);
    if decoded.contains('\0') || decoded.contains('\\') {
        return None;
    }
    Some(decoded)
}

fn resolve_file_path(directory: &Path, request_path: &str) -> Option<PathBuf> {
    let mut resolved = directory.to_path_buf();
    for component in request_path.split('/') {
        match component {
            "" | "." => {}
            ".." => return None,
            other => resolved.push(other),
        }
    }
    if resolved.is_dir() {
        resolved.push(INDEX_FILE_NAME);
    }
    resolved.is_file().then_some(resolved)
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let is_escape = bytes[index] == b'%' && index + 2 < bytes.len();
        if is_escape
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            decoded.push(high * 16 + low);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn inject_authoring_overlay(index_bytes: &[u8]) -> Vec<u8> {
    let document = String::from_utf8_lossy(index_bytes);
    let injection = format!("{AUTHORING_SCRIPT_ELEMENT}\n{BODY_END_TAG}");
    if document.contains(BODY_END_TAG) {
        document.replacen(BODY_END_TAG, &injection, 1).into_bytes()
    } else {
        let mut appended = document.into_owned();
        appended.push_str(AUTHORING_SCRIPT_ELEMENT);
        appended.into_bytes()
    }
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
    is_head: bool,
) -> Result<(), WalkthroughError> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .and_then(|()| {
            if is_head {
                Ok(())
            } else {
                stream.write_all(body)
            }
        })
        .and_then(|()| stream.flush())
        .map_err(network_error)
}

fn content_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("html") => CONTENT_TYPE_HTML,
        Some("css") => CONTENT_TYPE_CSS,
        Some("js") => CONTENT_TYPE_JAVASCRIPT,
        Some("json") => CONTENT_TYPE_JSON,
        Some("png") => CONTENT_TYPE_PNG,
        Some("mp4") => CONTENT_TYPE_MP4,
        Some("webm") => CONTENT_TYPE_WEBM,
        Some("svg") => CONTENT_TYPE_SVG,
        _ => CONTENT_TYPE_OCTET_STREAM,
    }
}

fn network_error(source: std::io::Error) -> WalkthroughError {
    WalkthroughError::Network { source }
}
