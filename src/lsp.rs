use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionItem {
    pub label: String,
    pub detail: Option<String>,
    pub insert_text: Option<String>,
}

#[derive(Serialize)]
struct LspRequest<'a> {
    jsonrpc: &'a str,
    id: i64,
    method: &'a str,
    params: Value,
}

#[derive(Serialize)]
struct LspNotification<'a> {
    jsonrpc: &'a str,
    method: &'a str,
    params: Value,
}

#[derive(Deserialize)]
struct LspResponse {
    id: Option<i64>,
    result: Option<Value>,
    error: Option<Value>,
}

#[derive(Clone)]
pub struct LspClient {
    tx: mpsc::Sender<(
        Option<i64>,
        Vec<u8>,
        Option<oneshot::Sender<Result<Value, String>>>,
    )>,
    next_id: Arc<AtomicI64>,
    doc_ver: Arc<AtomicI64>,
}

impl LspClient {
    pub async fn spawn() -> Result<Self, String> {
        let bin_path = Self::find_sqls_bin()?;
        let config_path = Self::find_sqls_config();

        let mut cmd = Command::new(&bin_path);
        if let Some(cfg) = &config_path {
            cmd.args(&["-config", cfg.to_str().unwrap_or("")]);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child: Child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn sqls at {:?}: {}", bin_path, e))?;

        let mut stdin = child.stdin.take().ok_or("Failed to get stdin")?;
        let mut stdout = child.stdout.take().ok_or("Failed to get stdout")?;

        let (req_tx, mut req_rx) = mpsc::channel::<(
            Option<i64>,
            Vec<u8>,
            Option<oneshot::Sender<Result<Value, String>>>,
        )>(64);
        let pending = Arc::new(Mutex::new(HashMap::<
            i64,
            oneshot::Sender<Result<Value, String>>,
        >::new()));
        let pending_reader = Arc::clone(&pending);

        // Reader loop parsing Content-Length: <n>\r\n\r\n<json>
        tokio::spawn(async move {
            let mut buf = Vec::new();
            let mut temp = [0u8; 1024];

            loop {
                let n = match stdout.read(&mut temp).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                buf.extend_from_slice(&temp[..n]);

                loop {
                    // Find header delimiter \r\n\r\n
                    let header_end = match find_subsequence(&buf, b"\r\n\r\n") {
                        Some(pos) => pos,
                        None => break,
                    };

                    let header_str = String::from_utf8_lossy(&buf[..header_end]);
                    let mut content_len = 0;
                    for line in header_str.lines() {
                        if line.to_lowercase().starts_with("content-length:") {
                            if let Some(val) = line.split(':').nth(1) {
                                content_len = val.trim().parse::<usize>().unwrap_or(0);
                            }
                        }
                    }

                    let body_start = header_end + 4;
                    if buf.len() < body_start + content_len {
                        break; // Wait for full body
                    }

                    let body = &buf[body_start..body_start + content_len];
                    if let Ok(resp) = serde_json::from_slice::<LspResponse>(body) {
                        if let Some(id) = resp.id {
                            let mut map = pending_reader.lock().await;
                            if let Some(sender) = map.remove(&id) {
                                if let Some(err) = resp.error {
                                    let _ = sender.send(Err(err.to_string()));
                                } else if let Some(res) = resp.result {
                                    let _ = sender.send(Ok(res));
                                } else {
                                    let _ = sender.send(Ok(Value::Null));
                                }
                            }
                        }
                    }

                    buf.drain(..body_start + content_len);
                }
            }
        });

        // Writer loop
        let pending_writer = Arc::clone(&pending);
        tokio::spawn(async move {
            while let Some((id_opt, payload, sender_opt)) = req_rx.recv().await {
                if let (Some(id), Some(sender)) = (id_opt, sender_opt) {
                    pending_writer.lock().await.insert(id, sender);
                }
                let header = format!("Content-Length: {}\r\n\r\n", payload.len());
                if stdin.write_all(header.as_bytes()).await.is_err()
                    || stdin.write_all(&payload).await.is_err()
                {
                    break;
                }
                let _ = stdin.flush().await;
            }
        });

        let client = Self {
            tx: req_tx,
            next_id: Arc::new(AtomicI64::new(1)),
            doc_ver: Arc::new(AtomicI64::new(1)),
        };

        // Initialize handshake
        client.initialize().await?;

        Ok(client)
    }

    fn find_sqls_bin() -> Result<PathBuf, String> {
        let home = std::env::var("HOME").unwrap_or_default();
        let candidates = [
            PathBuf::from(format!("{}/.local/share/nvim/mason/bin/sqls", home)),
            PathBuf::from(format!(
                "{}/.local/share/nvim/mason/packages/sqls/sqls",
                home
            )),
        ];

        for p in &candidates {
            if p.exists() {
                return Ok(p.clone());
            }
        }

        if let Ok(paths) = std::env::var("PATH") {
            for path in std::env::split_paths(&paths) {
                let bin = path.join("sqls");
                if bin.is_file() {
                    return Ok(bin);
                }
            }
        }

        Err("sqls language server not found".to_string())
    }

    fn find_sqls_config() -> Option<PathBuf> {
        let home = std::env::var("HOME").unwrap_or_default();
        let p = PathBuf::from(format!("{}/.config/sqls/config.yml", home));
        if p.exists() {
            Some(p)
        } else {
            None
        }
    }

    async fn initialize(&self) -> Result<(), String> {
        let params = serde_json::json!({
            "processId": std::process::id(),
            "rootUri": "file:///tmp",
            "capabilities": {
                "textDocument": {
                    "completion": {
                        "completionItem": {
                            "snippetSupport": true
                        }
                    }
                }
            }
        });

        let _ = self.call("initialize", params).await?;
        self.notify("initialized", serde_json::json!({})).await?;
        self.notify(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": "file:///tmp/query.sql",
                    "languageId": "sql",
                    "version": 1,
                    "text": "SELECT * FROM "
                }
            }),
        )
        .await?;

        Ok(())
    }

    pub async fn update_document(&self, text: &str) {
        let ver = self.doc_ver.fetch_add(1, Ordering::SeqCst);
        let _ = self
            .notify(
                "textDocument/didChange",
                serde_json::json!({
                    "textDocument": {
                        "uri": "file:///tmp/query.sql",
                        "version": ver
                    },
                    "contentChanges": [
                        { "text": text }
                    ]
                }),
            )
            .await;
    }

    pub async fn get_completions(&self, line: usize, col: usize) -> Vec<CompletionItem> {
        let params = serde_json::json!({
            "textDocument": {
                "uri": "file:///tmp/query.sql"
            },
            "position": {
                "line": line,
                "character": col
            }
        });

        let mut items = Vec::new();
        if let Ok(res) = self.call("textDocument/completion", params).await {
            let list = if let Some(arr) = res.as_array() {
                arr.clone()
            } else if let Some(items_val) = res.get("items").and_then(|v| v.as_array()) {
                items_val.clone()
            } else {
                Vec::new()
            };

            for it in list {
                let label = it
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let detail = it
                    .get("detail")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let insert_text = it
                    .get("insertText")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                if !label.is_empty() {
                    items.push(CompletionItem {
                        label,
                        detail,
                        insert_text,
                    });
                }
            }
        }
        items
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = LspRequest {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };
        let payload = serde_json::to_vec(&req).map_err(|e| e.to_string())?;

        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send((Some(id), payload, Some(resp_tx)))
            .await
            .map_err(|e| e.to_string())?;

        match tokio::time::timeout(std::time::Duration::from_millis(150), resp_rx).await {
            Ok(Ok(res)) => res,
            Ok(Err(_)) => Err("LSP dropped channel".to_string()),
            Err(_) => Err("LSP timeout".to_string()),
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        let notif = LspNotification {
            jsonrpc: "2.0",
            method,
            params,
        };
        let payload = serde_json::to_vec(&notif).map_err(|e| e.to_string())?;
        self.tx
            .send((None, payload, None))
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
