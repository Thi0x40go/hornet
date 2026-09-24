use crate::config;
use crate::models::*;
use sqlx::mysql::{MySqlPool, MySqlPoolOptions, MySqlRow};
use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions, SqliteRow};
use sqlx::{Column, Row, TypeInfo, ValueRef};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub enum DbPool {
    Postgres(PgPool),
    MySql(MySqlPool),
    Sqlite(SqlitePool),
}

pub struct DbManager {
    pub pools: HashMap<String, DbPool>,
    pub connections: HashMap<String, ConnectionParams>,
}

impl DbManager {
    pub fn new() -> Self {
        Self {
            pools: HashMap::new(),
            connections: HashMap::new(),
        }
    }

    pub fn list_connections(&mut self) -> Result<Vec<ConnectionParams>, String> {
        let conns = config::load_all_connections();
        self.connections.clear();
        for c in &conns {
            self.connections.insert(c.id.clone(), c.clone());
        }
        Ok(conns)
    }

    pub async fn add_connection(&mut self, name: &str, r#type: &str, url_str: &str) -> Result<Vec<ConnectionParams>, String> {
        let param = ConnectionParams {
            id: name.to_string(),
            name: name.to_string(),
            r#type: r#type.to_string(),
            url: url_str.to_string(),
        };

        // 1. Test connection with a short timeout pool
        Self::create_pool(r#type, url_str).await?;

        // 2. Persist to ~/.config/hornet/connections.json
        config::save_connection(&param)?;

        // 3. Drop any existing cached pool for this connection ID to force fresh connection
        self.pools.remove(name);
        self.connections.insert(name.to_string(), param);

        self.list_connections()
    }

    pub fn delete_connection(&mut self, name: &str) -> Result<Vec<ConnectionParams>, String> {
        self.pools.remove(name);
        self.connections.remove(name);
        config::delete_connection(name)?;
        self.list_connections()
    }

    async fn create_pool(r#type: &str, url_str: &str) -> Result<DbPool, String> {
        let driver = r#type.to_lowercase();
        match driver.as_str() {
            "postgres" | "postgresql" => {
                let pool = PgPoolOptions::new()
                    .max_connections(5)
                    .acquire_timeout(Duration::from_secs(5))
                    .connect(url_str)
                    .await
                    .map_err(|e| format!("PostgreSQL connection failed: {}", e))?;
                Ok(DbPool::Postgres(pool))
            }
            "mysql" | "mariadb" => {
                let pool = MySqlPoolOptions::new()
                    .max_connections(5)
                    .acquire_timeout(Duration::from_secs(5))
                    .connect(url_str)
                    .await
                    .map_err(|e| format!("MySQL connection failed: {}", e))?;
                Ok(DbPool::MySql(pool))
            }
            "sqlite" | "sqlite3" => {
                let sqlite_url = if url_str.starts_with("sqlite:") {
                    url_str.to_string()
                } else {
                    format!("sqlite://{}", url_str)
                };
                let pool = SqlitePoolOptions::new()
                    .max_connections(5)
                    .connect(&sqlite_url)
                    .await
                    .map_err(|e| format!("SQLite connection failed: {}", e))?;
                Ok(DbPool::Sqlite(pool))
            }
            other => Err(format!("Unsupported database driver: '{}'. Supported: postgres, mysql, sqlite", other)),
        }
    }

    pub async fn ensure_connected(&mut self, conn_id: &str) -> Result<(), String> {
        if self.pools.contains_key(conn_id) {
            return Ok(());
        }

        let param = self.connections.get(conn_id)
            .cloned()
            .ok_or_else(|| format!("Connection '{}' not found", conn_id))?;

        let pool = Self::create_pool(&param.r#type, &param.url).await?;
        self.pools.insert(conn_id.to_string(), pool);
        Ok(())
    }

    pub async fn ping(&mut self, conn_id: &str) -> Result<PingResponse, String> {
        self.ensure_connected(conn_id).await?;
        let pool = self.pools.get(conn_id).ok_or("Pool not found")?;

        let start = Instant::now();
        match pool {
            DbPool::Postgres(p) => {
                sqlx::query("SELECT 1;").fetch_one(p).await.map_err(|e| e.to_string())?;
            }
            DbPool::MySql(p) => {
                sqlx::query("SELECT 1;").fetch_one(p).await.map_err(|e| e.to_string())?;
            }
            DbPool::Sqlite(p) => {
                sqlx::query("SELECT 1;").fetch_one(p).await.map_err(|e| e.to_string())?;
            }
        }

        Ok(PingResponse {
            online: true,
            latency_ms: start.elapsed().as_millis() as i64,
        })
    }

    pub async fn list_databases(&mut self, conn_id: &str) -> Result<(String, Vec<String>), String> {
        self.ensure_connected(conn_id).await?;
        let pool = self.pools.get(conn_id).ok_or("Pool not found")?;

        match pool {
            DbPool::Postgres(p) => {
                let cur_row = sqlx::query("SELECT current_database();")
                    .fetch_one(p)
                    .await
                    .map_err(|e| e.to_string())?;
                let current_db: String = cur_row.try_get(0).unwrap_or_else(|_| "postgres".to_string());

                let rows = sqlx::query("SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname;")
                    .fetch_all(p)
                    .await
                    .map_err(|e| e.to_string())?;

                let mut avail_dbs = Vec::new();
                for r in rows {
                    if let Ok(name) = r.try_get::<String, _>(0) {
                        avail_dbs.push(name);
                    }
                }
                Ok((current_db, avail_dbs))
            }
            DbPool::MySql(p) => {
                let cur_row = sqlx::query("SELECT DATABASE();")
                    .fetch_one(p)
                    .await
                    .map_err(|e| e.to_string())?;
                let current_db: String = cur_row.try_get(0).unwrap_or_else(|_| "default".to_string());

                let rows = sqlx::query("SHOW DATABASES;")
                    .fetch_all(p)
                    .await
                    .map_err(|e| e.to_string())?;

                let mut avail_dbs = Vec::new();
                for r in rows {
                    if let Ok(name) = r.try_get::<String, _>(0) {
                        avail_dbs.push(name);
                    }
                }
                Ok((current_db, avail_dbs))
            }
            DbPool::Sqlite(_) => {
                Ok(("main".to_string(), vec!["main".to_string()]))
            }
        }
    }

    pub async fn select_database(&mut self, conn_id: &str, database: &str) -> Result<StructureResponse, String> {
        let param = self.connections.get(conn_id)
            .cloned()
            .ok_or_else(|| format!("Connection '{}' not found", conn_id))?;

        let new_url = match param.r#type.as_str() {
            "postgres" | "postgresql" => {
                if let Ok(mut u) = url::Url::parse(&param.url) {
                    u.set_path(&format!("/{}", database));
                    u.to_string()
                } else {
                    param.url.clone()
                }
            }
            "mysql" => {
                if let Ok(mut u) = url::Url::parse(&param.url) {
                    u.set_path(&format!("/{}", database));
                    u.to_string()
                } else {
                    param.url.clone()
                }
            }
            _ => param.url.clone(),
        };

        // Close old pool
        self.pools.remove(conn_id);

        // Update connection URL in memory
        let mut updated_param = param;
        updated_param.url = new_url.clone();
        self.connections.insert(conn_id.to_string(), updated_param);

        // Create new pool
        let new_pool = Self::create_pool(&self.connections[conn_id].r#type, &new_url).await?;
        self.pools.insert(conn_id.to_string(), new_pool);

        self.get_structure(conn_id).await
    }

    pub async fn get_structure(&mut self, conn_id: &str) -> Result<StructureResponse, String> {
        let (current_db, available_dbs) = self.list_databases(conn_id).await?;
        let pool = self.pools.get(conn_id).ok_or("Pool not found")?;

        let mut structures = Vec::new();

        match pool {
            DbPool::Postgres(p) => {
                let sql = "SELECT n.nspname AS table_schema, \
                                  c.relname AS table_name, \
                                  CASE c.relkind \
                                      WHEN 'v' THEN 'VIEW' \
                                      WHEN 'm' THEN 'VIEW' \
                                      ELSE 'BASE TABLE' \
                                  END AS table_type \
                           FROM pg_class c \
                           JOIN pg_namespace n ON n.oid = c.relnamespace \
                           WHERE n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast') \
                             AND c.relkind IN ('r', 'v', 'm', 'p') \
                           ORDER BY n.nspname, c.relname;";

                let rows = sqlx::query(sql)
                    .fetch_all(p)
                    .await
                    .map_err(|e| e.to_string())?;

                let mut schema_map: HashMap<String, Vec<StructureItem>> = HashMap::new();

                for r in rows {
                    let schema: String = r.try_get("table_schema").unwrap_or_else(|_| "public".to_string());
                    let name: String = r.try_get("table_name").unwrap_or_default();
                    let t_type: String = r.try_get("table_type").unwrap_or_default();
                    let item_type = if t_type == "VIEW" { 2 } else { 1 };

                    schema_map.entry(schema.clone()).or_default().push(StructureItem {
                        name,
                        schema: Some(schema),
                        r#type: item_type,
                        children: None,
                    });
                }

                for (schema, tables) in schema_map {
                    structures.push(StructureItem {
                        name: schema,
                        schema: None,
                        r#type: 0,
                        children: Some(tables),
                    });
                }
            }
            DbPool::MySql(p) => {
                let sql = "SELECT table_schema, table_name, table_type \
                           FROM information_schema.tables \
                           WHERE table_schema = DATABASE() \
                           ORDER BY table_name;";

                let rows = sqlx::query(sql)
                    .fetch_all(p)
                    .await
                    .map_err(|e| e.to_string())?;

                let mut tables = Vec::new();
                for r in rows {
                    let schema: String = r.try_get("table_schema").unwrap_or_else(|_| "default".to_string());
                    let name: String = r.try_get("table_name").unwrap_or_default();
                    let t_type: String = r.try_get("table_type").unwrap_or_default();
                    let item_type = if t_type == "VIEW" { 2 } else { 1 };

                    tables.push(StructureItem {
                        name,
                        schema: Some(schema),
                        r#type: item_type,
                        children: None,
                    });
                }

                structures.push(StructureItem {
                    name: current_db.clone(),
                    schema: None,
                    r#type: 0,
                    children: Some(tables),
                });
            }
            DbPool::Sqlite(p) => {
                let sql = "SELECT 'main' as table_schema, name as table_name, type as table_type \
                           FROM sqlite_master \
                           WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' \
                           ORDER BY name;";

                let rows = sqlx::query(sql)
                    .fetch_all(p)
                    .await
                    .map_err(|e| e.to_string())?;

                let mut tables = Vec::new();
                for r in rows {
                    let name: String = r.try_get("table_name").unwrap_or_default();
                    let t_type: String = r.try_get("table_type").unwrap_or_default();
                    let item_type = if t_type == "view" { 2 } else { 1 };

                    tables.push(StructureItem {
                        name,
                        schema: Some("main".to_string()),
                        r#type: item_type,
                        children: None,
                    });
                }

                structures.push(StructureItem {
                    name: "main".to_string(),
                    schema: None,
                    r#type: 0,
                    children: Some(tables),
                });
            }
        }

        // Sort schemas by name, with "public" or current_db first
        structures.sort_by(|a, b| {
            if a.name == "public" {
                std::cmp::Ordering::Less
            } else if b.name == "public" {
                std::cmp::Ordering::Greater
            } else {
                a.name.cmp(&b.name)
            }
        });

        Ok(StructureResponse {
            current_db,
            available_dbs,
            structures,
        })
    }

    pub async fn get_columns(&mut self, conn_id: &str, schema: &str, table: &str) -> Result<Vec<ColumnInfo>, String> {
        self.ensure_connected(conn_id).await?;
        let pool = self.pools.get(conn_id).ok_or("Pool not found")?;

        let mut cols = Vec::new();

        match pool {
            DbPool::Postgres(p) => {
                let sql = "SELECT a.attname AS column_name, \
                                  format_type(a.atttypid, a.atttypmod) AS data_type \
                           FROM pg_attribute a \
                           JOIN pg_class c ON c.oid = a.attrelid \
                           JOIN pg_namespace n ON n.oid = c.relnamespace \
                           WHERE n.nspname = $1 AND c.relname = $2 \
                             AND a.attnum > 0 AND NOT a.attisdropped \
                           ORDER BY a.attnum;";

                let rows = sqlx::query(sql)
                    .bind(schema)
                    .bind(table)
                    .fetch_all(p)
                    .await
                    .map_err(|e| e.to_string())?;

                for r in rows {
                    let name: String = r.try_get("column_name").unwrap_or_default();
                    let r#type: String = r.try_get("data_type").unwrap_or_default();
                    cols.push(ColumnInfo { name, r#type });
                }
            }
            DbPool::MySql(p) => {
                let sql = "SELECT column_name, data_type \
                           FROM information_schema.columns \
                           WHERE table_schema = DATABASE() AND table_name = ? \
                           ORDER BY ordinal_position;";

                let rows = sqlx::query(sql)
                    .bind(table)
                    .fetch_all(p)
                    .await
                    .map_err(|e| e.to_string())?;

                for r in rows {
                    let name: String = r.try_get("column_name").unwrap_or_default();
                    let r#type: String = r.try_get("data_type").unwrap_or_default();
                    cols.push(ColumnInfo { name, r#type });
                }
            }
            DbPool::Sqlite(p) => {
                let sql = format!("PRAGMA table_info('{}');", table.replace('\'', "''"));
                let rows = sqlx::query(&sql)
                    .fetch_all(p)
                    .await
                    .map_err(|e| e.to_string())?;

                for r in rows {
                    let name: String = r.try_get("name").unwrap_or_default();
                    let r#type: String = r.try_get("type").unwrap_or_default();
                    cols.push(ColumnInfo { name, r#type });
                }
            }
        }

        Ok(cols)
    }

    pub async fn execute(&mut self, conn_id: &str, query: &str) -> Result<QueryResult, String> {
        self.ensure_connected(conn_id).await?;
        let pool = self.pools.get(conn_id).ok_or("Pool not found")?;

        let start = Instant::now();

        match pool {
            DbPool::Postgres(p) => {
                let rows = sqlx::query(query)
                    .fetch_all(p)
                    .await
                    .map_err(|e| e.to_string())?;

                let dur = start.elapsed().as_millis() as i64;
                let mut headers = Vec::new();
                if let Some(first) = rows.first() {
                    for col in first.columns() {
                        headers.push(col.name().to_string());
                    }
                }

                let mut result_rows = Vec::with_capacity(rows.len());
                for r in &rows {
                    let mut row_vals = Vec::with_capacity(headers.len());
                    for i in 0..headers.len() {
                        row_vals.push(format_pg_value(r, i));
                    }
                    result_rows.push(row_vals);
                }

                Ok(QueryResult {
                    headers,
                    rows: result_rows,
                    duration_ms: dur,
                    total_rows: rows.len(),
                })
            }
            DbPool::MySql(p) => {
                let rows = sqlx::query(query)
                    .fetch_all(p)
                    .await
                    .map_err(|e| e.to_string())?;

                let dur = start.elapsed().as_millis() as i64;
                let mut headers = Vec::new();
                if let Some(first) = rows.first() {
                    for col in first.columns() {
                        headers.push(col.name().to_string());
                    }
                }

                let mut result_rows = Vec::with_capacity(rows.len());
                for r in &rows {
                    let mut row_vals = Vec::with_capacity(headers.len());
                    for i in 0..headers.len() {
                        row_vals.push(format_mysql_value(r, i));
                    }
                    result_rows.push(row_vals);
                }

                Ok(QueryResult {
                    headers,
                    rows: result_rows,
                    duration_ms: dur,
                    total_rows: rows.len(),
                })
            }
            DbPool::Sqlite(p) => {
                let rows = sqlx::query(query)
                    .fetch_all(p)
                    .await
                    .map_err(|e| e.to_string())?;

                let dur = start.elapsed().as_millis() as i64;
                let mut headers = Vec::new();
                if let Some(first) = rows.first() {
                    for col in first.columns() {
                        headers.push(col.name().to_string());
                    }
                }

                let mut result_rows = Vec::with_capacity(rows.len());
                for r in &rows {
                    let mut row_vals = Vec::with_capacity(headers.len());
                    for i in 0..headers.len() {
                        row_vals.push(format_sqlite_value(r, i));
                    }
                    result_rows.push(row_vals);
                }

                Ok(QueryResult {
                    headers,
                    rows: result_rows,
                    duration_ms: dur,
                    total_rows: rows.len(),
                })
            }
        }
    }
}

fn format_pg_value(row: &PgRow, idx: usize) -> String {
    let raw = match row.try_get_raw(idx) {
        Ok(r) => r,
        Err(_) => return "NULL".to_string(),
    };
    if raw.is_null() {
        return "NULL".to_string();
    }

    let type_name = row.column(idx).type_info().name();
    match type_name {
        "BOOL" | "BOOLEAN" => row.try_get::<bool, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string()),
        "INT2" | "SMALLINT" => row.try_get::<i16, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string()),
        "INT4" | "INT" | "INTEGER" => row.try_get::<i32, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string()),
        "INT8" | "BIGINT" => row.try_get::<i64, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string()),
        "OID" => row.try_get::<sqlx::postgres::types::Oid, _>(idx).map(|v| v.0.to_string()).unwrap_or_else(|_| "ERR".to_string()),
        "FLOAT4" | "REAL" => row.try_get::<f32, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string()),
        "FLOAT8" | "DOUBLE PRECISION" => row.try_get::<f64, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string()),
        "NUMERIC" | "DECIMAL" => {
            row.try_get::<rust_decimal::Decimal, _>(idx)
                .map(|v| v.to_string())
                .or_else(|_| row.try_get::<f64, _>(idx).map(|v| v.to_string()))
                .unwrap_or_else(|_| "ERR".to_string())
        }
        "TEXT" | "VARCHAR" | "CHAR" | "BPCHAR" | "NAME" | "CITEXT" => {
            row.try_get::<String, _>(idx).unwrap_or_else(|_| "ERR".to_string())
        }
        "JSON" | "JSONB" => {
            row.try_get::<serde_json::Value, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string())
        }
        "UUID" => {
            row.try_get::<uuid::Uuid, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string())
        }
        "DATE" => {
            row.try_get::<chrono::NaiveDate, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string())
        }
        "TIME" => {
            row.try_get::<chrono::NaiveTime, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string())
        }
        "TIMESTAMP" => {
            row.try_get::<chrono::NaiveDateTime, _>(idx).map(|v| v.to_string())
                .or_else(|_| row.try_get::<chrono::DateTime<chrono::Utc>, _>(idx).map(|v| v.to_string()))
                .unwrap_or_else(|_| "ERR".to_string())
        }
        "TIMESTAMPTZ" => {
            row.try_get::<chrono::DateTime<chrono::Utc>, _>(idx).map(|v| v.to_string())
                .or_else(|_| row.try_get::<chrono::NaiveDateTime, _>(idx).map(|v| v.to_string()))
                .unwrap_or_else(|_| "ERR".to_string())
        }
        "BYTEA" => {
            row.try_get::<Vec<u8>, _>(idx)
                .map(|v| format!("\\x{}", v.iter().map(|b| format!("{:02x}", b)).collect::<String>()))
                .unwrap_or_else(|_| "ERR".to_string())
        }
        _ => {
            row.try_get::<String, _>(idx)
                .or_else(|_| row.try_get::<rust_decimal::Decimal, _>(idx).map(|v| v.to_string()))
                .or_else(|_| row.try_get::<i64, _>(idx).map(|v| v.to_string()))
                .unwrap_or_else(|_| format!("<{}>", type_name))
        }
    }
}

