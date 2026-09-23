package config

import (
	"encoding/json"
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"strings"

	"hornet/engine/core"
	"gopkg.in/yaml.v3"
)

type SQLSConfig struct {
	LowercaseColumnNames bool             `yaml:"lowercaseColumnNames"`
	Connections          []SQLSConnection `yaml:"connections"`
}

type SQLSConnection struct {
	Alias  string            `yaml:"alias"`
	Driver string            `yaml:"driver"`
	Proto  string            `yaml:"proto"`
	User   string            `yaml:"user"`
	Passwd string            `yaml:"passwd"`
	Host   string            `yaml:"host"`
	Port   int               `yaml:"port"`
	DBName string            `yaml:"dbName"`
	Path   string            `yaml:"path"`
	Params map[string]string `yaml:"params"`
}

type ConnectionItem struct {
	ID   string `json:"id"`
	URL  string `json:"url"`
	Name string `json:"name"`
	Type string `json:"type"`
}

func DefaultHornetConfigDir() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".config", "hornet")
}

func DefaultHornetConnectionsPath() string {
	return filepath.Join(DefaultHornetConfigDir(), "connections.json")
}

func DefaultSQLSConfigPath() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".config", "sqls", "config.yml")
}

func DefaultNvimDbeePersistencePath() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".local", "state", "nvim", "dbee", "persistence.json")
}

// normalizeKey extracts host+port+dbname for strict deduplication
func normalizeKey(rawURL string) string {
	rawURL = strings.TrimSpace(rawURL)
	if u, err := url.Parse(rawURL); err == nil && u.Host != "" {
		return fmt.Sprintf("%s%s", u.Host, u.Path)
	}
	return rawURL
}

// LoadAllConnections merges connections from:
// 1. ~/.config/hornet/connections.json (primary)
// 2. ~/.local/state/nvim/dbee/persistence.json (fallback for nvim-dbee users)
// 3. ~/.config/sqls/config.yml (fallback for sqls users)
func LoadAllConnections(sqlsPath string) ([]*core.ConnectionParams, error) {
	seen := make(map[string]bool)
	var results []*core.ConnectionParams

	// 1. Load from Hornet native config
	hornetPath := DefaultHornetConnectionsPath()
	if data, err := os.ReadFile(hornetPath); err == nil {
		var items []ConnectionItem
		if err := json.Unmarshal(data, &items); err == nil {
			for _, item := range items {
				if item.Name == "" || item.URL == "" {
					continue
				}
				key := normalizeKey(item.URL)
				if !seen[key] {
					seen[key] = true
					results = append(results, &core.ConnectionParams{
						ID:   core.ConnectionID(item.Name),
						Name: item.Name,
						Type: item.Type,
						URL:  item.URL,
					})
				}
			}
		}
	}

	// 2. Load from nvim-dbee persistence.json fallback
	nvimPath := DefaultNvimDbeePersistencePath()
	if data, err := os.ReadFile(nvimPath); err == nil {
		var items []ConnectionItem
		if err := json.Unmarshal(data, &items); err == nil {
			for _, item := range items {
				if item.Name == "" || item.URL == "" {
					continue
				}
				key := normalizeKey(item.URL)
				if !seen[key] {
					seen[key] = true
					results = append(results, &core.ConnectionParams{
						ID:   core.ConnectionID(item.Name),
						Name: item.Name,
						Type: item.Type,
						URL:  item.URL,
					})
				}
			}
		}
	}

	// 3. Load from sqls config.yml
	if sqlsPath == "" {
		sqlsPath = DefaultSQLSConfigPath()
	}

	if data, err := os.ReadFile(sqlsPath); err == nil {
		var cfg SQLSConfig
		if err := yaml.Unmarshal(data, &cfg); err == nil {
			for _, conn := range cfg.Connections {
				connURL := BuildURL(conn)
				driverType := conn.Driver
				if driverType == "postgresql" || driverType == "postgres" {
					driverType = "postgres"
				}

				alias := conn.Alias
				if alias == "" {
					alias = fmt.Sprintf("%s-%s", conn.Driver, conn.DBName)
				}

				key := normalizeKey(connURL)
				if !seen[key] {
					seen[key] = true
					results = append(results, &core.ConnectionParams{
						ID:   core.ConnectionID(alias),
						Name: alias,
						Type: driverType,
						URL:  connURL,
					})
				}
			}
		}
	}

	return results, nil
}

// BuildURL transforms SQLSConnection struct into connection URI
func BuildURL(c SQLSConnection) string {
	switch c.Driver {
	case "postgresql", "postgres":
		host := c.Host
		if host == "" {
			host = "localhost"
		}
		port := c.Port
		if port == 0 {
			port = 5432
		}

		u := &url.URL{
			Scheme: "postgres",
			Host:   fmt.Sprintf("%s:%d", host, port),
			Path:   "/" + c.DBName,
		}
		if c.User != "" {
			if c.Passwd != "" {
				u.User = url.UserPassword(c.User, c.Passwd)
			} else {
				u.User = url.User(c.User)
			}
		}
		if len(c.Params) > 0 {
			q := u.Query()
			for k, v := range c.Params {
				q.Set(k, v)
			}
			u.RawQuery = q.Encode()
		}
		return u.String()

	case "mysql":
		host := c.Host
		if host == "" {
			host = "localhost"
		}
		port := c.Port
		if port == 0 {
			port = 3306
		}
		auth := c.User
		if c.Passwd != "" {
			auth += ":" + c.Passwd
		}
		if auth != "" {
			auth += "@"
		}
		return fmt.Sprintf("%stcp(%s:%d)/%s", auth, host, port, c.DBName)

	case "sqlite", "sqlite3":
		if c.Path != "" {
			return c.Path
		}
		return c.DBName

	default:
		if c.Path != "" {
			return c.Path
		}
		return c.Host
	}
}

// SavePersistenceConnection saves or updates a connection in ~/.config/hornet/connections.json
// and also syncs to ~/.local/state/nvim/dbee/persistence.json if the directory exists
func SavePersistenceConnection(param *core.ConnectionParams) error {
	hornetPath := DefaultHornetConnectionsPath()
	_ = os.MkdirAll(filepath.Dir(hornetPath), 0755)

	saveToFile := func(targetPath string) error {
		var items []ConnectionItem
		if data, err := os.ReadFile(targetPath); err == nil {
			_ = json.Unmarshal(data, &items)
		}

		found := false
		for i, item := range items {
			if item.Name == param.Name || item.ID == string(param.ID) {
				items[i] = ConnectionItem{
					ID:   string(param.ID),
					URL:  param.URL,
					Name: param.Name,
					Type: param.Type,
				}
				found = true
				break
			}
		}

		if !found {
			id := string(param.ID)
			if id == "" {
				id = fmt.Sprintf("conn_%s", param.Name)
			}
			items = append(items, ConnectionItem{
				ID:   id,
				URL:  param.URL,
				Name: param.Name,
				Type: param.Type,
			})
		}

		data, err := json.MarshalIndent(items, "", "  ")
		if err != nil {
			return err
		}
		return os.WriteFile(targetPath, data, 0644)
	}

	// Always save to Hornet native config
	if err := saveToFile(hornetPath); err != nil {
		return err
	}

	// Also sync to nvim-dbee if that dir exists
	nvimPath := DefaultNvimDbeePersistencePath()
	if _, err := os.Stat(filepath.Dir(nvimPath)); err == nil {
		_ = saveToFile(nvimPath)
	}

	return nil
}
