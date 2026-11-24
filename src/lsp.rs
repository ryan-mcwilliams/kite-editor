//! A small Language Server Protocol client: spawn a server, frame JSON-RPC
//! over stdio, and forward everything it says to the main event loop.

use crate::app::AppEvent;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Each spawned server gets a new generation, so events from a server that
/// has since been restarted can be told apart and ignored.
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

/// What a pending request was for, so the response can be routed.
#[derive(Debug, Clone)]
pub enum Pending {
    Initialize,
    Completion {
        path: PathBuf,
        /// Where the word being completed started when this was asked.
        start: usize,
    },
    /// For the mouse, the word (char range) it was asked about; None if
    /// asked from the keyboard about the cursor.
    Hover {
        mouse: Option<(usize, usize)>,
    },
    Definition,
    Format {
        path: PathBuf,
        version: i32,
        save: bool,
    },
}

pub struct Server {
    pub name: String,
    pub generation: u64,
    child: Child,
    stdin: ChildStdin,
    next_id: i64,
    pub pending: HashMap<i64, Pending>,
    pub ready: bool,
    queue: Vec<Value>,
    /// Characters that should trigger completion, from server capabilities.
    pub triggers: Vec<char>,
    pub progress: Option<String>,
}

impl Server {
    pub fn spawn(
        lang: &str,
        cmd: &[String],
        root: &Path,
        tx: Sender<AppEvent>,
    ) -> io::Result<Server> {
        let (program, args) = cmd
            .split_first()
            .ok_or_else(|| io::Error::other("empty command"))?;
        let mut child = Command::new(program)
            .args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let key = lang.to_string();
        let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        // Keep the last thing the server said on stderr (and whether stderr
        // has closed), to explain why it stopped if it does.
        let last_err = Arc::new(Mutex::new((None::<String>, false)));
        let err = last_err.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if !line.trim().is_empty() {
                    err.lock().unwrap().0 = Some(line.trim().to_string());
                }
            }
            err.lock().unwrap().1 = true;
        });
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Some(msg) = read_message(&mut reader) {
                if tx
                    .send(AppEvent::Lsp(key.clone(), generation, msg))
                    .is_err()
                {
                    return;
                }
            }
            // Give stderr a moment to deliver the server's last words.
            for _ in 0..20 {
                if last_err.lock().unwrap().1 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let reason = last_err.lock().unwrap().0.take();
            let _ = tx.send(AppEvent::LspExit(key, generation, reason));
        });
        let mut server = Server {
            name: Path::new(program)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            generation,
            child,
            stdin,
            next_id: 1,
            pending: HashMap::new(),
            ready: false,
            queue: vec![],
            triggers: vec!['.'],
            progress: None,
        };
        let uri = path_to_uri(root);
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        server.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "clientInfo": { "name": "kite" },
                "rootUri": uri,
                "workspaceFolders": [{ "uri": uri, "name": name }],
                "capabilities": {
                    "general": { "positionEncodings": ["utf-16"] },
                    "window": { "workDoneProgress": true },
                    "textDocument": {
                        "synchronization": { "didSave": true },
                        "publishDiagnostics": {},
                        "completion": { "completionItem": { "snippetSupport": false, "documentationFormat": ["plaintext"] } },
                        "hover": { "contentFormat": ["plaintext", "markdown"] },
                        "definition": {},
                        "formatting": {}
                    }
                }
            }),
            Pending::Initialize,
        );
        Ok(server)
    }

    fn write(&mut self, msg: &Value) {
        let body = msg.to_string();
        let _ = write!(self.stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body);
        let _ = self.stdin.flush();
    }

    fn send(&mut self, msg: Value) {
        let is_init = msg["method"] == "initialize" || msg["method"] == "initialized";
        if self.ready || is_init || msg.get("method").is_none() {
            self.write(&msg);
        } else {
            self.queue.push(msg);
        }
    }

    pub fn request(&mut self, method: &str, params: Value, pending: Pending) {
        let id = self.next_id;
        self.next_id += 1;
        self.pending.insert(id, pending);
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
    }

    pub fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    pub fn respond(&mut self, id: &Value, result: Value) {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }

    /// Called when the `initialize` response arrives.
    pub fn initialized(&mut self, result: &Value) {
        if let Some(t) =
            result["capabilities"]["completionProvider"]["triggerCharacters"].as_array()
        {
            self.triggers = t
                .iter()
                .filter_map(|v| v.as_str()?.chars().next())
                .collect();
        }
        self.notify("initialized", json!({}));
        self.ready = true;
        for msg in std::mem::take(&mut self.queue) {
            self.write(&msg);
        }
    }

    pub fn shutdown(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_message(reader: &mut impl BufRead) -> Option<Value> {
    let mut len = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0; len?];
    reader.read_exact(&mut body).ok()?;
    Some(serde_json::from_slice(&body).unwrap_or(Value::Null))
}

pub fn path_to_uri(path: &Path) -> String {
    let mut out = String::from("file://");
    for b in path.to_string_lossy().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() && rest.is_char_boundary(i + 3) {
            if let Ok(v) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    Some(PathBuf::from(String::from_utf8_lossy(&out).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_round_trip_with_spaces_and_unicode() {
        let p = Path::new("/tmp/my project/café.rs");
        let uri = path_to_uri(p);
        assert_eq!(uri, "file:///tmp/my%20project/caf%C3%A9.rs");
        assert_eq!(uri_to_path(&uri).unwrap(), p);
    }

    #[test]
    fn reads_framed_messages() {
        let raw = b"Content-Length: 13\r\nContent-Type: x\r\n\r\n{\"a\":[1,2,3]}Content-Length: 2\r\n\r\n{}";
        let mut r = BufReader::new(&raw[..]);
        assert_eq!(read_message(&mut r).unwrap()["a"][2], 3);
        assert!(read_message(&mut r).unwrap().is_object());
        assert!(read_message(&mut r).is_none());
    }
}
