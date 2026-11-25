//! MCP (Model Context Protocol) support: AI assistants can look at and work in AudioCraft.
//!
//! The running app serves MCP over HTTP on this computer only (`http://127.0.0.1:PORT/mcp`,
//! JSON-RPC 2.0, the "Streamable HTTP" transport answering with plain JSON). Assistants that
//! start a program instead connect through `audiocraft --mcp-stdio`, a small bridge that
//! forwards each line of stdin to the running app and prints the answers.
//!
//! Protocol: MCP 2025-11-25 (also 2025-06-18, 2025-03-26 and 2024-11-05, whichever the
//! assistant asks for). `initialize` gives an `Mcp-Session-Id`; later requests carry it and the
//! `MCP-Protocol-Version` header (an unknown session is 404, an unknown version 400, and DELETE
//! ends a session). Batches are only accepted from 2025-03-26 and older clients. Tools carry a
//! title, behaviour hints (read-only, destructive, idempotent) and, for the ones that report
//! data, an output schema with the data as `structuredContent` beside the text.
//!
//! Guardrails (Preferences > Control Surface > MCP):
//! - off until the person turns it on; only this computer can connect (127.0.0.1, Host and
//!   Origin checked against browser tricks), and every request needs the secret access token;
//! - four permission levels: Read only, Navigate & play, Edit (every edit is undoable), Full
//!   (also saving); commands that quit, open a dialog, launch other programs or scan plug-ins
//!   are never available;
//! - "Ask me before each change": an edit waits for the person's Allow / Deny in the app
//!   (declined after a minute without an answer);
//! - a requests-per-minute limit, size limits on requests, and an activity log of every call.

use crate::app::App;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// The largest request body accepted.
const MAX_BODY: usize = 1 << 20;
/// The largest request head (request line and headers) accepted.
const MAX_HEAD: usize = 16 * 1024;
/// Connections served at the same time.
const MAX_CONNECTIONS: usize = 8;
/// How long a change may wait for the person's answer.
pub const CONFIRM_TIMEOUT: Duration = Duration::from_secs(60);
/// The MCP protocol versions spoken, newest first.
pub const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
/// The version assumed for an HTTP request with neither a version header nor a session (the
/// rule the 2025-06-18 transport gives).
const ASSUMED_VERSION: &str = "2025-03-26";
/// The first version without JSON-RPC batches.
const NO_BATCHES_FROM: &str = "2025-06-18";
/// Sessions remembered at the same time (the oldest is forgotten first).
const MAX_SESSIONS: usize = 64;
/// The tools AudioCraft offers.
const TOOL_NAMES: [&str; 7] = [
    "get_state",
    "list_tracks",
    "list_commands",
    "run_command",
    "set_playhead",
    "set_selection",
    "set_track",
];

/// The sessions given out at `initialize`: (session id, protocol version), oldest first.
type Sessions = Arc<Mutex<VecDeque<(String, String)>>>;

fn session_version(sessions: &Sessions, sid: &str) -> Option<String> {
    let list = sessions.lock().ok()?;
    list.iter().find(|(s, _)| same_secret(s, sid)).map(|(_, v)| v.clone())
}

fn add_session(sessions: &Sessions, sid: String, version: &str) {
    if let Ok(mut list) = sessions.lock() {
        list.push_back((sid, version.to_string()));
        while list.len() > MAX_SESSIONS {
            list.pop_front();
        }
    }
}

fn end_session(sessions: &Sessions, sid: &str) -> bool {
    let Ok(mut list) = sessions.lock() else {
        return false;
    };
    let before = list.len();
    list.retain(|(s, _)| !same_secret(s, sid));
    list.len() != before
}

/// The permission levels, lowest first.
pub const LEVELS: [(&str, &str); 4] = [
    ("Read only", "Assistants can look at the session, tracks and commands, and change nothing."),
    (
        "Navigate & play",
        "Also move the playhead and selection, zoom and scroll, and play or stop.",
    ),
    (
        "Edit",
        "Also edit the session and files (each change can be undone with Ctrl+Z).",
    ),
    ("Full", "Also save sessions and files that already have a place on disk."),
];

/// One JSON-RPC message (or batch) from a client, with where to send the answer.
pub struct McpRequest {
    pub msg: Value,
    /// `None`: nothing to answer (only notifications).
    pub reply: mpsc::Sender<Option<Value>>,
}

/// The HTTP server thread; dropping it stops it.
pub struct McpServer {
    pub port: u16,
    stop: Arc<AtomicBool>,
    pub rx: mpsc::Receiver<McpRequest>,
}

