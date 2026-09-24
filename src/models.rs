use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionParams {
    pub id: String,
    pub name: String,
    pub r#type: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ColumnInfo {
    pub name: String,
    pub r#type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct StructureItem {
    pub name: String,
    pub schema: Option<String>,
    pub r#type: i32,
    pub children: Option<Vec<StructureItem>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructureResponse {
    pub current_db: String,
    pub available_dbs: Vec<String>,
    pub structures: Vec<StructureItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub duration_ms: i64,
    pub total_rows: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NoteItem {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(rename = "FilePath")]
    pub file_path: String,
    #[serde(default)]
    pub namespace: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingResponse {
    pub online: bool,
    pub latency_ms: i64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct HistoryEntry {
    pub conn_id: String,
    pub query: String,
    pub duration_ms: i64,
    pub row_count: usize,
    pub timestamp: chrono::DateTime<chrono::Local>,
    pub error: Option<String>,
}
