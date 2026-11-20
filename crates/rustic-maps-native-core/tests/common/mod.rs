//! Shared test infrastructure for the native test targets.
//!
//! Every native test answers its provider requests from committed fixtures.
//! This module provides the one loopback HTTP server those tests share: it
//! binds an ephemeral address on the IPv4 loopback interface, serves a
//! scripted list of responses, counts the requests it answered, and exposes
//! the requests it received (method, request target, headers and body) so a
//! test can assert on them.
//!
//! Tests never touch the network. The server binds `127.0.0.1:0`, so the
//! kernel assigns an unused ephemeral port and every request stays on this
//! machine; payloads are read from committed fixture files, never fetched.
//!
//! A scripted response answers every request unless it declares a request
//! target fragment. A fragment-constrained response answers only requests
//! whose target contains that fragment, which lets one server answer several
//! payloads by path. Responses without a fragment are served in declaration
//! order, one per request, and the last one repeats. A request no scripted
//! response matches is answered with `404 Not Found` and recorded, so a
//! mismatched path fails a test instead of stalling it.
#![allow(dead_code)] // Each test target compiles this module; no target uses every helper.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

/// The media type committed JSON payloads are served as.
pub const JSON_CONTENT_TYPE: &str = "application/json";

/// The loopback bind address. Port zero asks the operating system for an
/// unused ephemeral port, so tests can run in parallel.
const LOOPBACK_BIND_ADDRESS: &str = "127.0.0.1:0";

/// HTTP status of a successful scripted response.
const HTTP_OK_STATUS: u16 = 200;
/// HTTP status of the committed unauthorized payloads.
const HTTP_UNAUTHORIZED_STATUS: u16 = 401;
/// HTTP status of the response to a request no scripted response matches.
const HTTP_NOT_FOUND_STATUS: u16 = 404;
/// HTTP status of the committed rate-limited payloads.
const HTTP_RATE_LIMITED_STATUS: u16 = 429;

/// The body the server answers a request no scripted response matches with.
const UNMATCHED_RESPONSE_BODY: &[u8] = b"{}";

/// The request-line separator between the three request-line tokens.
const REQUEST_LINE_SEPARATOR: &str = " ";

/// The line ending every response head uses.
const HEADER_LINE_ENDING: &str = "\r\n";

/// Reads one committed fixture, with `relative_path` resolved from the
/// repository root.
///
/// Panics when the file is absent or unreadable, so the test that needs it
/// fails with the path it expected instead of serving an empty payload.
pub fn fixture_bytes(relative_path: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative_path);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{} is checked in: {error}", path.display()))
}

/// One response the fixture server can serve.
///
/// A response always carries a status, a content type and a body. Extra
/// headers cover what a test needs to assert on, such as a retry hint. A
/// response constrained to a request target fragment answers only requests
/// whose target contains that fragment; an unconstrained response answers in
/// declaration order.
#[derive(Debug)]
pub struct ScriptedResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    path_fragment: Option<String>,
}

impl ScriptedResponse {
    /// A response carrying a JSON document.
    pub fn json(status: u16, body: Vec<u8>) -> Self {
        Self::with_content_type(status, JSON_CONTENT_TYPE, body)
    }

    /// A response carrying a document of the given media type.
    pub fn with_content_type(status: u16, content_type: &str, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".to_string(), content_type.to_string())],
            body,
            path_fragment: None,
        }
    }

    /// Adds one response header, such as a retry hint.
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// Answers only requests whose target contains `fragment`.
    pub fn for_path_fragment(mut self, fragment: &str) -> Self {
        self.path_fragment = Some(fragment.to_string());
        self
    }
}

