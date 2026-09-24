use crate::models::ConnectionParams;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct DbeePersistenceItem {
    pub id: String,
    pub url: String,
    pub name: String,
    pub r#type: String,
}

#[derive(Debug, Deserialize)]
struct SqlsConfig {
    #[serde(default)]
    connections: Vec<SqlsConnection>,
}

#[derive(Debug, Deserialize)]
struct SqlsConnection {
    #[serde(default)]
    alias: String,
    #[serde(default)]
    driver: String,
    #[serde(default)]
    user: String,
    #[serde(default)]
    passwd: Option<String>,
    #[serde(default)]
    host: String,
    #[serde(default)]
    port: Option<u16>,
    #[serde(rename = "dbName", default)]
    db_name: String,
    #[serde(default)]
    path: String,
}

pub fn default_hornet_config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hornet")
}

pub fn default_hornet_connections_path() -> PathBuf {
    default_hornet_config_dir().join("connections.json")
}

pub fn default_nvim_dbee_persistence_path() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nvim")
        .join("dbee")
        .join("persistence.json")
}

pub fn default_sqls_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sqls")
        .join("config.yml")
}

fn normalize_key(url_str: &str) -> String {
    let trimmed = url_str.trim();
    if let Ok(parsed) = url::Url::parse(trimmed) {
        if let Some(host) = parsed.host_str() {
            return format!("{}{}", host, parsed.path());
        }
    }
    trimmed.to_string()
}

pub fn load_all_connections() -> Vec<ConnectionParams> {
    let hornet_path = default_hornet_connections_path();

    // 1. Primary: if ~/.config/hornet/connections.json exists, it is the authoritative source!
    if hornet_path.exists() {
        if let Ok(data) = fs::read_to_string(&hornet_path) {
            if let Ok(items) = serde_json::from_str::<Vec<DbeePersistenceItem>>(&data) {
                let mut results = Vec::new();
                for item in items {
                    if !item.name.is_empty() && !item.url.is_empty() {
                        results.push(ConnectionParams {
                            id: item.name.clone(),
                            name: item.name,
                            r#type: item.r#type,
                            url: item.url,
                        });
                    }
                }
                return results;
            }
        }
    }

    // 2. Otherwise, seed from ~/.local/state/nvim/dbee/persistence.json and ~/.config/sqls/config.yml
    let mut seen = HashSet::new();
    let mut results = Vec::new();

    let nvim_path = default_nvim_dbee_persistence_path();
    if let Ok(data) = fs::read_to_string(&nvim_path) {
        if let Ok(items) = serde_json::from_str::<Vec<DbeePersistenceItem>>(&data) {
            for item in items {
                if item.name.is_empty() || item.url.is_empty() {
                    continue;
                }
                let key = normalize_key(&item.url);
                if seen.insert(key) {
                    results.push(ConnectionParams {
                        id: item.name.clone(),
                        name: item.name,
                        r#type: item.r#type,
                        url: item.url,
                    });
                }
            }
        }
    }

    let sqls_path = default_sqls_config_path();
    if let Ok(data) = fs::read_to_string(&sqls_path) {
        if let Ok(cfg) = serde_yaml::from_str::<SqlsConfig>(&data) {
            for conn in cfg.connections {
                let driver_type = match conn.driver.as_str() {
                    "postgresql" | "postgres" => "postgres",
                    "mysql" => "mysql",
                    "sqlite" | "sqlite3" => "sqlite",
                    other => other,
                };

                let conn_url = match driver_type {
                    "postgres" => {
                        let host = if conn.host.is_empty() { "localhost" } else { &conn.host };
                        let port = conn.port.unwrap_or(5432);
                        let auth = match &conn.passwd {
                            Some(p) if !p.is_empty() => format!("{}:{}@", conn.user, p),
                            _ if !conn.user.is_empty() => format!("{}@", conn.user),
                            _ => "".to_string(),
                        };
                        format!("postgres://{}{}:{}/{}", auth, host, port, conn.db_name)
                    }
                    "mysql" => {
                        let host = if conn.host.is_empty() { "localhost" } else { &conn.host };
                        let port = conn.port.unwrap_or(3306);
                        let auth = match &conn.passwd {
                            Some(p) if !p.is_empty() => format!("{}:{}", conn.user, p),
                            _ => conn.user.clone(),
                        };
                        format!("mysql://{}@{}:{}/{}", auth, host, port, conn.db_name)
                    }
                    "sqlite" => {
                        if !conn.path.is_empty() {
                            conn.path.clone()
                        } else {
                            conn.db_name.clone()
                        }
                    }
                    _ => conn.path.clone(),
                };

                let alias = if conn.alias.is_empty() {
                    format!("{}-{}", conn.driver, conn.db_name)
                } else {
                    conn.alias
                };

                let key = normalize_key(&conn_url);
                if seen.insert(key) {
                    results.push(ConnectionParams {
                        id: alias.clone(),
                        name: alias,
                        r#type: driver_type.to_string(),
                        url: conn_url,
                    });
                }
            }
        }
    }

    // Auto-save to ~/.config/hornet/connections.json so Hornet has its own local config
    if let Some(parent) = hornet_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let items_to_save: Vec<DbeePersistenceItem> = results
        .iter()
        .map(|p| DbeePersistenceItem {
            id: p.id.clone(),
            name: p.name.clone(),
            url: p.url.clone(),
            r#type: p.r#type.clone(),
        })
        .collect();
    if let Ok(json) = serde_json::to_string_pretty(&items_to_save) {
        let _ = fs::write(&hornet_path, json);
    }

    results
}