impl Drop for McpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl McpServer {
    /// Listen on 127.0.0.1:`port` (0: any free port). `ctx` wakes the app when a request
    /// comes in.
    pub fn start(port: u16, token: String, ctx: Option<egui::Context>) -> Result<McpServer, String> {
        if token.len() < 16 {
            return Err("the access token is missing".into());
        }
        let listener = TcpListener::bind(("127.0.0.1", port))
            .map_err(|e| format!("Could not listen on port {port}: {e}"))?;
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("Could not listen on port {port}: {e}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let stop2 = stop.clone();
        let active = Arc::new(AtomicUsize::new(0));
        let token = Arc::new(token);
        let sessions: Sessions = Arc::default();
        std::thread::Builder::new()
            .name("mcp-server".into())
            .spawn(move || {
                while !stop2.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, addr)) => {
                            if !addr.ip().is_loopback()
                                || active.load(Ordering::Relaxed) >= MAX_CONNECTIONS
                            {
                                continue;
                            }
                            active.fetch_add(1, Ordering::Relaxed);
                            let (tx, token, ctx, active, sessions) = (
                                tx.clone(),
                                token.clone(),
                                ctx.clone(),
                                active.clone(),
                                sessions.clone(),
                            );
                            let _ = std::thread::Builder::new()
                                .name("mcp-conn".into())
                                .spawn(move || {
                                    serve(stream, &token, port, &tx, ctx.as_ref(), &sessions);
                                    active.fetch_sub(1, Ordering::Relaxed);
                                });
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(25)),
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(McpServer { port, stop, rx })
    }
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &str, extra: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n{extra}\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// Compare without leaking how much matched through timing.
fn same_secret(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Is this Host / Origin header this computer (not a web page posing as it)?
fn local_name(v: &str, port: u16, origin: bool) -> bool {
    let v = v.trim().to_ascii_lowercase();
    let v = if origin {
        match v.strip_prefix("http://") {
            Some(r) => r.to_string(),
            None => return false,
        }
    } else {
        v
    };
    ["127.0.0.1", "localhost", "[::1]"].iter().any(|h| {
        v == *h || v == format!("{h}:{port}")
    })
}

/// Answer one HTTP connection: checks, then hand the JSON-RPC message to the app and wait.
fn serve(
    mut stream: TcpStream,
    token: &str,
    port: u16,
    tx: &mpsc::Sender<McpRequest>,
    ctx: Option<&egui::Context>,
    sessions: &Sessions,
) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(read_half);
    let mut head = 0usize;
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    head += line.len();
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut headers: Vec<(String, String)> = vec![];
    loop {
        let mut h = String::new();
        match reader.read_line(&mut h) {
            Ok(0) | Err(_) => return,
            Ok(n) => head += n,
        }
        if head > MAX_HEAD {
            return respond(&mut stream, "431 Request Header Fields Too Large", "text/plain", "", "");
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let header = |k: &str| headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    // only this computer: a web page in a browser cannot pose as it
    if header("host").is_some_and(|h| !local_name(h, port, false)) {
        return respond(&mut stream, "403 Forbidden", "text/plain", "host not allowed", "");
    }
    if header("origin").is_some_and(|o| o != "null" && !local_name(o, port, true)) {
        return respond(&mut stream, "403 Forbidden", "text/plain", "origin not allowed", "");
    }
    if path.split('?').next() != Some("/mcp") {
        return respond(&mut stream, "404 Not Found", "text/plain", "AudioCraft MCP lives at /mcp", "");
    }
    // no server-sent event stream (GET): AudioCraft never sends requests of its own
    if method != "POST" && method != "DELETE" {
        return respond(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain",
            "",
            "Allow: POST, DELETE\r\n",
        );
    }
    let auth_ok = header("authorization")
        .and_then(|a| a.strip_prefix("Bearer "))
        .is_some_and(|t| same_secret(t.trim(), token));
    if !auth_ok {
        return respond(
            &mut stream,
            "401 Unauthorized",
            "text/plain",
            "missing or wrong access token (Preferences > Control Surface > MCP)",
            "WWW-Authenticate: Bearer\r\n",
        );
    }
    // DELETE ends a session
    if method == "DELETE" {
        return match header("mcp-session-id") {
            Some(sid) if end_session(sessions, sid) => respond(&mut stream, "200 OK", "text/plain", "", ""),
            Some(_) => respond(&mut stream, "404 Not Found", "text/plain", "unknown session", ""),
            None => respond(&mut stream, "400 Bad Request", "text/plain", "Mcp-Session-Id is missing", ""),
        };
    }
    let len: usize = header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if len == 0 {
        return respond(&mut stream, "411 Length Required", "text/plain", "", "");
    }
    if len > MAX_BODY {
        return respond(&mut stream, "413 Payload Too Large", "text/plain", "", "");
    }
    let mut body = vec![0u8; len];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    // (checked once the body is read: refusing before that can reset the connection on
    // Windows, and the client would lose the answer)
    let header_version = header("mcp-protocol-version").map(str::to_string);
    if let Some(v) = &header_version {
        if !PROTOCOL_VERSIONS.contains(&v.as_str()) {
            let err = rpc_error(
                &Value::Null,
                -32600,
                &format!(
                    "unsupported MCP-Protocol-Version {v}; AudioCraft speaks {}",
                    PROTOCOL_VERSIONS.join(", ")
                ),
            );
            return respond(&mut stream, "400 Bad Request", "application/json", &err.to_string(), "");
        }
    }
    if header("content-type").is_some_and(|c| !c.to_ascii_lowercase().starts_with("application/json")) {
        return respond(&mut stream, "415 Unsupported Media Type", "text/plain", "send application/json", "");
    }
    let msg: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            let err = rpc_error(&Value::Null, -32700, &format!("parse error: {e}"));
            return respond(&mut stream, "400 Bad Request", "application/json", &err.to_string(), "");
        }
    };
    let is_init = msg.get("method").and_then(|m| m.as_str()) == Some("initialize");
    // a session id from an earlier run of AudioCraft (or one that was ended) is unknown: the
    // assistant starts again with initialize; requests without one are still served
    let session_v = match header("mcp-session-id") {
        Some(sid) if !is_init => match session_version(sessions, sid) {
            Some(v) => Some(v),
            None => {
                let err = rpc_error(&Value::Null, -32001, "unknown or ended session: initialize again");
                return respond(&mut stream, "404 Not Found", "application/json", &err.to_string(), "");
            }
        },
        _ => None,
    };
    let version = header_version
        .or(session_v)
        .unwrap_or_else(|| ASSUMED_VERSION.to_string());
    if msg.is_array() && version.as_str() >= NO_BATCHES_FROM {
        let err = rpc_error(
            &Value::Null,
            -32600,
            "batches are not part of MCP 2025-06-18 and later: send one message per request",
        );
        return respond(&mut stream, "400 Bad Request", "application/json", &err.to_string(), "");
    }
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let (rtx, rrx) = mpsc::channel();
    if tx.send(McpRequest { msg, reply: rtx }).is_err() {
        return respond(&mut stream, "503 Service Unavailable", "text/plain", "", "");
    }
    if let Some(ctx) = ctx {
        ctx.request_repaint();
    }
    // an edit may wait for the person to allow it
    match rrx.recv_timeout(CONFIRM_TIMEOUT + Duration::from_secs(15)) {
        Ok(Some(v)) => {
            // a successful initialize starts a session
            let mut extra = String::new();
            if is_init {
                if let Some(pv) = v.pointer("/result/protocolVersion").and_then(|x| x.as_str()) {
                    let sid = new_token()[..32].to_string();
                    extra = format!("Mcp-Session-Id: {sid}\r\n");
                    add_session(sessions, sid, pv);
                }
            }
            respond(&mut stream, "200 OK", "application/json", &v.to_string(), &extra)
        }
        Ok(None) => respond(&mut stream, "202 Accepted", "text/plain", "", ""),
        Err(_) => {
            let err = rpc_error(&id, -32000, "AudioCraft did not answer in time");
            respond(&mut stream, "504 Gateway Timeout", "application/json", &err.to_string(), "")
        }
    }
}

/// A new random access token (64 hex characters).
pub fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut out = String::new();
    for i in 0..4u64 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(i);
        h.write_u128(nanos);
        h.write_u32(std::process::id());
        h.write_usize(&out as *const String as usize);
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

/// What a client remembers between messages: the session AudioCraft gave at initialize and
/// the protocol version agreed there (sent back as headers on every later request).
#[derive(Default, Debug, Clone)]
pub struct Link {
    pub session: Option<String>,
    pub version: Option<String>,
}

/// POST one message to the running app, outside any session; (HTTP status, body).
#[cfg(test)]
pub fn post(port: u16, token: &str, body: &str) -> Result<(u16, String), String> {
    post_in(port, token, body, &mut Link::default())
}

/// POST one message within `link`'s session; an initialize answer fills `link` in.
pub fn post_in(port: u16, token: &str, body: &str, link: &mut Link) -> Result<(u16, String), String> {
    let mut more = String::new();
    if let Some(s) = &link.session {
        more.push_str(&format!("Mcp-Session-Id: {s}\r\n"));
    }
    if let Some(v) = &link.version {
        more.push_str(&format!("MCP-Protocol-Version: {v}\r\n"));
    }
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .map_err(|e| format!("cannot reach AudioCraft on port {port}: {e}"))?;
    let _ = s.set_read_timeout(Some(CONFIRM_TIMEOUT + Duration::from_secs(30)));
    write!(
        s,
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nAuthorization: Bearer {token}\r\n{more}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .map_err(|e| e.to_string())?;
    let mut resp = String::new();
    s.read_to_string(&mut resp).map_err(|e| e.to_string())?;
    let (head, body) = resp.split_once("\r\n\r\n").unwrap_or((resp.as_str(), ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    if status == 200 {
        if let Some(sid) = head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case("mcp-session-id").then(|| v.trim().to_string())
        }) {
            link.session = Some(sid);
        }
        if let Some(v) = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| v.pointer("/result/protocolVersion").and_then(|x| x.as_str()).map(str::to_string))
        {
            link.version = Some(v);
        }
    }
    Ok((status, body.to_string()))
}

/// `audiocraft --mcp-stdio`: newline-delimited JSON-RPC on stdin/stdout, forwarded to the
/// running AudioCraft (its port and token come from the saved preferences).
pub fn stdio_bridge() -> i32 {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    let mut link = Link::default();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let msg = serde_json::from_str::<Value>(&line).ok();
        let id = msg.as_ref().and_then(|v| v.get("id").cloned());
        if msg.as_ref().and_then(|m| m.get("method")).and_then(|m| m.as_str()) == Some("initialize") {
            link = Link::default();
        }
        // read for every message: turning MCP on, or a new port or token, works without
        // restarting the assistant
        let p = crate::prefs::load();
        let answer = if !p.mcp_enabled {
            id.map(|id| {
                rpc_error(
                    &id,
                    -32000,
                    "MCP is off in AudioCraft: turn it on in Preferences > Control Surface",
                )
                .to_string()
            })
        } else {
            let mut r = post_in(p.mcp_port, &p.mcp_token, &line, &mut link);
            if matches!(r, Ok((404, _))) && link.session.is_some() {
                // AudioCraft was restarted and does not know the old session: carry on without
                link.session = None;
                r = post_in(p.mcp_port, &p.mcp_token, &line, &mut link);
            }
            let is_rpc = |b: &str| {
                serde_json::from_str::<Value>(b).is_ok_and(|v| v.is_array() || v.get("jsonrpc").is_some())
            };
            match r {
                Ok((_, body)) if is_rpc(&body) => Some(body),
                Ok((202, _)) => None,
                Ok((code, body)) => id.map(|id| {
                    rpc_error(&id, -32000, &format!("AudioCraft answered {code}: {body}"))
                        .to_string()
                }),
                Err(e) => id.map(|id| {
                    rpc_error(&id, -32000, &format!("{e} (is AudioCraft running with MCP on?)"))
                        .to_string()
                }),
            }
        };
        if let Some(a) = answer {
            let _ = writeln!(out, "{a}");
            let _ = out.flush();
        }
    }
    0
}

/// A change waiting for the person's Allow / Deny.
pub struct McpPending {
    pub msg: Value,
    pub reply: mpsc::Sender<Option<Value>>,
    pub at: Instant,
    pub what: String,
}

/// The app's side of MCP.
#[derive(Default)]
pub struct McpState {
    pub server: Option<McpServer>,
    /// The (port, token) the server was started with.
    pub running_with: Option<(u16, String)>,
    pub status: String,
    /// Recent calls, newest last.
    pub log: VecDeque<String>,
    pub calls: VecDeque<Instant>,
    pub pending: VecDeque<McpPending>,
    /// The person allowed every change until the app closes (or MCP settings change).
    pub allow_all: bool,
}

/// The permission level a command needs, or `None` when assistants may never run it.
pub fn command_level(id: &str) -> Option<u8> {
    let cmd = crate::commands::find(id)?;
    // commands that need the person (a dialog or a window to fill in), quit, launch other
    // programs, or scan and load plug-ins: never
    if cmd.title.trim_end().ends_with("...") || cmd.title.ends_with('\u{2026}') {
        return None;
    }
    let group = id.split('.').next().unwrap_or("");
    match group {
        "help" | "plugins" | "video" => return None,
        "file" => {
            return match id {
                "file.save" => Some(3),
                _ => None,
            };
        }
        _ => {}
    }
    let navigate = matches!(group, "view" | "transport" | "tool" | "window")
        || id.contains("select")
        || id.contains("zoom")
        || id.contains("next")
        || id.contains("prev")
        || id.contains("goto");
    Some(if navigate { 1 } else { 2 })
}

fn tool_list() -> Value {
    let obj = |props: Value, req: &[&str]| {
        json!({"type": "object", "properties": props, "required": req, "additionalProperties": false})
    };
    let looks = json!({"readOnlyHint": true, "openWorldHint": false});
    let moves = json!({"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false});
    let runs = json!({"readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": false});
    let num_or_null = json!({"type": ["number", "null"]});
    let state_out = json!({
        "type": "object",
        "properties": {
            "editor": {"type": "string", "enum": ["Waveform", "Multitrack"]},
            "playing": {"type": "boolean"},
            "playhead": {"type": "number", "description": "seconds"},
            "selection": {"type": ["array", "null"], "items": {"type": "number"}, "description": "[start, end] in seconds"},
            "session": {"type": ["object", "null"], "properties": {
                "name": {"type": "string"},
                "saved_at": {"type": ["string", "null"]},
                "sample_rate": {"type": "number"},
                "length": {"type": "number"},
                "tracks": {"type": "integer"}
            }},
            "files": {"type": "array", "items": {"type": "object", "properties": {
                "name": {"type": "string"},
                "seconds": {"type": "number"},
                "channels": {"type": "integer"},
                "sample_rate": {"type": "number"},
                "offline": {"type": "boolean"}
            }}},
            "current_file": {"type": ["string", "null"]},
            "permission": {"type": "string"}
        },
        "required": ["editor", "playing", "playhead", "files", "permission"]
    });
    let tracks_out = json!({
        "type": "object",
        "properties": {"tracks": {"type": "array", "items": {"type": "object", "properties": {
            "track": {"type": "integer"},
            "name": {"type": "string"},
            "volume_db": {"type": "number"},
            "pan": {"type": "number"},
            "mute": {"type": "boolean"},
            "solo": {"type": "boolean"},
            "prerender": {"type": "boolean"},
            "clips": {"type": "array", "items": {"type": "object", "properties": {
                "file": {"type": ["string", "null"]},
                "start": num_or_null,
                "length": {"type": "number"}
            }}}
        }}}},
        "required": ["tracks"]
    });
    let commands_out = json!({
        "type": "object",
        "properties": {"commands": {"type": "array", "items": {"type": "object", "properties": {
            "id": {"type": "string"},
            "title": {"type": "string"},
            "doc": {"type": "string"}
        }}}},
        "required": ["commands"]
    });
    json!([
        {"name": "get_state", "title": "Get AudioCraft's state",
         "description": "What AudioCraft shows now: the editor (Waveform or Multitrack), playback, playhead and selection in seconds, the session's name, length and track count, and the open files.",
         "inputSchema": obj(json!({}), &[]), "outputSchema": state_out, "annotations": looks},
        {"name": "list_tracks", "title": "List the session's tracks",
         "description": "The multitrack session's tracks: index, name, volume (dB), pan (-100..100), mute, solo, pre-render, and their clips (file, start, length in seconds).",
         "inputSchema": obj(json!({}), &[]), "outputSchema": tracks_out, "annotations": looks},
        {"name": "list_commands", "title": "List the commands you may run",
         "description": "The AudioCraft commands an assistant may run at the current permission level, with their help text.",
         "inputSchema": obj(json!({}), &[]), "outputSchema": commands_out, "annotations": looks},
        {"name": "run_command", "title": "Run a command",
         "description": "Run an AudioCraft command by its id (see list_commands), on the current selection, like the menu item would. Edits can be undone with edit.undo.",
         "inputSchema": obj(json!({"id": {"type": "string", "description": "a command id, e.g. transport.play"}}), &["id"]),
         "annotations": runs},
        {"name": "set_playhead", "title": "Move the playhead",
         "description": "Move the playhead to a time in seconds.",
         "inputSchema": obj(json!({"seconds": {"type": "number", "minimum": 0}}), &["seconds"]),
         "annotations": moves},
        {"name": "set_selection", "title": "Select a time range",
         "description": "Select a time range in seconds (start < end) in the current editor.",
         "inputSchema": obj(json!({"start": {"type": "number", "minimum": 0}, "end": {"type": "number", "minimum": 0}}), &["start", "end"]),
         "annotations": moves},
        {"name": "set_track", "title": "Change a track",
         "description": "Change a multitrack track: its name, volume (dB, -60..+15), pan (-100 left .. 100 right), mute or solo. One undoable step.",
         "annotations": moves,
         "inputSchema": obj(json!({
            "track": {"type": "integer", "minimum": 0, "description": "track index from list_tracks"},
            "name": {"type": "string", "maxLength": 64},
            "volume_db": {"type": "number", "minimum": crate::session::TRACK_DB_MIN, "maximum": crate::session::TRACK_DB_MAX},
            "pan": {"type": "number", "minimum": -100, "maximum": 100},
            "mute": {"type": "boolean"},
            "solo": {"type": "boolean"}
         }), &["track"])}
    ])
}

/// The permission level a tool call needs (`None`: never allowed).
pub fn call_level(name: &str, args: &Value) -> Option<u8> {
    match name {
        "get_state" | "list_tracks" | "list_commands" => Some(0),
        "set_playhead" | "set_selection" => Some(1),
        "set_track" => Some(2),
        "run_command" => args.get("id").and_then(|v| v.as_str()).and_then(command_level),
        _ => None,
    }
}

fn r1(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

impl App {
    /// Keep the server in step with Preferences, take requests, and show the Allow / Deny
    /// window for changes that wait for the person. Runs every frame.
    pub fn mcp_service(&mut self, ctx: &egui::Context) {
        let want = if self.prefs.mcp_enabled {
            if self.prefs.mcp_token.len() < 16 {
                self.prefs.mcp_token = new_token();
                crate::prefs::save(&self.prefs);
            }
            Some((self.prefs.mcp_port, self.prefs.mcp_token.clone()))
        } else {
            None
        };
        if self.mcp.running_with != want {
            self.mcp.server = None;
            self.mcp.allow_all = false;
            // waiting changes are answered: the settings they were asked under are gone
            for p in std::mem::take(&mut self.mcp.pending) {
                let _ = p.reply.send(refuse(&p.msg, "MCP settings changed"));
            }
            self.mcp.status = match &want {
                None => "Off".into(),
                Some((port, token)) => match McpServer::start(*port, token.clone(), Some(ctx.clone())) {
                    Ok(s) => {
                        let st = format!("Listening on http://127.0.0.1:{}/mcp", s.port);
                        self.mcp.server = Some(s);
                        st
                    }
                    Err(e) => e,
                },
            };
            self.mcp.running_with = want;
        }
        let incoming: Vec<McpRequest> = match &self.mcp.server {
            Some(s) => s.rx.try_iter().collect(),
            None => vec![],
        };
        for req in incoming {
            match self.mcp_waits_for_person(&req.msg) {
                Some(what) => {
                    self.mcp_log(format!("waiting for you: {what}"));
                    self.mcp.pending.push_back(McpPending {
                        msg: req.msg,
                        reply: req.reply,
                        at: Instant::now(),
                        what,
                    });
                }
                None => {
                    let a = self.mcp_handle(&req.msg, ctx);
                    let _ = req.reply.send(a);
                }
            }
        }
        // unanswered for a minute: declined
        while self
            .mcp
            .pending
            .front()
            .is_some_and(|p| p.at.elapsed() > CONFIRM_TIMEOUT)
        {
            if let Some(p) = self.mcp.pending.pop_front() {
                self.mcp_log(format!("declined (no answer): {}", p.what));
                let _ = p.reply.send(refuse(&p.msg, "the person did not answer in time"));
            }
        }
        self.mcp_confirm_window(ctx);
        if !self.mcp.pending.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn mcp_log(&mut self, s: String) {
        let t = chrono_like_now();
        self.mcp.log.push_back(format!("{t}  {s}"));
        while self.mcp.log.len() > 200 {
            self.mcp.log.pop_front();
        }
    }

    /// A change that must wait for the person ("Ask me before each change"): what it is.
    fn mcp_waits_for_person(&self, msg: &Value) -> Option<String> {
        if !self.prefs.mcp_confirm || self.mcp.allow_all {
            return None;
        }
        let msgs: Vec<&Value> = match msg.as_array() {
            Some(a) => a.iter().collect(),
            None => vec![msg],
        };
        for m in msgs {
            if m.get("method").and_then(|v| v.as_str()) != Some("tools/call") {
                continue;
            }
            let p = m.get("params").cloned().unwrap_or(Value::Null);
            let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = p.get("arguments").cloned().unwrap_or(Value::Null);
            let lvl = call_level(name, &args);
            if lvl.is_some_and(|l| l >= 2 && l <= self.prefs.mcp_level) {
                return Some(describe_call(name, &args));
            }
        }
        None
    }

    /// The Allow / Deny window for a waiting change.
    fn mcp_confirm_window(&mut self, ctx: &egui::Context) {
        let Some(p) = self.mcp.pending.front() else {
            return;
        };
        let what = p.what.clone();
        let left = CONFIRM_TIMEOUT.saturating_sub(p.at.elapsed()).as_secs();
        let more = self.mcp.pending.len() - 1;
        let mut choice: Option<u8> = None;
        egui::Window::new("AI assistant request")
            .id(egui::Id::new("mcp_confirm"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label("An AI assistant connected over MCP wants to:");
                ui.add_space(4.0);
                ui.label(egui::RichText::new(&what).strong());
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!(
                        "It can be undone with Ctrl+Z. Declined in {left} s if you don't answer.{}",
                        if more > 0 { format!(" ({more} more waiting)") } else { String::new() }
                    ))
                    .small()
                    .color(crate::theme::text_dim()),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Deny").clicked() {
                        choice = Some(0);
                    }
                    if ui
                        .button("Allow all until AudioCraft closes")
                        .on_hover_text("Stop asking for this run of AudioCraft")
                        .clicked()
                    {
                        choice = Some(2);
                    }
                    if crate::theme::dialog_button(ui, "Allow", true).clicked() {
                        choice = Some(1);
                    }
                });
            });
        if let Some(c) = choice {
            self.mcp_decide(c, ctx);
        }
    }

    /// Answer the oldest waiting change: 0 deny, 1 allow, 2 allow all until AudioCraft closes.
    pub fn mcp_decide(&mut self, c: u8, ctx: &egui::Context) {
        let Some(p) = self.mcp.pending.pop_front() else {
            return;
        };
        if c == 0 {
            self.mcp_log(format!("denied: {}", p.what));
            let _ = p.reply.send(refuse(&p.msg, "the person declined this change"));
            return;
        }
        if c == 2 {
            self.mcp.allow_all = true;
        }
        self.mcp_log(format!("allowed: {}", p.what));
        let a = self.mcp_handle(&p.msg, ctx);
        let _ = p.reply.send(a);
    }

    /// Answer one JSON-RPC message or batch (`None` for notifications only).
    pub fn mcp_handle(&mut self, msg: &Value, ctx: &egui::Context) -> Option<Value> {
        if let Some(batch) = msg.as_array() {
            if batch.is_empty() {
                return Some(rpc_error(&Value::Null, -32600, "empty batch"));
            }
            if batch.iter().any(|m| m.get("method").and_then(|v| v.as_str()) == Some("initialize")) {
                return Some(rpc_error(&Value::Null, -32600, "initialize must not be part of a batch"));
            }
            let out: Vec<Value> = batch.iter().filter_map(|m| self.mcp_one(m, ctx)).collect();
            return (!out.is_empty()).then(|| Value::Array(out));
        }
        self.mcp_one(msg, ctx)
    }

    fn mcp_one(&mut self, m: &Value, ctx: &egui::Context) -> Option<Value> {
        let Some(obj) = m.as_object() else {
            return Some(rpc_error(&Value::Null, -32600, "invalid request: expected a JSON-RPC object"));
        };
        let id = obj.get("id").cloned();
        let Some(method) = obj.get("method").and_then(|v| v.as_str()) else {
            // a response from the client (AudioCraft never asks it anything): nothing to answer
            return match id {
                Some(id) if !obj.contains_key("result") && !obj.contains_key("error") => {
                    Some(rpc_error(&id, -32600, "invalid request: no method"))
                }
                _ => None,
            };
        };
        let Some(id) = id else {
            return None; // a notification (initialized, cancelled...)
        };
        if !(id.is_string() || id.is_i64() || id.is_u64()) {
            return Some(rpc_error(&Value::Null, -32600, "the request id must be a string or an integer"));
        }
        if obj.get("jsonrpc").and_then(|v| v.as_str()) != Some("2.0") {
            return Some(rpc_error(&id, -32600, "jsonrpc must be \"2.0\""));
        }
        let ok = |r: Value| Some(json!({"jsonrpc": "2.0", "id": id.clone(), "result": r}));
        match method {
            "initialize" => {
                let asked = m
                    .pointer("/params/protocolVersion")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                // the assistant's version when AudioCraft speaks it, otherwise the newest
                let version = PROTOCOL_VERSIONS
                    .iter()
                    .find(|v| **v == asked)
                    .copied()
                    .unwrap_or(PROTOCOL_VERSIONS[0]);
                self.mcp_log("an assistant connected".into());
                ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {
                        "name": "AudioCraft",
                        "title": "AudioCraft audio editor",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "instructions": format!(
                        "AudioCraft is an audio editor (Waveform and Multitrack editors). Permission level: {}. Use get_state and list_tracks first; list_commands shows what you may run. Times are in seconds. Edits are undoable (run_command edit.undo). Some changes wait for the person to allow them.",
                        LEVELS[self.prefs.mcp_level.min(3) as usize].0
                    )
                }))
            }
            "ping" => ok(json!({})),
            "tools/list" => ok(json!({"tools": tool_list()})),
            "tools/call" => {
                let p = m.get("params").cloned().unwrap_or(Value::Null);
                let Some(name) = p.get("name").and_then(|v| v.as_str()).map(str::to_string) else {
                    return Some(rpc_error(&id, -32602, "tools/call needs params.name"));
                };
                // a tool that does not exist is a protocol error (the assistant asked wrongly),
                // not a tool that failed
                if !TOOL_NAMES.contains(&name.as_str()) {
                    self.mcp_log(format!("unknown tool {name}"));
                    return Some(rpc_error(&id, -32602, &format!("unknown tool: {name}")));
                }
                let args = match p.get("arguments") {
                    None | Some(Value::Null) => json!({}),
                    Some(a) if a.is_object() => a.clone(),
                    Some(_) => return Some(rpc_error(&id, -32602, "arguments must be an object")),
                };
                let r = self.mcp_call(&name, &args, ctx);
                let line = match &r {
                    Ok(_) => format!("{}  ok", describe_call(&name, &args)),
                    Err(e) => format!("{}  refused: {e}", describe_call(&name, &args)),
                };
                self.mcp_log(line);
                Some(tool_answer(m, r))
            }
            _ => Some(rpc_error(&id, -32601, &format!("unknown method {method}"))),
        }
    }

    /// Run one tool, with every guardrail.
    /// A text answer is a JSON string; any other value is data (sent as structured content).
    pub fn mcp_call(&mut self, name: &str, args: &Value, ctx: &egui::Context) -> Result<Value, String> {
        // the rate limit
        let limit = self.prefs.mcp_rate_per_min.max(1) as usize;
        let now = Instant::now();
        while self
            .mcp
            .calls
            .front()
            .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60))
        {
            self.mcp.calls.pop_front();
        }
        if self.mcp.calls.len() >= limit {
            return Err(format!("too many requests: at most {limit} a minute (Preferences > Control Surface)"));
        }
        self.mcp.calls.push_back(now);
        // the permission level
        let level = self.prefs.mcp_level.min(3);
        let need = call_level(name, args).ok_or_else(|| match name {
            "run_command" => "that command is not available to assistants (it needs you, quits, opens a dialog or another program)".to_string(),
            _ => format!("unknown tool {name}"),
        })?;
        if need > level {
            return Err(format!(
                "this needs the \"{}\" permission; AudioCraft allows \"{}\" (Preferences > Control Surface)",
                LEVELS[need as usize].0, LEVELS[level as usize].0
            ));
        }
        match name {
            "get_state" => Ok(self.mcp_state()),
            "list_tracks" => Ok(json!({"tracks": self.mcp_tracks()})),
            "list_commands" => {
                let v: Vec<Value> = crate::commands::all()
                    .filter(|c| command_level(c.id).is_some_and(|l| l <= level))
                    .map(|c| json!({"id": c.id, "title": c.title, "doc": c.doc}))
                    .collect();
                Ok(json!({"commands": v}))
            }
            "run_command" => {
                let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
                if id == "file.save" {
                    let has_place = match self.mode {
                        crate::app::Mode::Multitrack => self.session.as_ref().is_some_and(|s| s.path.is_some()),
                        crate::app::Mode::Waveform => self.cur().is_some_and(|f| f.path.is_some()),
                    };
                    if !has_place {
                        return Err("save it once yourself first: assistants never choose where files go".into());
                    }
                }
                let had_dialog = self.dialog.is_some();
                self.exec(id, ctx)?;
                if !had_dialog && self.dialog.is_some() {
                    return Ok(format!("ran {id}; it opened a dialog in AudioCraft that waits for the person").into());
                }
                Ok(format!("ran {id}").into())
            }
            "set_playhead" => {
                let t = args.get("seconds").and_then(|v| v.as_f64()).filter(|t| t.is_finite() && *t >= 0.0)
                    .ok_or("seconds must be a number of seconds, 0 or more")?;
                self.set_cursor(t);
                Ok(format!("playhead at {:.3} s", self.cursor()).into())
            }
            "set_selection" => {
                let a = args.get("start").and_then(|v| v.as_f64()).filter(|t| t.is_finite() && *t >= 0.0);
                let b = args.get("end").and_then(|v| v.as_f64()).filter(|t| t.is_finite() && *t >= 0.0);
                let (Some(a), Some(b)) = (a, b) else {
                    return Err("start and end must be seconds, 0 or more".into());
                };
                if b <= a {
                    return Err("end must be after start".into());
                }
                match self.mode {
                    crate::app::Mode::Multitrack => {
                        let s = self.session.as_mut().ok_or("no session is open")?;
                        s.selection = Some((a, b));
                    }
                    crate::app::Mode::Waveform => {
                        let f = self.cur_mut().ok_or("no file is open")?;
                        let d = f.data.duration();
                        f.selection = Some((a.min(d), b.min(d)));
                    }
                }
                Ok(format!("selected {a:.3}-{b:.3} s").into())
            }
            "set_track" => {
                let i = args.get("track").and_then(|v| v.as_u64()).ok_or("track must be a track index")? as usize;
                let s = self.session.as_mut().ok_or("no session is open")?;
                let t = s.tracks.get_mut(i).ok_or_else(|| format!("there is no track {i}"))?;
                let mut changed = vec![];
                if let Some(n) = args.get("name").and_then(|v| v.as_str()) {
                    let n: String = n.chars().filter(|c| !c.is_control()).take(64).collect();
                    if !n.trim().is_empty() {
                        t.name = n;
                        changed.push("name");
                    }
                }
                if let Some(v) = args.get("volume_db").and_then(|v| v.as_f64()).filter(|v| v.is_finite()) {
                    t.volume_db = (v as f32)
                        .clamp(crate::session::TRACK_DB_MIN, crate::session::TRACK_DB_MAX);
                    changed.push("volume");
                }
                if let Some(v) = args.get("pan").and_then(|v| v.as_f64()).filter(|v| v.is_finite()) {
                    t.pan = (v as f32 / 100.0).clamp(-1.0, 1.0);
                    changed.push("pan");
                }
                if let Some(v) = args.get("mute").and_then(|v| v.as_bool()) {
                    t.mute = v;
                    changed.push("mute");
                }
                if let Some(v) = args.get("solo").and_then(|v| v.as_bool()) {
                    t.solo = v;
                    changed.push("solo");
                }
                if changed.is_empty() {
                    return Err("nothing to change: give name, volume_db, pan, mute or solo".into());
                }
                let msg = format!("track {i} {}", changed.join(", "));
                self.session_push("Assistant: Track");
                Ok(msg.into())
            }
            _ => Err(format!("unknown tool {name}")),
        }
    }

    fn mcp_state(&self) -> Value {
        let mode = match self.mode {
            crate::app::Mode::Waveform => "Waveform",
            crate::app::Mode::Multitrack => "Multitrack",
        };
        let sel = match self.mode {
            crate::app::Mode::Multitrack => self.session.as_ref().and_then(|s| s.selection),
            crate::app::Mode::Waveform => self.cur().and_then(|f| f.selection),
        };
        json!({
            "editor": mode,
            "playing": self.is_playing(),
            "playhead": r1(self.cursor()),
            "selection": sel.map(|(a, b)| json!([r1(a), r1(b)])),
            "session": self.session.as_ref().map(|s| json!({
                "name": s.name,
                "saved_at": s.path.as_ref().map(|p| p.display().to_string()),
                "sample_rate": s.rate,
                "length": r1(s.length()),
                "tracks": s.tracks.len(),
            })),
            "files": self.files.iter().map(|f| json!({
                "name": f.name,
                "seconds": r1(f.data.duration()),
                "channels": f.data.channels(),
                "sample_rate": f.data.rate,
                "offline": self.offline.contains_key(&f.id),
            })).collect::<Vec<_>>(),
            "current_file": self.cur().map(|f| f.name.clone()),
            "permission": LEVELS[self.prefs.mcp_level.min(3) as usize].0,
        })
    }

    fn mcp_tracks(&self) -> Value {
        let Some(s) = &self.session else {
            return json!([]);
        };
        Value::Array(
            s.tracks
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    json!({
                        "track": i,
                        "name": t.name,
                        "volume_db": r1(t.volume_db as f64),
                        "pan": (t.pan * 100.0).round(),
                        "mute": t.mute,
                        "solo": t.solo,
                        "prerender": t.prerender,
                        "clips": t.clips.iter().map(|c| json!({
                            "file": self.file(c.file_id).map(|f| f.name.clone()),
                            "start": r1(c.start),
                            "length": r1(c.len),
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect(),
        )
    }

    /// The text to paste into an assistant's settings to connect it to AudioCraft.
    pub fn mcp_client_config(&self) -> String {
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "audiocraft".into());
        json!({
            "mcpServers": {
                "audiocraft": {"command": exe, "args": ["--mcp-stdio"]}
            }
        })
        .to_string()
    }
}

/// A tools/call answer: text, data (as structured content plus the same JSON as text, for
/// assistants that only read text), or an error the assistant can read.
fn tool_answer(m: &Value, r: Result<Value, String>) -> Value {
    let id = m.get("id").cloned().unwrap_or(Value::Null);
    let result = match r {
        Ok(Value::String(t)) => json!({"content": [{"type": "text", "text": t}], "isError": false}),
        Ok(v) => json!({
            "content": [{"type": "text", "text": v.to_string()}],
            "structuredContent": v,
            "isError": false
        }),
        Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
    };
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// Answer a waiting message (one request or a batch) with the same refusal for each request.
fn refuse(msg: &Value, why: &str) -> Option<Value> {
    match msg.as_array() {
        Some(batch) => {
            let out: Vec<Value> = batch
                .iter()
                .filter(|m| m.get("id").is_some() && m.get("method").is_some())
                .map(|m| tool_answer(m, Err(why.to_string())))
                .collect();
            (!out.is_empty()).then_some(Value::Array(out))
        }
        None => Some(tool_answer(msg, Err(why.to_string()))),
    }
}

/// A short readable description of a tool call for the log and the Allow window.
fn describe_call(name: &str, args: &Value) -> String {
    match name {
        "run_command" => {
            let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            match crate::commands::find(id) {
                Some(c) => format!("run \"{}\" ({id})", c.title),
                None => format!("run {id}"),
            }
        }
        "set_track" => format!("change track {}", args.get("track").map(|v| v.to_string()).unwrap_or_default()),
        _ => {
            let a = args.to_string();
            let a: String = a.chars().take(80).collect();
            format!("{name} {a}")
        }
    }
}

/// hh:mm:ss of the local day (for the activity log), without a date library.
fn chrono_like_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let d = secs % 86_400;
    format!("{:02}:{:02}:{:02} UTC", d / 3600, d / 60 % 60, d % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_that_need_the_person_are_never_available() {
        assert_eq!(command_level("file.exit"), None);
        assert_eq!(command_level("file.open"), None);
        assert_eq!(command_level("file.save"), Some(3));
        assert_eq!(command_level("no.such.command"), None);
        // every command that opens a dialog ("...") is out
        for c in crate::commands::all() {
            if c.title.trim_end().ends_with("...") {
                assert_eq!(command_level(c.id), None, "{}", c.id);
            }
        }
        // there are commands at each of navigate and edit
        assert!(crate::commands::all().any(|c| command_level(c.id) == Some(1)));
        assert!(crate::commands::all().any(|c| command_level(c.id) == Some(2)));
    }

    #[test]
    fn only_this_computer_names_pass() {
        assert!(local_name("127.0.0.1:4000", 4000, false));
        assert!(local_name("localhost:4000", 4000, false));
        assert!(!local_name("evil.example:4000", 4000, false));
        assert!(!local_name("127.0.0.1:4001", 4000, false));
        assert!(local_name("http://localhost:4000", 4000, true));
        assert!(!local_name("https://evil.example", 4000, true));
        assert!(same_secret("abc", "abc"));
        assert!(!same_secret("abc", "abd"));
        assert!(!same_secret("abc", "abcd"));
        let a = new_token();
        assert_eq!(a.len(), 64);
        assert_ne!(a, new_token());
    }

    #[test]
    fn sessions_are_kept_found_and_ended() {
        let s: Sessions = Arc::default();
        add_session(&s, "abc".into(), "2025-11-25");
        assert_eq!(session_version(&s, "abc").as_deref(), Some("2025-11-25"));
        assert_eq!(session_version(&s, "abd"), None);
        assert!(end_session(&s, "abc"));
        assert!(!end_session(&s, "abc"));
        for i in 0..MAX_SESSIONS + 5 {
            add_session(&s, format!("s{i}"), "2025-06-18");
        }
        assert_eq!(s.lock().unwrap().len(), MAX_SESSIONS, "the oldest are forgotten");
        assert_eq!(session_version(&s, "s0"), None);
        // every tool is offered, and every offered tool has a permission level
        let tools = tool_list();
        for t in tools.as_array().unwrap() {
            let n = t["name"].as_str().unwrap();
            assert!(TOOL_NAMES.contains(&n), "{n}");
            assert!(t["title"].is_string() && t["annotations"].is_object(), "{n}");
        }
        assert_eq!(tools.as_array().unwrap().len(), TOOL_NAMES.len());
    }
}