/// One request the fixture server received.
///
/// The request target carries the path and its query string, so an assertion
/// can prove which credential the vendor client applied. The headers and the
/// body are kept verbatim for the same reason.
#[derive(Clone, Debug)]
pub struct ReceivedRequest {
    request_line: String,
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl ReceivedRequest {
    /// The method the request used.
    pub fn method(&self) -> &str {
        &self.method
    }

    /// The request target: the path and query the client asked for.
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Every header the request carried, in arrival order.
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    /// The value of the first header named `name`, compared case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The request body, empty when the request carried none.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// The request line, header block and body joined, for assertions that
    /// only need to prove a value reached the vendor.
    pub fn head_text(&self) -> String {
        let mut text = format!("{}{HEADER_LINE_ENDING}", self.request_line);
        for (name, value) in &self.headers {
            text.push_str(name);
            text.push_str(": ");
            text.push_str(value);
            text.push_str(HEADER_LINE_ENDING);
        }
        text.push_str(HEADER_LINE_ENDING);
        text.push_str(&String::from_utf8_lossy(&self.body));
        text
    }
}

/// A loopback HTTP server serving a scripted list of responses.
///
/// The server runs on its own thread for the test's lifetime. It counts every
/// request it answers and records each request, so a test can assert how many
/// requests reached it and what they carried.
pub struct LoopbackFixtureServer {
    base_url: String,
    request_count: Arc<Mutex<usize>>,
    received_requests: Arc<Mutex<Vec<ReceivedRequest>>>,
    unmatched_requests: Arc<Mutex<Vec<String>>>,
}

impl LoopbackFixtureServer {
    /// Binds an ephemeral loopback port and starts serving `responses`.
    pub fn start(responses: Vec<ScriptedResponse>) -> Self {
        let listener = TcpListener::bind(LOOPBACK_BIND_ADDRESS).expect("the loopback port binds");
        let port = listener.local_addr().expect("the port is readable").port();
        let request_count = Arc::new(Mutex::new(0));
        let received_requests = Arc::new(Mutex::new(Vec::new()));
        let unmatched_requests = Arc::new(Mutex::new(Vec::new()));
        let served_count = Arc::clone(&request_count);
        let recorded_requests = Arc::clone(&received_requests);
        let recorded_unmatched = Arc::clone(&unmatched_requests);
        thread::spawn(move || {
            let mut sequence_index = 0usize;
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                serve_one(
                    stream,
                    &responses,
                    &mut sequence_index,
                    &served_count,
                    &recorded_requests,
                    &recorded_unmatched,
                );
            }
        });
        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            request_count,
            received_requests,
            unmatched_requests,
        }
    }

    /// The origin a test points its provider at.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The number of requests the server has answered.
    pub fn request_count(&self) -> usize {
        *self
            .request_count
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Every request the server received, in arrival order.
    pub fn received_requests(&self) -> Vec<ReceivedRequest> {
        self.received_requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Every received request's head text joined, for assertions that only
    /// need to prove a value reached the vendor.
    pub fn received_requests_text(&self) -> String {
        self.received_requests()
            .iter()
            .map(ReceivedRequest::head_text)
            .collect::<Vec<String>>()
            .join("\n")
    }

    /// Every received request's target, in arrival order.
    pub fn request_targets(&self) -> Vec<String> {
        self.received_requests()
            .iter()
            .map(|request| request.target().to_string())
            .collect()
    }

    /// The request lines no scripted response matched.
    pub fn unmatched_requests(&self) -> Vec<String> {
        self.unmatched_requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

/// One response selected for the request being served.
struct ServedResponse<'a> {
    status: u16,
    headers: &'a [(String, String)],
    body: &'a [u8],
}

/// Serves one connection: reads the request, selects its response, then
/// writes the response and closes.
fn serve_one(
    mut stream: TcpStream,
    responses: &[ScriptedResponse],
    sequence_index: &mut usize,
    request_count: &Arc<Mutex<usize>>,
    received_requests: &Arc<Mutex<Vec<ReceivedRequest>>>,
    unmatched_requests: &Arc<Mutex<Vec<String>>>,
) {
    let request = read_request(&stream);
    *request_count
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) += 1;
    received_requests
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(request.clone());

    let unmatched_headers = vec![("Content-Type".to_string(), JSON_CONTENT_TYPE.to_string())];
    let response = match select_response(responses, request.target(), sequence_index) {
        Some(response) => ServedResponse {
            status: response.status,
            headers: &response.headers,
            body: &response.body,
        },
        None => {
            unmatched_requests
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(request.request_line.clone());
            ServedResponse {
                status: HTTP_NOT_FOUND_STATUS,
                headers: &unmatched_headers,
                body: UNMATCHED_RESPONSE_BODY,
            }
        }
    };
    write_response(&mut stream, response);
}

/// Selects the response for a request target.
///
/// A response constrained to a target fragment wins over an unconstrained one.
/// Unconstrained responses are consumed in declaration order and the last one
/// repeats.
fn select_response<'a>(
    responses: &'a [ScriptedResponse],
    request_target: &str,
    sequence_index: &mut usize,
) -> Option<&'a ScriptedResponse> {
    if let Some(response) = responses.iter().find(|response| {
        response
            .path_fragment
            .as_deref()
            .is_some_and(|fragment| request_target.contains(fragment))
    }) {
        return Some(response);
    }

    let unconstrained_count = responses
        .iter()
        .filter(|response| response.path_fragment.is_none())
        .count();
    if unconstrained_count == 0 {
        return None;
    }
    let selected_index = (*sequence_index).min(unconstrained_count - 1);
    *sequence_index += 1;
    responses
        .iter()
        .filter(|response| response.path_fragment.is_none())
        .nth(selected_index)
}

/// Reads one request: the request line, its header block and any body.
fn read_request(stream: &TcpStream) -> ReceivedRequest {
    let mut reader = BufReader::new(stream.try_clone().expect("the request stream clones"));
    let mut request_line = String::new();
    let _ = reader.read_line(&mut request_line);
    let request_line = request_line.trim_end().to_string();
    let mut request_line_tokens = request_line.split(REQUEST_LINE_SEPARATOR);
    let method = request_line_tokens.next().unwrap_or_default().to_string();
    let target = request_line_tokens.next().unwrap_or_default().to_string();

    let mut headers = Vec::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        let Ok(bytes_read) = reader.read_line(&mut line) else {
            break;
        };
        if bytes_read == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_string();
        let value = value.trim().to_string();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.parse().unwrap_or(0);
        }
        headers.push((name, value));
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        let _ = reader.read_exact(&mut body);
    }
    ReceivedRequest {
        request_line,
        method,
        target,
        headers,
        body,
    }
}

/// Writes one response head and body, then closes the connection.
fn write_response(stream: &mut TcpStream, response: ServedResponse<'_>) {
    let mut head = format!(
        "HTTP/1.1 {} {}{HEADER_LINE_ENDING}",
        response.status,
        reason_phrase(response.status)
    );
    for (name, value) in response.headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str(HEADER_LINE_ENDING);
    }
    head.push_str("Content-Length: ");
    head.push_str(&response.body.len().to_string());
    head.push_str(HEADER_LINE_ENDING);
    head.push_str("Connection: close");
    head.push_str(HEADER_LINE_ENDING);
    head.push_str(HEADER_LINE_ENDING);
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(response.body);
}

/// The reason phrase for a status the fixture server serves.
fn reason_phrase(status: u16) -> &'static str {
    match status {
        HTTP_OK_STATUS => "OK",
        HTTP_UNAUTHORIZED_STATUS => "Unauthorized",
        HTTP_NOT_FOUND_STATUS => "Not Found",
        HTTP_RATE_LIMITED_STATUS => "Too Many Requests",
        _ => "Unknown",
    }
}
