use crate::clipboard::copy_text;
use crate::db::DbManager;
use crate::lsp::{CompletionItem, LspClient};
use crate::models::*;
use crate::syntax::highlight_sql;
use crate::theme::Theme;
use crate::vim::{VimMode, VimState};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row,
        Table, TableState,
    },
    Frame,
};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

pub const DRIVER_TYPES: &[(&str, &str, &str)] = &[
    ("postgres", "PostgreSQL", "postgresql://user:password@localhost:5432/dbname?sslmode=disable"),
    ("mysql", "MySQL / MariaDB", "mysql://user:password@localhost:3306/dbname"),
    ("sqlite", "SQLite", "/path/to/database.db"),
    ("sqlserver", "SQL Server (MSSQL)", "sqlserver://user:password@localhost:1433?database=master"),
    ("clickhouse", "ClickHouse", "clickhouse://user:password@localhost:9000/default"),
    ("duckdb", "DuckDB", "/path/to/duck.db"),
    ("oracle", "Oracle", "oracle://user:password@localhost:1521/xe"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusArea {
    Drawer,
    Editor,
    Results,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeType {
    SectionConnections,
    SectionNotes,
    SectionHistory,
    ConnectionNew,
    Connection(String),
    Database { conn_id: String, name: String, is_active: bool },
    Schema { conn_id: String, name: String },
    Table { conn_id: String, schema: String, name: String },
    Column { name: String, r#type: String },
    NoteNew,
    Note { name: String, path: String },
    History { query: String },
}

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub label: String,
    pub node_type: NodeType,
    pub level: usize,
    pub expanded: bool,
    pub children: Vec<TreeNode>,
    pub loaded: bool,
}

pub struct App {
    pub theme: Theme,
    pub focus: FocusArea,
    pub db: DbManager,
    pub lsp: Option<LspClient>,

    // Drawer / Explorer
    pub tree: Vec<TreeNode>,
    pub flat_nodes: Vec<TreeNode>,
    pub drawer_state: ListState,
    pub active_conn_id: String,
    pub conn_health: HashMap<String, (bool, i64)>, // (online, latency_ms)

    // New Connection Modal
    pub show_new_conn_modal: bool,
    pub new_conn_name: String,
    pub new_conn_type_idx: usize,
    pub new_conn_url: String,
    pub new_conn_field: usize,
    pub new_conn_error: Option<String>,
    pub is_testing_conn: bool,

    // Editor & Vim State
    pub editor_lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub scroll_row: usize,
    pub current_note: Option<String>,
    pub vim: VimState,

    // LSP & Completions
    pub local_tables: HashSet<String>,
    pub local_columns: HashSet<String>,
    pub show_completions: bool,
    pub completions: Vec<CompletionItem>,
    pub selected_completion: usize,
    pub completion_prefix: String,

    // Results
    pub query_result: Option<QueryResult>,
    pub results_error: Option<String>,
    pub table_state: TableState,
    pub selected_col: usize,
    pub col_offset: usize,
    pub col_widths: Vec<u16>,
    pub is_executing: bool,

    // Copy Menu
    pub show_copy_menu: bool,
    pub copy_menu_cursor: usize,
    pub toast_msg: Option<String>,
    pub toast_time: Option<Instant>,

    // History
    pub history: Vec<HistoryEntry>,
    pub status_msg: String,
    pub should_quit: bool,
}

impl App {
    pub async fn new(lsp: Option<LspClient>) -> Self {
        let mut app = Self {
            theme: Theme::default(),
            focus: FocusArea::Drawer,
            db: DbManager::new(),
            lsp,
            tree: vec![
                TreeNode {
                    label: "CONNECTIONS".to_string(),
                    node_type: NodeType::SectionConnections,
                    level: 0,
                    expanded: true,
                    children: Vec::new(),
                    loaded: true,
                },
                TreeNode {
                    label: "SAVED SQL".to_string(),
                    node_type: NodeType::SectionNotes,
                    level: 0,
                    expanded: true,
                    children: vec![TreeNode {
                        label: "+ [New Note]".to_string(),
                        node_type: NodeType::NoteNew,
                        level: 1,
                        expanded: false,
                        children: Vec::new(),
                        loaded: true,
                    }],
                    loaded: true,
                },
                TreeNode {
                    label: "RECENT HISTORY".to_string(),
                    node_type: NodeType::SectionHistory,
                    level: 0,
                    expanded: true,
                    children: Vec::new(),
                    loaded: true,
                },
            ],
            flat_nodes: Vec::new(),
            drawer_state: ListState::default(),
            active_conn_id: String::new(),
            conn_health: HashMap::new(),
            show_new_conn_modal: false,
            new_conn_name: String::new(),
            new_conn_type_idx: 0,
            new_conn_url: DRIVER_TYPES[0].2.to_string(),
            new_conn_field: 0,
            new_conn_error: None,
            is_testing_conn: false,
            editor_lines: vec!["SELECT * FROM ".to_string()],
            cursor_row: 0,
            cursor_col: 14,
            scroll_row: 0,
            current_note: None,
            vim: VimState::new(),
            local_tables: HashSet::new(),
            local_columns: HashSet::new(),
            show_completions: false,
            completions: Vec::new(),
            selected_completion: 0,
            completion_prefix: String::new(),
            query_result: None,
            results_error: None,
            table_state: TableState::default(),
            selected_col: 0,
            col_offset: 0,
            col_widths: Vec::new(),
            is_executing: false,
            show_copy_menu: false,
            copy_menu_cursor: 0,
            toast_msg: None,
            toast_time: None,
            history: Vec::new(),
            status_msg: "Initializing Hornet...".to_string(),
            should_quit: false,
        };

        app.init().await;
        app
    }

    pub async fn init(&mut self) {
        self.load_connections().await;
        self.load_notes().await;
        self.rebuild_flat_tree();
        if !self.flat_nodes.is_empty() {
            self.drawer_state.select(Some(1));
        }
    }

    pub fn open_new_conn_modal(&mut self) {
        self.show_new_conn_modal = true;
        self.new_conn_name = String::new();
        self.new_conn_type_idx = 0;
        self.new_conn_url = DRIVER_TYPES[0].2.to_string();
        self.new_conn_field = 0;
        self.new_conn_error = None;
        self.is_testing_conn = false;
    }

    pub async fn submit_new_connection(&mut self) -> Result<(), String> {
        if self.new_conn_name.trim().is_empty() {
            let err = "Connection name cannot be empty".to_string();
            self.new_conn_error = Some(err.clone());
            return Err(err);
        }
        if self.new_conn_url.trim().is_empty() {
            let err = "Connection URL cannot be empty".to_string();
            self.new_conn_error = Some(err.clone());
            return Err(err);
        }
        let driver_type = DRIVER_TYPES[self.new_conn_type_idx].0;
        self.is_testing_conn = true;
        self.new_conn_error = None;
        let conn_name = self.new_conn_name.trim().to_string();
        self.status_msg = format!("Testing and adding connection '{}'...", conn_name);

        match self.db.add_connection(&conn_name, driver_type, self.new_conn_url.trim()).await {
            Ok(conns) => {
                self.is_testing_conn = false;
                self.show_new_conn_modal = false;
                self.set_toast(format!("Connection '{}' added successfully!", conn_name));
                if let Some(created) = conns.iter().find(|c| c.name == conn_name) {
                    self.active_conn_id = created.id.clone();
                }
                self.load_connections().await;
                self.rebuild_flat_tree();
                Ok(())
            }
            Err(e) => {
                self.is_testing_conn = false;
                self.new_conn_error = Some(e.clone());
                self.status_msg = format!("Error adding connection: {}", e);
                Err(e)
            }
        }
    }

    pub async fn load_connections(&mut self) {
        if let Ok(conns) = self.db.list_connections() {
            let mut conn_nodes = vec![TreeNode {
                label: "+ [New Connection]".to_string(),
                node_type: NodeType::ConnectionNew,
                level: 1,
                expanded: false,
                children: Vec::new(),
                loaded: true,
            }];

            for c in &conns {
                let id = c.id.clone();
                // Check health in background
                if let Ok(ping) = self.db.ping(&id).await {
                    self.conn_health.insert(id.clone(), (ping.online, ping.latency_ms));
                } else {
                    self.conn_health.insert(id.clone(), (false, 0));
                }

                if self.active_conn_id.is_empty() {
                    self.active_conn_id = id.clone();
                }

                conn_nodes.push(TreeNode {
                    label: c.name.clone(),
                    node_type: NodeType::Connection(id),
                    level: 1,
                    expanded: false,
                    children: Vec::new(),
                    loaded: false,
                });
            }

            if let Some(conn_sec) = self.tree.get_mut(0) {
                conn_sec.children = conn_nodes;
            }

            if !self.active_conn_id.is_empty() {
                self.load_structure_for_active().await;
            }
        }
    }

    pub fn apply_structure_response(&mut self, conn_id: &str, resp: StructureResponse) {
        self.status_msg = format!("Connected: database '{}' ({} schemas)", resp.current_db, resp.structures.len());
        self.local_tables.clear();
        self.local_columns.clear();

        // Find connection node in tree
        if let Some(conn_sec) = self.tree.get_mut(0) {
            if let Some(conn_node) = conn_sec.children.iter_mut().find(|n| match &n.node_type {
                NodeType::Connection(id) => id == conn_id,
                _ => false,
            }) {
                conn_node.expanded = true;
                conn_node.loaded = true;

                let mut schema_children = Vec::new();
                for s in resp.structures {
                    let schema_name = if s.name.is_empty() { "public".to_string() } else { s.name.clone() };
                    let mut table_nodes = Vec::new();
                    if let Some(children) = s.children {
                        for t in children {
                            self.local_tables.insert(t.name.clone());
                            if schema_name != "public" && !schema_name.is_empty() {
                                self.local_tables.insert(format!("{}.{}", schema_name, t.name));
                            }
                            table_nodes.push(TreeNode {
                                label: t.name.clone(),
                                node_type: NodeType::Table {
                                    conn_id: conn_id.to_string(),
                                    schema: schema_name.clone(),
                                    name: t.name,
                                },
                                level: 4,
                                expanded: false,
                                children: Vec::new(),
                                loaded: false,
                            });
                        }
                    }

                    let is_public = schema_name == "public";
                    schema_children.push(TreeNode {
                        label: schema_name.clone(),
                        node_type: NodeType::Schema {
                            conn_id: conn_id.to_string(),
                            name: schema_name,
                        },
                        level: 3,
                        expanded: is_public,
                        children: table_nodes,
                        loaded: true,
                    });
                }

                // Build list of all databases available on the server
                let mut all_dbs: Vec<String> = resp.available_dbs.clone();
                if !all_dbs.contains(&resp.current_db) && !resp.current_db.is_empty() {
                    all_dbs.insert(0, resp.current_db.clone());
                }
                if all_dbs.is_empty() && !resp.current_db.is_empty() {
                    all_dbs.push(resp.current_db.clone());
                }

                let mut db_nodes = Vec::new();
                for db in all_dbs {
                    let is_active = db == resp.current_db;
                    db_nodes.push(TreeNode {
                        label: db.clone(),
                        node_type: NodeType::Database {
                            conn_id: conn_id.to_string(),
                            name: db,
                            is_active,
                        },
                        level: 2,
                        expanded: is_active,
                        children: if is_active { schema_children.clone() } else { Vec::new() },
                        loaded: is_active,
                    });
                }

                conn_node.children = db_nodes;
            }
        }
        self.rebuild_flat_tree();
    }

    pub async fn load_structure_for_active(&mut self) {
        if self.active_conn_id.is_empty() {
            return;
        }
        let conn_id = self.active_conn_id.clone();
        if let Ok(resp) = self.db.get_structure(&conn_id).await {
            self.apply_structure_response(&conn_id, resp);
        }
    }

    pub async fn load_columns_for_table(&mut self, conn_id: &str, schema: &str, table: &str) {
        if let Ok(cols) = self.db.get_columns(conn_id, schema, table).await {
            let mut col_nodes = Vec::new();
            for c in cols {
                self.local_columns.insert(c.name.clone());
                col_nodes.push(TreeNode {
                    label: format!("{}: {}", c.name, c.r#type),
                    node_type: NodeType::Column {
                        name: c.name,
                        r#type: c.r#type,
                    },
                    level: 5,
                    expanded: false,
                    children: Vec::new(),
                    loaded: true,
                });
            }

            let target = NodeType::Table {
                conn_id: conn_id.to_string(),
                schema: schema.to_string(),
                name: table.to_string(),
            };

            Self::attach_columns_recursive(&mut self.tree, &target, col_nodes);
            self.rebuild_flat_tree();
        }
    }

    fn attach_columns_recursive(nodes: &mut [TreeNode], target: &NodeType, cols: Vec<TreeNode>) -> bool {
        for n in nodes.iter_mut() {
            if &n.node_type == target {
                n.children = cols;
                n.loaded = true;
                n.expanded = true;
                return true;
            }
            if Self::attach_columns_recursive(&mut n.children, target, cols.clone()) {
                return true;
            }
        }
        false
    }

    pub async fn load_notes(&mut self) {
        if let Ok(n_list) = crate::notes::list_notes() {
            if let Some(notes_sec) = self.tree.get_mut(1) {
                let mut note_items = vec![TreeNode {
                    label: "+ [New Note]".to_string(),
                    node_type: NodeType::NoteNew,
                    level: 1,
                    expanded: false,
                    children: Vec::new(),
                    loaded: true,
                }];
                for n in n_list {
                    note_items.push(TreeNode {
                        label: n.name.clone(),
                        node_type: NodeType::Note {
                            name: n.name,
                            path: n.file_path,
                        },
                        level: 1,
                        expanded: false,
                        children: Vec::new(),
                        loaded: true,
                    });
                }
                notes_sec.children = note_items;
                notes_sec.expanded = true;
            }
        }
    }

    pub fn rebuild_flat_tree(&mut self) {
        let mut flat = Vec::new();
        for node in &self.tree {
            Self::flatten_recursive(node, &mut flat);
        }
        self.flat_nodes = flat;
    }

    fn flatten_recursive(node: &TreeNode, out: &mut Vec<TreeNode>) {
        out.push(node.clone());
        if node.expanded {
            for child in &node.children {
                Self::flatten_recursive(child, out);
            }
        }
    }

    pub async fn toggle_selected_node(&mut self) {
        if let Some(idx) = self.drawer_state.selected() {
            if idx < self.flat_nodes.len() {
                let node = self.flat_nodes[idx].clone();
                match &node.node_type {
                    NodeType::Table { conn_id, schema, name } => {
                        if !node.loaded {
                            let cid = conn_id.clone();
                            let s = schema.clone();
                            let n = name.clone();
                            self.load_columns_for_table(&cid, &s, &n).await;
                            return;
                        }
                    }
                    _ => {}
                }

                Self::toggle_recursive(&mut self.tree, &node.node_type);
                self.rebuild_flat_tree();
            }
        }
    }

    fn toggle_recursive(nodes: &mut [TreeNode], target: &NodeType) -> bool {
        for n in nodes.iter_mut() {
            if &n.node_type == target {
                n.expanded = !n.expanded;
                return true;
            }
            if Self::toggle_recursive(&mut n.children, target) {
                return true;
            }
        }
        false
    }

    pub async fn handle_drawer_enter(&mut self) {
        if let Some(idx) = self.drawer_state.selected() {
            if idx < self.flat_nodes.len() {
                let node = self.flat_nodes[idx].clone();
                match node.node_type {
                    NodeType::SectionConnections | NodeType::SectionNotes | NodeType::SectionHistory | NodeType::Schema { .. } => {
                        self.toggle_selected_node().await;
                    }
                    NodeType::ConnectionNew => {
                        self.open_new_conn_modal();
                    }
                    NodeType::Connection(id) => {
                        self.active_conn_id = id;
                        self.load_structure_for_active().await;
                    }
                    NodeType::Database { conn_id, name, is_active } => {
                        if is_active {
                            self.toggle_selected_node().await;
                        } else {
                            self.status_msg = format!("Switching database to '{}'...", name);
                            match self.db.select_database(&conn_id, &name).await {
                                Ok(resp) => {
                                    self.active_conn_id = conn_id.clone();
                                    self.apply_structure_response(&conn_id, resp);
                                    self.set_toast(format!("Switched to database '{}'", name));
                                }
                                Err(e) => {
                                    self.status_msg = format!("Failed to select database: {}", e);
                                    self.set_toast(format!("Error: {}", e));
                                }
                            }
                        }
                    }
                    NodeType::Table { schema, name, .. } => {
                        let table_name = if schema != "public" && !schema.is_empty() {
                            format!("{}.{}", schema, name)
                        } else {
                            name
                        };
                        self.editor_lines = vec![format!("SELECT * FROM {} LIMIT 50;", table_name)];
                        self.cursor_row = 0;
                        self.cursor_col = self.editor_lines[0].len();
                        self.execute_query_scope(ExecScope::All).await;
                    }
                    NodeType::Column { name, .. } => {
                        let mut line = self.editor_lines.get(self.cursor_row).cloned().unwrap_or_default();
                        line.push_str(&format!(" {}", name));
                        self.editor_lines[self.cursor_row] = line;
                    }
                    NodeType::NoteNew => {
                        let name = format!("query_{}", chrono::Local::now().format("%H%M%S"));
                        self.current_note = Some(name);
                        self.editor_lines = vec!["SELECT * FROM ".to_string()];
                        self.cursor_row = 0;
                        self.cursor_col = 14;
                        self.focus = FocusArea::Editor;
                        self.vim.enter_insert(&self.editor_lines, 0, 14);
                    }
                    NodeType::Note { path, name } => {
                        if let Ok(content) = crate::notes::read_note(&path) {
                            self.editor_lines = content.lines().map(|s| s.to_string()).collect();
                            if self.editor_lines.is_empty() {
                                self.editor_lines = vec![String::new()];
                            }
                            self.cursor_row = 0;
                            self.cursor_col = 0;
                            self.current_note = Some(name);
                            self.focus = FocusArea::Editor;
                            self.status_msg = format!("Opened note: {}", path);
                        }
                    }
                    NodeType::History { query } => {
                        self.editor_lines = query.lines().map(|s| s.to_string()).collect();
                        self.focus = FocusArea::Editor;
                    }
                }
            }
        }
    }

    pub async fn trigger_completions(&mut self) {
        let cur_line = self.editor_lines.get(self.cursor_row).cloned().unwrap_or_default();
        let col = self.cursor_col.min(cur_line.len());
        let before_cursor = &cur_line[..col];

        // Find prefix word before cursor
        let prefix: String = before_cursor
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
            .collect::<Vec<char>>()
            .into_iter()
            .rev()
            .collect();

        self.completion_prefix = prefix.clone();
        let lower_prefix = prefix.to_lowercase();
        let mut items = Vec::new();
        let mut seen = HashSet::new();

        // 1. LSP Completions
        if let Some(lsp) = &self.lsp {
            let doc_text = self.editor_lines.join("\n");
            lsp.update_document(&doc_text).await;
            let lsp_items = lsp.get_completions(self.cursor_row, self.cursor_col).await;
            for it in lsp_items {
                if !seen.contains(&it.label) {
                    seen.insert(it.label.clone());
                    items.push(it);
                }
            }
        }

        // 2. Local Database Tables & Columns
        for tbl in &self.local_tables {
            if (prefix.is_empty() || tbl.to_lowercase().starts_with(&lower_prefix)) && !seen.contains(tbl) {
                seen.insert(tbl.clone());
                items.push(CompletionItem {
                    label: tbl.clone(),
                    detail: Some("table".to_string()),
                    insert_text: Some(tbl.clone()),
                });
            }
        }

        for col in &self.local_columns {
            if (prefix.is_empty() || col.to_lowercase().starts_with(&lower_prefix)) && !seen.contains(col) {
                seen.insert(col.clone());
                items.push(CompletionItem {
                    label: col.clone(),
                    detail: Some("column".to_string()),
                    insert_text: Some(col.clone()),
                });
            }
        }

        // 3. Common SQL Keywords
        let keywords = [
            "SELECT", "FROM", "WHERE", "JOIN", "INNER JOIN", "LEFT JOIN", "RIGHT JOIN",
            "GROUP BY", "ORDER BY", "HAVING", "LIMIT", "INSERT INTO", "UPDATE", "DELETE",
            "CREATE TABLE", "DROP TABLE", "ALTER TABLE", "AS", "DISTINCT", "AND", "OR", "NOT",
            "IN", "BETWEEN", "LIKE", "IS NULL", "IS NOT NULL", "COUNT(*)", "SUM", "AVG", "MIN", "MAX",
        ];

        for kw in keywords {
            if (prefix.is_empty() || kw.to_lowercase().starts_with(&lower_prefix)) && !seen.contains(kw) {
                seen.insert(kw.to_string());
                items.push(CompletionItem {
                    label: kw.to_string(),
                    detail: Some("keyword".to_string()),
                    insert_text: Some(kw.to_string()),
                });
            }
        }

        if !items.is_empty() && (!prefix.is_empty() || items.len() < 30) {
            self.completions = items;
            self.selected_completion = 0;
            self.show_completions = true;
        } else {
            self.show_completions = false;
        }
    }

    pub fn insert_completion(&mut self) {
        if let Some(item) = self.completions.get(self.selected_completion) {
            let text_to_insert = item.insert_text.as_ref().unwrap_or(&item.label);
            let cur_line = self.editor_lines.get(self.cursor_row).cloned().unwrap_or_default();
            let col = self.cursor_col.min(cur_line.len());

            let prefix_len = self.completion_prefix.len();
            let start = col.saturating_sub(prefix_len);

            let before = &cur_line[..start];
            let after = &cur_line[col..];

            let new_line = format!("{}{}{}", before, text_to_insert, after);
            self.cursor_col = start + text_to_insert.len();
            self.editor_lines[self.cursor_row] = new_line;
            self.show_completions = false;
        }
    }

    pub async fn execute_query_scope(&mut self, scope: ExecScope) {
        if self.active_conn_id.is_empty() {
            self.results_error = Some("No active database connection".to_string());
            return;
        }

        let (query, desc) = match scope {
            ExecScope::Line => {
                let line = self.editor_lines.get(self.cursor_row).cloned().unwrap_or_default();
                (line.trim().to_string(), format!("line {}", self.cursor_row + 1))
            }
            ExecScope::Statement => {
                let (stmt, s, e) = self.current_statement();
                let desc = if s == e { format!("line {}", s + 1) } else { format!("lines {}-{}", s + 1, e + 1) };
                (stmt, desc)
            }
            ExecScope::All => {
                (self.editor_lines.join("\n").trim().to_string(), format!("all {} lines", self.editor_lines.len()))
            }
        };

        if query.is_empty() {
            self.results_error = Some(format!("No SQL query found in {}", desc));
            return;
        }

        self.is_executing = true;
        self.status_msg = format!("Executing {}...", desc);
        let start = Instant::now();

        match self.db.execute(&self.active_conn_id, &query).await {
            Ok(res) => {
                self.is_executing = false;
                self.results_error = None;

                // Calculate optimal column widths (min 10, max 38, header + content)
                let mut widths = Vec::new();
                for (j, h) in res.headers.iter().enumerate() {
                    let mut max_len = h.len();
                    for r in res.rows.iter().take(60) {
                        if let Some(val) = r.get(j) {
                            if val.len() > max_len {
                                max_len = val.len();
                            }
                        }
                    }
                    widths.push((max_len.clamp(10, 38) + 2) as u16);
                }

                self.col_widths = widths;
                self.status_msg = format!("Executed in {}ms ({} rows)", res.duration_ms, res.total_rows);

                self.history.insert(0, HistoryEntry {
                    conn_id: self.active_conn_id.clone(),
                    query: query.clone(),
                    duration_ms: res.duration_ms,
                    row_count: res.total_rows,
                    timestamp: chrono::Local::now(),
                    error: None,
                });

                // Update history tree node
                if let Some(hist_sec) = self.tree.get_mut(2) {
                    hist_sec.children.insert(0, TreeNode {
                        label: format!("{} ({}ms)", truncate_str(&query.replace('\n', " "), 25), res.duration_ms),
                        node_type: NodeType::History { query: query.clone() },
                        level: 1,
                        expanded: false,
                        children: Vec::new(),
                        loaded: true,
                    });
                }
                self.rebuild_flat_tree();

                self.query_result = Some(res);
                self.table_state.select(Some(0));
                self.selected_col = 0;
                self.col_offset = 0;
            }
            Err(e) => {
                self.is_executing = false;
                self.status_msg = format!("Error: {}", e);
                self.results_error = Some(e.clone());
                self.history.insert(0, HistoryEntry {
                    conn_id: self.active_conn_id.clone(),
                    query,
                    duration_ms: start.elapsed().as_millis() as i64,
                    row_count: 0,
                    timestamp: chrono::Local::now(),
                    error: Some(e),
                });
            }
        }
    }

    pub fn current_statement(&self) -> (String, usize, usize) {
        if self.editor_lines.is_empty() {
            return (String::new(), 0, 0);
        }
        let cur = self.cursor_row.min(self.editor_lines.len() - 1);
        let mut start = cur;
        while start > 0 {
            let prev = self.editor_lines[start - 1].trim();
            if prev.ends_with(';') {
                break;
            }
            start -= 1;
        }

        let mut end = cur;
        while end < self.editor_lines.len() - 1 {
            let current = self.editor_lines[end].trim();
            if current.ends_with(';') {
                break;
            }
            end += 1;
        }

        let stmt = self.editor_lines[start..=end].join("\n").trim().to_string();
        (stmt, start, end)
    }

    pub fn current_cell_value(&self) -> String {
        if let Some(res) = &self.query_result {
            if let Some(row_idx) = self.table_state.selected() {
                if let Some(row) = res.rows.get(row_idx) {
                    if let Some(cell) = row.get(self.selected_col) {
                        return cell.clone();
                    }
                }
            }
        }
        String::new()
    }

    pub fn current_col_name(&self) -> String {
        if let Some(res) = &self.query_result {
            if let Some(h) = res.headers.get(self.selected_col) {
                return h.clone();
            }
        }
        String::new()
    }

    pub fn copy_current_cell(&mut self) {
        let val = self.current_cell_value();
        copy_text(&val);
        let col = self.current_col_name();
        self.set_toast(format!("Copied cell [{}] = \"{}\"", col, truncate_str(&val, 25)));
    }

    pub fn copy_current_row_json(&mut self) {
        if let Some(res) = &self.query_result {
            if let Some(row_idx) = self.table_state.selected() {
                if let Some(row) = res.rows.get(row_idx) {
                    let mut map = serde_json::Map::new();
                    for (i, h) in res.headers.iter().enumerate() {
                        if let Some(val) = row.get(i) {
                            map.insert(h.clone(), serde_json::Value::String(val.clone()));
                        }
                    }
                    let json = serde_json::to_string_pretty(&serde_json::Value::Object(map)).unwrap_or_default();
                    copy_text(&json);
                    self.set_toast(format!("Copied row #{} as JSON", row_idx + 1));
                }
            }
        }
    }

    pub fn copy_current_row_csv(&mut self) {
        if let Some(res) = &self.query_result {
            if let Some(row_idx) = self.table_state.selected() {
                if let Some(row) = res.rows.get(row_idx) {
                    let csv = row.join(",");
                    copy_text(&csv);
                    self.set_toast(format!("Copied row #{} as CSV", row_idx + 1));
                }
            }
        }
    }

    pub fn copy_current_row_sql(&mut self) {
        if let Some(res) = &self.query_result {
            if let Some(row_idx) = self.table_state.selected() {
                if let Some(row) = res.rows.get(row_idx) {
                    let cols = res.headers.join(", ");
                    let vals: Vec<String> = row.iter().map(|v| if v == "NULL" { "NULL".to_string() } else { format!("'{}'", v.replace('\'', "''")) }).collect();
                    let sql = format!("INSERT INTO table_name ({}) VALUES ({});", cols, vals.join(", "));
                    copy_text(&sql);
                    self.set_toast(format!("Copied row #{} as SQL INSERT", row_idx + 1));
                }
            }
        }
    }

    pub fn copy_all_csv(&mut self) {
        if let Some(res) = &self.query_result {
            let mut csv = res.headers.join(",") + "\n";
            for r in &res.rows {
                csv.push_str(&r.join(","));
                csv.push('\n');
            }
            copy_text(&csv);
            self.set_toast(format!("Copied all {} rows as CSV", res.rows.len()));
        }
    }

    pub fn copy_all_json(&mut self) {
        if let Some(res) = &self.query_result {
            let mut list = Vec::new();
            for r in &res.rows {
                let mut map = serde_json::Map::new();
                for (i, h) in res.headers.iter().enumerate() {
                    if let Some(val) = r.get(i) {
                        map.insert(h.clone(), serde_json::Value::String(val.clone()));
                    }
                }
                list.push(serde_json::Value::Object(map));
            }
            let json = serde_json::to_string_pretty(&serde_json::Value::Array(list)).unwrap_or_default();
            copy_text(&json);
            self.set_toast(format!("Copied all {} rows as JSON", res.rows.len()));
        }
    }

    pub fn copy_all_markdown(&mut self) {
        if let Some(res) = &self.query_result {
            let mut md = format!("| {} |\n| {} |\n", res.headers.join(" | "), vec!["---"; res.headers.len()].join(" | "));
            for r in &res.rows {
                md.push_str(&format!("| {} |\n", r.join(" | ")));
            }
            copy_text(&md);
            self.set_toast(format!("Copied all {} rows as Markdown Table", res.rows.len()));
        }
    }

    pub fn set_toast(&mut self, msg: String) {
        self.toast_msg = Some(msg);
        self.toast_time = Some(Instant::now());
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let size = frame.area();

        // Clear toast after 3s
        if let Some(t) = self.toast_time {
            if t.elapsed().as_secs() >= 3 {
                self.toast_msg = None;
                self.toast_time = None;
            }
        }

        // Layout: Main (Horizontal: Drawer | Vertical: Editor + Results) + Bottom Status
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(6), Constraint::Length(1)])
            .split(size);

        let body_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(38), Constraint::Min(20)])
            .split(main_chunks[0]);

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(body_chunks[1]);

        self.render_drawer(frame, body_chunks[0]);
        self.render_editor(frame, right_chunks[0]);
        self.render_results(frame, right_chunks[1]);
        self.render_status_bar(frame, main_chunks[1]);

        if self.show_copy_menu {
            self.render_copy_modal(frame, size);
        }

        if self.show_new_conn_modal {
            self.render_new_conn_modal(frame, size);
        }
    }

    fn render_drawer(&mut self, frame: &mut Frame, area: Rect) {
        let border_color = if self.focus == FocusArea::Drawer { self.theme.border_active } else { self.theme.border_inactive };
        let block = Block::default()
            .title(Span::styled(" 󱃖 Explorer (1) ", Style::default().fg(self.theme.title).add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color));

        let items: Vec<ListItem> = self.flat_nodes.iter().map(|n| {
            let indent = "  ".repeat(n.level);
            let icon = match &n.node_type {
                NodeType::SectionConnections | NodeType::SectionNotes | NodeType::SectionHistory => {
                    if n.expanded { "󰅂 " } else { "󰅃 " }
                }
                NodeType::ConnectionNew => "󰐕 ",
                NodeType::Connection(id) => {
                    if let Some((online, _)) = self.conn_health.get(id) {
                        if *online { "🟢 " } else { "🔴 " }
                    } else {
                        "🟡 "
                    }
                }
                NodeType::Database { is_active, .. } => if *is_active { "󱤝 " } else { "󱤞 " },
                NodeType::Schema { .. } => if n.expanded { "󰉓 " } else { "󰉖 " },
                NodeType::Table { .. } => if n.expanded { "󰓫  " } else { "󰓫 " },
                NodeType::Column { .. } => " ",
                NodeType::NoteNew => "󰎔 ",
                NodeType::Note { .. } => "󰈙 ",
                NodeType::History { .. } => "󰋚 ",
            };

            let style = match &n.node_type {
                NodeType::SectionConnections | NodeType::SectionNotes | NodeType::SectionHistory => {
                    Style::default().fg(self.theme.accent).add_modifier(Modifier::BOLD)
                }
                NodeType::ConnectionNew => Style::default().fg(self.theme.success).add_modifier(Modifier::BOLD),
                NodeType::Connection(id) => if id == &self.active_conn_id {
                    Style::default().fg(self.theme.success).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(self.theme.fg)
                },
                NodeType::Database { is_active, .. } => if *is_active {
                    Style::default().fg(self.theme.warning).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(self.theme.muted)
                },
                NodeType::Schema { .. } => Style::default().fg(self.theme.title),
                NodeType::Table { .. } => Style::default().fg(self.theme.fg),
                NodeType::Column { .. } => Style::default().fg(self.theme.muted),
                NodeType::NoteNew => Style::default().fg(self.theme.success),
                NodeType::Note { .. } => Style::default().fg(self.theme.fg),
                NodeType::History { .. } => Style::default().fg(self.theme.muted),
            };

            ListItem::new(Line::from(vec![
                Span::raw(indent),
                Span::raw(icon),
                Span::styled(n.label.clone(), style),
            ]))
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(self.theme.table_selected_bg).fg(self.theme.table_selected_fg).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ ");

        frame.render_stateful_widget(list, area, &mut self.drawer_state);
    }

    fn render_editor(&mut self, frame: &mut Frame, area: Rect) {
        let border_color = if self.focus == FocusArea::Editor { self.theme.border_active } else { self.theme.border_inactive };

        let mode_badge = match self.vim.mode {
            VimMode::Normal => Span::styled(" [NORMAL] ", Style::default().bg(self.theme.border_active).fg(self.theme.bg).add_modifier(Modifier::BOLD)),
            VimMode::Insert => Span::styled(" [INSERT] ", Style::default().bg(self.theme.success).fg(self.theme.bg).add_modifier(Modifier::BOLD)),
            VimMode::Visual => Span::styled(" [VISUAL] ", Style::default().bg(self.theme.warning).fg(self.theme.bg).add_modifier(Modifier::BOLD)),
            VimMode::VisualLine => Span::styled(" [VISUAL LINE] ", Style::default().bg(self.theme.warning).fg(self.theme.bg).add_modifier(Modifier::BOLD)),
        };

        let mut title_spans = vec![
            Span::styled(" 󰅩 SQL Editor (2) ", Style::default().fg(self.theme.title).add_modifier(Modifier::BOLD)),
            mode_badge,
            Span::raw(" "),
            Span::styled(format!("[Ln {}/{}, Col {}] ", self.cursor_row + 1, self.editor_lines.len(), self.cursor_col + 1), Style::default().fg(self.theme.muted)),
        ];

        if self.focus == FocusArea::Editor {
            let hint = match self.vim.mode {
                VimMode::Normal => "[i: Insert │ v: Visual │ dd: Delete │ yy: Yank │ Enter: Run] ",
                VimMode::Insert => "[Esc: Normal │ Ctrl+Space: Completions] ",
                VimMode::Visual | VimMode::VisualLine => "[d: Cut │ y: Copy │ Esc: Normal] ",
            };
            title_spans.push(Span::styled(hint, Style::default().fg(self.theme.border_active).add_modifier(Modifier::BOLD)));
        }

        let block = Block::default()
            .title(Line::from(title_spans))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let visible_lines = inner.height as usize;
        if self.cursor_row < self.scroll_row {
            self.scroll_row = self.cursor_row;
        }
        if self.cursor_row >= self.scroll_row + visible_lines && visible_lines > 0 {
            self.scroll_row = self.cursor_row - visible_lines + 1;
        }

        let mut lines = Vec::new();
        for (i, l) in self.editor_lines.iter().enumerate().skip(self.scroll_row).take(visible_lines) {
            let line_num = format!("{:2} │ ", i + 1);
            let mut spans = vec![Span::styled(line_num, Style::default().fg(self.theme.muted))];

            if i == self.cursor_row && self.focus == FocusArea::Editor {
                let col = self.cursor_col.min(l.len());
                let before = &l[..col];
                let cursor_char = if col < l.len() { &l[col..col + 1] } else { " " };
                let after = if col < l.len() { &l[col + 1..] } else { "" };

                spans.extend(highlight_sql(before, &self.theme));
                let cursor_style = match self.vim.mode {
                    VimMode::Normal => Style::default().bg(self.theme.border_active).fg(self.theme.bg).add_modifier(Modifier::BOLD),
                    VimMode::Insert => Style::default().bg(self.theme.success).fg(self.theme.bg).add_modifier(Modifier::BOLD),
                    VimMode::Visual | VimMode::VisualLine => Style::default().bg(self.theme.warning).fg(self.theme.bg).add_modifier(Modifier::BOLD),
                };
                spans.push(Span::styled(cursor_char, cursor_style));
                spans.extend(highlight_sql(after, &self.theme));
            } else {
                spans.extend(highlight_sql(l, &self.theme));
            }

            lines.push(Line::from(spans));
        }

        frame.render_widget(Paragraph::new(lines), inner);

        // Render Autocomplete Popup overlay if active
        if self.show_completions && !self.completions.is_empty() {
            let popup_w = 42u16.min(inner.width.saturating_sub(4));
            let popup_h = 7u16.min(inner.height.saturating_sub(2));

            let cursor_y = (self.cursor_row.saturating_sub(self.scroll_row)) as u16;
            let mut popup_y = inner.y + cursor_y + 1;
            if popup_y + popup_h > inner.y + inner.height {
                popup_y = (inner.y + cursor_y).saturating_sub(popup_h);
            }

            let popup_x = (inner.x + 5 + (self.cursor_col as u16)).min(inner.x + inner.width.saturating_sub(popup_w));
            let popup_area = Rect::new(popup_x, popup_y, popup_w, popup_h);

            frame.render_widget(Clear, popup_area);

            let popup_block = Block::default()
                .title(Span::styled(" 󰘦 Suggestions (LSP) ", Style::default().fg(self.theme.title).add_modifier(Modifier::BOLD)))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(self.theme.border_active));

            let items: Vec<ListItem> = self.completions.iter().take(5).enumerate().map(|(i, it)| {
                let is_sel = i == self.selected_completion;
                let detail_str = it.detail.as_deref().unwrap_or("sql");
                let line_style = if is_sel {
                    Style::default().bg(self.theme.table_selected_bg).fg(Color::Rgb(255, 255, 255)).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(self.theme.fg)
                };

                ListItem::new(Line::from(vec![
                    Span::styled(if is_sel { "▶ " } else { "  " }, Style::default().fg(self.theme.border_active)),
                    Span::styled(format!("{:<20} ", it.label), line_style),
                    Span::styled(format!("[{}]", detail_str), Style::default().fg(self.theme.muted)),
                ]))
            }).collect();

            let list = List::new(items).block(popup_block);
            frame.render_widget(list, popup_area);
        }
    }

    fn render_results(&mut self, frame: &mut Frame, area: Rect) {
        let border_color = if self.focus == FocusArea::Results { self.theme.border_active } else { self.theme.border_inactive };

        if let Some(err) = &self.results_error {
            let block = Block::default()
                .title(Span::styled(" 󱃖 Results (3) ", Style::default().fg(self.theme.title).add_modifier(Modifier::BOLD)))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_color));
            let p = Paragraph::new(format!(" ✖ Error: {}", err))
                .style(Style::default().fg(self.theme.error).add_modifier(Modifier::BOLD))
                .block(block);
            frame.render_widget(p, area);
            return;
        }

        if let Some(res) = &self.query_result {
            if res.headers.is_empty() {
                let block = Block::default()
                    .title(Span::styled(" 󱃖 Results (3) ", Style::default().fg(self.theme.title).add_modifier(Modifier::BOLD)))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(border_color));
                let p = Paragraph::new(" Query executed successfully. 0 rows returned.")
                    .style(Style::default().fg(self.theme.muted))
                    .block(block);
                frame.render_widget(p, area);
                return;
            }

            let total_cols = res.headers.len();
            if self.selected_col >= total_cols {
                self.selected_col = total_cols.saturating_sub(1);
            }

            let avail_w = (area.width.saturating_sub(6)) as usize;

            // Ensure selected_col is visible
            if self.selected_col < self.col_offset {
                self.col_offset = self.selected_col;
            }

            // Calculate visible columns starting at col_offset
            let mut end_col = self.col_offset;
            let mut current_w = 0;
            while end_col < total_cols {
                let w = self.col_widths.get(end_col).copied().unwrap_or(15) as usize;
                if current_w + w > avail_w && end_col > self.col_offset {
                    break;
                }
                current_w += w + 1;
                end_col += 1;
            }

            // If selected_col is beyond end_col, advance col_offset
            if self.selected_col >= end_col {
                self.col_offset = self.selected_col.saturating_sub(end_col.saturating_sub(self.col_offset)).saturating_add(1);
                end_col = self.col_offset;
                current_w = 0;
                while end_col < total_cols {
                    let w = self.col_widths.get(end_col).copied().unwrap_or(15) as usize;
                    if current_w + w > avail_w && end_col > self.col_offset {
                        break;
                    }
                    current_w += w + 1;
                    end_col += 1;
                }
            }

            let start_col = self.col_offset;
            let end_col = end_col.min(total_cols).max(start_col + 1);

            let cur_row = self.table_state.selected().map(|r| r + 1).unwrap_or(1);
            let col_name = self.current_col_name();

            let mut title_spans = vec![
                Span::styled(" 󱃖 Results (3) ", Style::default().fg(self.theme.title).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("[Row {}/{} │ Cols {}-{}/{} │ {}: \"{}\" │ {}ms] ",
                        cur_row, res.total_rows, start_col + 1, end_col, total_cols, col_name, truncate_str(&self.current_cell_value(), 18), res.duration_ms),
                    Style::default().fg(self.theme.muted),
                ),
            ];

            if self.focus == FocusArea::Results {
                title_spans.push(Span::styled("[c/Enter: Copy Menu │ h/l: Cols │ y: Copy Cell] ", Style::default().fg(self.theme.border_active).add_modifier(Modifier::BOLD)));
            }

            if let Some(toast) = &self.toast_msg {
                title_spans.push(Span::styled(format!("✓ {} ", toast), Style::default().bg(self.theme.success).fg(self.theme.bg).add_modifier(Modifier::BOLD)));
            }

            let block = Block::default()
                .title(Line::from(title_spans))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_color));

            let visible_headers = &res.headers[start_col..end_col];
            let header_cells = visible_headers.iter().enumerate().map(|(idx, h)| {
                let orig_col = start_col + idx;
                let prefix = if orig_col == self.selected_col && self.focus == FocusArea::Results { "● " } else { "" };
                Cell::from(format!("{}{}", prefix, h))
                    .style(Style::default().fg(self.theme.table_header_fg).bg(self.theme.table_header_bg).add_modifier(Modifier::BOLD))
            });
            let header = Row::new(header_cells).height(1);

            let rows: Vec<Row> = res.rows.iter().map(|r| {
                let cells: Vec<Cell> = (start_col..end_col).map(|orig_col| {
                    let val = r.get(orig_col).map(|s| s.as_str()).unwrap_or("");
                    if val == "NULL" {
                        Cell::from("NULL").style(Style::default().fg(self.theme.muted).add_modifier(Modifier::ITALIC))
                    } else {
                        Cell::from(val).style(Style::default().fg(self.theme.fg))
                    }
                }).collect();
                Row::new(cells)
            }).collect();

            let constraints: Vec<Constraint> = (start_col..end_col)
                .map(|i| Constraint::Length(self.col_widths.get(i).copied().unwrap_or(15)))
                .collect();

            let table = Table::new(rows, constraints)
                .header(header)
                .block(block)
                .row_highlight_style(Style::default().bg(self.theme.table_selected_bg).fg(self.theme.table_selected_fg).add_modifier(Modifier::BOLD))
                .highlight_symbol("▶ ");

            frame.render_stateful_widget(table, area, &mut self.table_state);
        } else {
            let block = Block::default()
                .title(Span::styled(" 󱃖 Results (3) ", Style::default().fg(self.theme.title).add_modifier(Modifier::BOLD)))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_color));
            let p = Paragraph::new(" No query executed yet. Press F5, Ctrl+Enter or select a table in Explorer.")
                .style(Style::default().fg(self.theme.muted))
                .block(block);
            frame.render_widget(p, area);
        }
    }

    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let conn_status = if let Some((online, lat)) = self.conn_health.get(&self.active_conn_id) {
            if *online { format!("🟢 Online ({}ms)", lat) } else { "🔴 Offline".to_string() }
        } else {
            "🔴 Offline".to_string()
        };

        let focus_str = match self.focus {
            FocusArea::Drawer => "1:Drawer",
            FocusArea::Editor => "2:Editor",
            FocusArea::Results => "3:Results",
        };

        let left_spans = vec![
            Span::styled(" DB: ", Style::default().fg(self.theme.warning).add_modifier(Modifier::BOLD)),
            Span::styled(if self.active_conn_id.is_empty() { "No Connection" } else { &self.active_conn_id }, Style::default().fg(self.theme.success)),
            Span::raw(" "),
            Span::styled(conn_status, Style::default().fg(self.theme.success)),
            Span::raw(" │ "),
            Span::styled("Focus: ", Style::default().fg(self.theme.warning).add_modifier(Modifier::BOLD)),
            Span::styled(focus_str, Style::default().fg(self.theme.accent)),
            Span::raw(" │ "),
            Span::raw(&self.status_msg),
        ];

        let right_spans = vec![
            Span::styled("[Tab] Focus │ [Ctrl+Enter/E] Run Stmt │ [i/Esc] Vim Mode │ [F5] Run All ", Style::default().fg(self.theme.muted)),
        ];

        let line = Line::from(left_spans);
        let right_line = Line::from(right_spans);

        let p_left = Paragraph::new(line).style(Style::default().bg(Color::Rgb(31, 35, 53)));
        let p_right = Paragraph::new(right_line).alignment(Alignment::Right).style(Style::default().bg(Color::Rgb(31, 35, 53)));

        frame.render_widget(p_left, area);
        frame.render_widget(p_right, area);
    }

    fn render_copy_modal(&self, frame: &mut Frame, area: Rect) {
        let modal_area = centered_rect(65, 45, area);
        frame.render_widget(Clear, modal_area);

        let block = Block::default()
            .title(Span::styled(" 📋 Copy to Clipboard ", Style::default().fg(self.theme.title).add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(self.theme.border_active));

        let cell_val = truncate_str(&self.current_cell_value(), 25);
        let cur_row = self.table_state.selected().map(|r| r + 1).unwrap_or(1);
        let total_rows = self.query_result.as_ref().map(|r| r.total_rows).unwrap_or(0);

        let items = [
            ("1", "📄", "Copy Selected Cell", format!("[{}] \"{}\"", self.current_col_name(), cell_val)),
            ("2", "📦", "Copy Row as JSON", format!("Row #{} as key-value JSON object", cur_row)),
            ("3", "📊", "Copy Row as CSV", format!("Row #{} comma-separated values", cur_row)),
            ("4", "📝", "Copy Row as SQL INSERT", format!("INSERT INTO table VALUES (...) for row #{}", cur_row)),
            ("5", "📑", "Copy ALL Results as CSV", format!("Entire query result ({} rows)", total_rows)),
            ("6", "🗃️", "Copy ALL Results as JSON", format!("Array of {} JSON objects", total_rows)),
            ("7", "📋", "Copy ALL as Markdown Table", format!("Formatted Markdown table ({} rows)", total_rows)),
        ];

        let mut lines = vec![Line::from("")];
        for (i, (num, icon, name, desc)) in items.iter().enumerate() {
            let is_sel = i == self.copy_menu_cursor;
            let prefix = if is_sel { " ▶ " } else { "   " };
            let name_style = if is_sel {
                Style::default().fg(self.theme.success).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(self.theme.fg)
            };

            lines.push(Line::from(vec![
                Span::styled(prefix, Style::default().fg(self.theme.border_active)),
                Span::styled(format!("[{}] ", num), Style::default().fg(self.theme.warning).add_modifier(Modifier::BOLD)),
                Span::raw(format!("{} ", icon)),
                Span::styled(format!("{:<26} ", name), name_style),
                Span::styled(desc.clone(), Style::default().fg(self.theme.muted)),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("   [Enter / 1-7: Select & Copy │ Esc / q: Close]", Style::default().fg(self.theme.muted))));

        let p = Paragraph::new(lines).block(block);
        frame.render_widget(p, modal_area);
    }

    fn render_new_conn_modal(&self, frame: &mut Frame, area: Rect) {
        let modal_area = centered_rect(65, 55, area);
        frame.render_widget(Clear, modal_area);

        let block = Block::default()
            .title(Span::styled(" 🔌 Add New Database Connection ", Style::default().fg(self.theme.title).add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(self.theme.border_active));

        let inner = block.inner(modal_area);
        frame.render_widget(block, modal_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3), // Name input
                Constraint::Length(3), // Driver selector
                Constraint::Length(3), // URL input
                Constraint::Length(2), // Status/Error message
                Constraint::Length(3), // Buttons
            ])
            .split(inner);

        // 1. Connection Name
        let name_border = if self.new_conn_field == 0 { self.theme.border_active } else { self.theme.border_inactive };
        let name_block = Block::default()
            .title(Span::styled(" Connection Name ", Style::default().fg(if self.new_conn_field == 0 { self.theme.border_active } else { self.theme.fg })))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(name_border));
        let cursor_char = if self.new_conn_field == 0 { "▏" } else { "" };
        let name_p = Paragraph::new(format!(" {}{}", self.new_conn_name, cursor_char))
            .style(Style::default().fg(self.theme.fg))
            .block(name_block);
        frame.render_widget(name_p, chunks[0]);

        // 2. Driver / Type Selector
        let type_border = if self.new_conn_field == 1 { self.theme.border_active } else { self.theme.border_inactive };
        let type_block = Block::default()
            .title(Span::styled(" Database Driver (◄ / ► to change) ", Style::default().fg(if self.new_conn_field == 1 { self.theme.border_active } else { self.theme.fg })))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(type_border));
        let (cur_id, cur_label, _) = DRIVER_TYPES[self.new_conn_type_idx];
        let type_text = format!(" ◀  {} ({})  ▶", cur_label, cur_id);
        let type_p = Paragraph::new(type_text)
            .style(Style::default().fg(self.theme.warning).add_modifier(Modifier::BOLD))
            .block(type_block);
        frame.render_widget(type_p, chunks[1]);

        // 3. Connection URL
        let url_border = if self.new_conn_field == 2 { self.theme.border_active } else { self.theme.border_inactive };
        let url_block = Block::default()
            .title(Span::styled(" Connection URL / URI ", Style::default().fg(if self.new_conn_field == 2 { self.theme.border_active } else { self.theme.fg })))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(url_border));
        let url_cursor = if self.new_conn_field == 2 { "▏" } else { "" };
        let url_p = Paragraph::new(format!(" {}{}", self.new_conn_url, url_cursor))
            .style(Style::default().fg(self.theme.fg))
            .block(url_block);
        frame.render_widget(url_p, chunks[2]);

        // 4. Status / Error hint
        if let Some(err) = &self.new_conn_error {
            let err_p = Paragraph::new(format!(" ✖ {}", err))
                .style(Style::default().fg(self.theme.error).add_modifier(Modifier::BOLD));
            frame.render_widget(err_p, chunks[3]);
        } else if self.is_testing_conn {
            let test_p = Paragraph::new(" ⏳ Testing and establishing connection...")
                .style(Style::default().fg(self.theme.warning).add_modifier(Modifier::BOLD));
            frame.render_widget(test_p, chunks[3]);
        } else {
            let hint_p = Paragraph::new(" [Tab / Shift+Tab] Next/Prev Field │ [Enter] Save & Test │ [Esc] Cancel")
                .style(Style::default().fg(self.theme.muted));
            frame.render_widget(hint_p, chunks[3]);
        }

        // 5. Buttons
        let btn_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[4]);

        let save_style = if self.new_conn_field == 3 {
            Style::default().bg(self.theme.success).fg(self.theme.bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.theme.success)
        };
        let save_btn = Paragraph::new("  [ Enter: Test & Save ]  ")
            .alignment(Alignment::Center)
            .style(save_style)
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(if self.new_conn_field == 3 { self.theme.success } else { self.theme.border_inactive })));
        frame.render_widget(save_btn, btn_chunks[0]);

        let cancel_style = if self.new_conn_field == 4 {
            Style::default().bg(self.theme.error).fg(self.theme.bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.theme.muted)
        };
        let cancel_btn = Paragraph::new("  [ Esc: Cancel ]  ")
            .alignment(Alignment::Center)
            .style(cancel_style)
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(if self.new_conn_field == 4 { self.theme.error } else { self.theme.border_inactive })));
        frame.render_widget(cancel_btn, btn_chunks[1]);
    }
}

pub enum ExecScope {
    All,
    Statement,
    Line,
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}…", &s[..max_len])
    } else {
        s.to_string()
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
