use crate::models::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

#[derive(Serialize)]
struct RpcRequest<'a> {
    id: u64,
    method: &'a str,
    params: Value,
}

#[derive(Deserialize)]
struct RpcResponse {
    id: Option<u64>,
    result: Option<Value>,
    error: Option<String>,
}

#[derive(Clone)]
pub struct HornetClient {
    tx: mpsc::Sender<(u64, String, oneshot::Sender<Result<Value, String>>)>,
    next_id: Arc<AtomicU64>,
}

impl HornetClient {
    pub async fn spawn() -> Result<Self, String> {
        let server_path = Self::find_server_bin()?;
        let mut child: Child = Command::new(&server_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("Failed to spawn hornet-server at {:?}: {}", server_path, e))?;

        let mut stdin = child.stdin.take().ok_or("Failed to get stdin")?;
        let stdout = child.stdout.take().ok_or("Failed to get stdout")?;

        let (req_tx, mut req_rx) = mpsc::channel::<(u64, String, oneshot::Sender<Result<Value, String>>)>(64);
        let pending = Arc::new(Mutex::new(HashMap::<u64, oneshot::Sender<Result<Value, String>>>::new()));
        let pending_clone = Arc::clone(&pending);

        // Reader loop
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(resp) = serde_json::from_str::<RpcResponse>(&line) {
                    if let Some(id) = resp.id {
                        let mut map = pending_clone.lock().await;
                        if let Some(sender) = map.remove(&id) {
                            if let Some(err) = resp.error {
                                let _ = sender.send(Err(err));
                            } else if let Some(res) = resp.result {
                                let _ = sender.send(Ok(res));
                            } else {
                                let _ = sender.send(Ok(Value::Null));
                            }
                        }
                    }
                }
            }
        });

        // Writer loop
        let pending_writer = Arc::clone(&pending);
        tokio::spawn(async move {
            while let Some((id, line, sender)) = req_rx.recv().await {
                pending_writer.lock().await.insert(id, sender);
                if stdin.write_all(line.as_bytes()).await.is_err() || stdin.write_all(b"\n").await.is_err() {
                    break;
                }
                let _ = stdin.flush().await;
            }
        });

        Ok(Self {
            tx: req_tx,
            next_id: Arc::new(AtomicU64::new(1)),
        })
    }

    fn find_server_bin() -> Result<PathBuf, String> {
        // 1. Look in the same directory as the hornet executable
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let candidate = parent.join("hornet-server");
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }

        // 2. Relative candidates
        let candidates = [
            PathBuf::from("./bin/hornet-server"),
            PathBuf::from("./hornet-server"),
            PathBuf::from("./engine/bin/hornet-server"),
            PathBuf::from("../bin/hornet-server"),
            PathBuf::from("../engine/bin/hornet-server"),
        ];

        for p in &candidates {
            if p.exists() {
                return Ok(p.clone());
            }
        }

        // 3. Look in PATH
        if let Ok(paths) = std::env::var("PATH") {
            for path in std::env::split_paths(&paths) {
                let bin = path.join("hornet-server");
                if bin.is_file() {
                    return Ok(bin);
                }
            }
        }

        Err("hornet-server binary not found. Please compile it with `make build` or `go build -o bin/hornet-server ./cmd/server` in engine/".to_string())
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = RpcRequest { id, method, params };
        let json_str = serde_json::to_string(&req).map_err(|e| e.to_string())?;

        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send((id, json_str, resp_tx))
            .await
            .map_err(|e| format!("Channel error: {}", e))?;

        resp_rx.await.map_err(|_| "RPC response dropped".to_string())?
    }

    pub async fn list_connections(&self) -> Result<Vec<ConnectionParams>, String> {
        let res = self.call("list_connections", Value::Null).await?;
        serde_json::from_value(res).map_err(|e| e.to_string())
    }

    pub async fn ping(&self, conn_id: &str) -> Result<PingResponse, String> {
        let params = serde_json::json!({ "conn_id": conn_id });
        let res = self.call("ping", params).await?;
        serde_json::from_value(res).map_err(|e| e.to_string())
    }

    pub async fn get_structure(&self, conn_id: &str) -> Result<StructureResponse, String> {
        let params = serde_json::json!({ "conn_id": conn_id });
        let res = self.call("get_structure", params).await?;
        serde_json::from_value(res).map_err(|e| e.to_string())
    }

    pub async fn get_columns(&self, conn_id: &str, schema: &str, table: &str) -> Result<Vec<ColumnInfo>, String> {
        let params = serde_json::json!({
            "conn_id": conn_id,
            "schema": schema,
            "table": table,
        });
        let res = self.call("get_columns", params).await?;
        serde_json::from_value(res).map_err(|e| e.to_string())
    }

    pub async fn execute(&self, conn_id: &str, query: &str) -> Result<QueryResult, String> {
        let params = serde_json::json!({
            "conn_id": conn_id,
            "query": query,
        });
        let res = self.call("execute", params).await?;
        serde_json::from_value(res).map_err(|e| e.to_string())
    }

    pub async fn select_database(&self, conn_id: &str, database: &str) -> Result<StructureResponse, String> {
        let params = serde_json::json!({
            "conn_id": conn_id,
            "database": database,
        });
        let res = self.call("select_database", params).await?;
        serde_json::from_value(res).map_err(|e| e.to_string())
    }

    pub async fn add_connection(&self, name: &str, r#type: &str, url: &str) -> Result<Vec<ConnectionParams>, String> {
        let params = serde_json::json!({
            "name": name,
            "type": r#type,
            "url": url,
        });
        let res = self.call("add_connection", params).await?;
        serde_json::from_value(res).map_err(|e| e.to_string())
    }

    pub async fn list_notes(&self) -> Result<Vec<NoteItem>, String> {
        let res = self.call("list_notes", Value::Null).await?;
        serde_json::from_value(res).map_err(|e| e.to_string())
    }

    pub async fn read_note(&self, path: &str) -> Result<String, String> {
        let params = serde_json::json!({ "path": path });
        let res = self.call("read_note", params).await?;
        serde_json::from_value(res).map_err(|e| e.to_string())
    }

    pub async fn save_note(&self, name: &str, content: &str) -> Result<String, String> {
        let params = serde_json::json!({ "name": name, "content": content });
        let res = self.call("save_note", params).await?;
        serde_json::from_value(res).map_err(|e| e.to_string())
    }
}