fn format_mysql_value(row: &MySqlRow, idx: usize) -> String {
    let raw = match row.try_get_raw(idx) {
        Ok(r) => r,
        Err(_) => return "NULL".to_string(),
    };
    if raw.is_null() {
        return "NULL".to_string();
    }

    let type_name = row.column(idx).type_info().name();
    match type_name {
        "BOOLEAN" | "TINYINT(1)" => row.try_get::<bool, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string()),
        "TINYINT" => row.try_get::<i8, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string()),
        "SMALLINT" => row.try_get::<i16, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string()),
        "INT" | "INTEGER" | "MEDIUMINT" => row.try_get::<i32, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string()),
        "BIGINT" => row.try_get::<i64, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string()),
        "FLOAT" => row.try_get::<f32, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string()),
        "DOUBLE" => row.try_get::<f64, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string()),
        "DECIMAL" | "NEWDECIMAL" | "NUMERIC" => {
            row.try_get::<rust_decimal::Decimal, _>(idx)
                .map(|v| v.to_string())
                .or_else(|_| row.try_get::<f64, _>(idx).map(|v| v.to_string()))
                .unwrap_or_else(|_| "ERR".to_string())
        }
        "VARCHAR" | "CHAR" | "TEXT" | "TINYTEXT" | "MEDIUMTEXT" | "LONGTEXT" => {
            row.try_get::<String, _>(idx).unwrap_or_else(|_| "ERR".to_string())
        }
        "JSON" => {
            row.try_get::<serde_json::Value, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string())
        }
        "DATE" => {
            row.try_get::<chrono::NaiveDate, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string())
        }
        "TIME" => {
            row.try_get::<chrono::NaiveTime, _>(idx).map(|v| v.to_string()).unwrap_or_else(|_| "ERR".to_string())
        }
        "DATETIME" | "TIMESTAMP" => {
            row.try_get::<chrono::NaiveDateTime, _>(idx).map(|v| v.to_string())
                .or_else(|_| row.try_get::<chrono::DateTime<chrono::Utc>, _>(idx).map(|v| v.to_string()))
                .unwrap_or_else(|_| "ERR".to_string())
        }
        _ => {
            row.try_get::<String, _>(idx)
                .or_else(|_| row.try_get::<rust_decimal::Decimal, _>(idx).map(|v| v.to_string()))
                .or_else(|_| row.try_get::<i64, _>(idx).map(|v| v.to_string()))
                .unwrap_or_else(|_| format!("<{}>", type_name))
        }
    }
}

fn format_sqlite_value(row: &SqliteRow, idx: usize) -> String {
    let raw = match row.try_get_raw(idx) {
        Ok(r) => r,
        Err(_) => return "NULL".to_string(),
    };
    if raw.is_null() {
        return "NULL".to_string();
    }

    row.try_get::<String, _>(idx)
        .or_else(|_| row.try_get::<i64, _>(idx).map(|v| v.to_string()))
        .or_else(|_| row.try_get::<f64, _>(idx).map(|v| v.to_string()))
        .or_else(|_| row.try_get::<bool, _>(idx).map(|v| v.to_string()))
        .unwrap_or_else(|_| "ERR".to_string())
}