pub fn save_connection(param: &ConnectionParams) -> Result<(), String> {
    let hornet_path = default_hornet_connections_path();
    if let Some(parent) = hornet_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let save_to_file = |path: &PathBuf| -> Result<(), String> {
        let mut items = Vec::new();
        if let Ok(data) = fs::read_to_string(path) {
            if let Ok(existing) = serde_json::from_str::<Vec<DbeePersistenceItem>>(&data) {
                items = existing;
            }
        }

        let mut found = false;
        for it in items.iter_mut() {
            if it.name == param.name || it.id == param.id {
                it.id = param.id.clone();
                it.url = param.url.clone();
                it.name = param.name.clone();
                it.r#type = param.r#type.clone();
                found = true;
                break;
            }
        }

        if !found {
            items.push(DbeePersistenceItem {
                id: param.id.clone(),
                url: param.url.clone(),
                name: param.name.clone(),
                r#type: param.r#type.clone(),
            });
        }

        let json = serde_json::to_string_pretty(&items).map_err(|e| e.to_string())?;
        fs::write(path, json).map_err(|e| e.to_string())?;
        Ok(())
    };

    save_to_file(&hornet_path)?;

    // Also sync to nvim-dbee persistence if that directory exists
    let nvim_path = default_nvim_dbee_persistence_path();
    if let Some(parent) = nvim_path.parent() {
        if parent.exists() {
            let _ = save_to_file(&nvim_path);
        }
    }

    Ok(())
}

pub fn delete_connection(name_or_id: &str) -> Result<(), String> {
    let hornet_path = default_hornet_connections_path();
    let mut items = Vec::new();
    if let Ok(data) = fs::read_to_string(&hornet_path) {
        if let Ok(existing) = serde_json::from_str::<Vec<DbeePersistenceItem>>(&data) {
            items = existing;
        }
    }

    items.retain(|it| it.name != name_or_id && it.id != name_or_id);

    if let Some(parent) = hornet_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(&items).map_err(|e| e.to_string())?;
    fs::write(&hornet_path, json).map_err(|e| e.to_string())?;

    // Also sync delete to nvim-dbee persistence if that file exists
    let nvim_path = default_nvim_dbee_persistence_path();
    if nvim_path.exists() {
        let mut nvim_items = Vec::new();
        if let Ok(data) = fs::read_to_string(&nvim_path) {
            if let Ok(existing) = serde_json::from_str::<Vec<DbeePersistenceItem>>(&data) {
                nvim_items = existing;
            }
        }
        nvim_items.retain(|it| it.name != name_or_id && it.id != name_or_id);
        if let Ok(json) = serde_json::to_string_pretty(&nvim_items) {
            let _ = fs::write(&nvim_path, json);
        }
    }

    Ok(())
}
