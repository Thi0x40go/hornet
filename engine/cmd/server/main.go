package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"hornet/engine/adapters"
	"hornet/engine/config"
	"hornet/engine/core"
	"hornet/engine/notes"
)

type Request struct {
	ID     any             `json:"id"`
	Method string          `json:"method"`
	Params json.RawMessage `json:"params"`
}

type Response struct {
	ID     any    `json:"id"`
	Result any    `json:"result,omitempty"`
	Error  string `json:"error,omitempty"`
}

type Server struct {
	mu          sync.Mutex
	connections map[core.ConnectionID]*core.Connection
	connParams  map[core.ConnectionID]*core.ConnectionParams
	configPath  string
}

func NewServer() *Server {
	home, _ := os.UserHomeDir()
	return &Server{
		connections: make(map[core.ConnectionID]*core.Connection),
		connParams:  make(map[core.ConnectionID]*core.ConnectionParams),
		configPath:  filepath.Join(home, ".config", "sqls", "config.yml"),
	}
}

func (s *Server) getOrCreateConn(connID core.ConnectionID) (*core.Connection, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	if conn, ok := s.connections[connID]; ok {
		return conn, nil
	}

	param, ok := s.connParams[connID]
	if !ok {
		return nil, fmt.Errorf("connection %q not found", connID)
	}

	conn, err := adapters.NewConnection(param)
	if err != nil {
		return nil, err
	}
	s.connections[connID] = conn
	return conn, nil
}

func (s *Server) Handle(req Request) Response {
	resp := Response{ID: req.ID}

	switch req.Method {
	case "list_connections":
		conns, err := config.LoadAllConnections(s.configPath)
		if err != nil {
			resp.Error = err.Error()
			return resp
		}
		s.mu.Lock()
		for _, c := range conns {
			s.connParams[c.ID] = c
		}
		s.mu.Unlock()
		resp.Result = conns
		return resp

	case "ping":
		var p struct {
			ConnID core.ConnectionID `json:"conn_id"`
		}
		_ = json.Unmarshal(req.Params, &p)
		param, ok := s.connParams[p.ConnID]
		if !ok {
			resp.Error = "connection not found"
			return resp
		}

		start := time.Now()
		c, err := adapters.NewConnection(param)
		if err != nil {
			resp.Error = err.Error()
			return resp
		}
		defer c.Close()

		call := c.Execute("SELECT 1;", nil)
		select {
		case <-call.Done():
			if call.Err() != nil {
				resp.Error = call.Err().Error()
			} else {
				resp.Result = map[string]any{
					"online":     true,
					"latency_ms": time.Since(start).Milliseconds(),
				}
			}
		case <-time.After(3 * time.Second):
			call.Cancel()
			resp.Error = "timeout (3s)"
		}
		return resp

	case "get_structure":
		var p struct {
			ConnID core.ConnectionID `json:"conn_id"`
		}
		_ = json.Unmarshal(req.Params, &p)
		conn, err := s.getOrCreateConn(p.ConnID)
		if err != nil {
			resp.Error = err.Error()
			return resp
		}

		curDB, availDBs, _ := conn.ListDatabases()
		structs, err := conn.GetStructure()
		if err != nil {
			resp.Error = err.Error()
			return resp
		}

		resp.Result = map[string]any{
			"current_db":    curDB,
			"available_dbs": availDBs,
			"structures":    structs,
		}
		return resp

	case "select_database":
		var p struct {
			ConnID   core.ConnectionID `json:"conn_id"`
			Database string            `json:"database"`
		}
		_ = json.Unmarshal(req.Params, &p)
		conn, err := s.getOrCreateConn(p.ConnID)
		if err != nil {
			resp.Error = err.Error()
			return resp
		}

		if err := conn.SelectDatabase(p.Database); err != nil {
			resp.Error = err.Error()
			return resp
		}

		curDB, availDBs, _ := conn.ListDatabases()
		structs, err := conn.GetStructure()
		if err != nil {
			resp.Error = err.Error()
			return resp
		}

		resp.Result = map[string]any{
			"current_db":    curDB,
			"available_dbs": availDBs,
			"structures":    structs,
		}
		return resp

	case "add_connection":
		var p struct {
			Name string `json:"name"`
			Type string `json:"type"`
			URL  string `json:"url"`
		}
		_ = json.Unmarshal(req.Params, &p)
		if p.Name == "" || p.URL == "" {
			resp.Error = "name and url are required"
			return resp
		}
		if p.Type == "" {
			p.Type = "postgres"
		}

		newParam := &core.ConnectionParams{
			ID:   core.ConnectionID(p.Name),
			Name: p.Name,
			Type: p.Type,
			URL:  p.URL,
		}

		// Test connection first
		testConn, err := adapters.NewConnection(newParam)
		if err != nil {
			resp.Error = fmt.Sprintf("failed to connect: %v", err)
			return resp
		}
		testConn.Close()

		// Save to connections.json
		if err := config.SavePersistenceConnection(newParam); err != nil {
			resp.Error = fmt.Sprintf("failed to save connection: %v", err)
			return resp
		}

		// Update in-memory map
		s.mu.Lock()
		s.connParams[newParam.ID] = newParam
		delete(s.connections, newParam.ID)
		s.mu.Unlock()

		// Reload all connections
		conns, err := config.LoadAllConnections(s.configPath)
		if err != nil {
			resp.Error = err.Error()
			return resp
		}
		resp.Result = conns
		return resp

	case "get_columns":
		var p struct {
			ConnID core.ConnectionID `json:"conn_id"`
			Schema string            `json:"schema"`
			Table  string            `json:"table"`
		}
		_ = json.Unmarshal(req.Params, &p)
		conn, err := s.getOrCreateConn(p.ConnID)
		if err != nil {
			resp.Error = err.Error()
			return resp
		}

		cols, err := conn.GetColumns(&core.TableOptions{
			Table:           p.Table,
			Schema:          p.Schema,
			Materialization: core.StructureTypeTable,
		})
		if err != nil {
			resp.Error = err.Error()
			return resp
		}
		resp.Result = cols
		return resp

	case "execute":
		var p struct {
			ConnID core.ConnectionID `json:"conn_id"`
			Query  string            `json:"query"`
		}
		_ = json.Unmarshal(req.Params, &p)
		conn, err := s.getOrCreateConn(p.ConnID)
		if err != nil {
			resp.Error = err.Error()
			return resp
		}

		start := time.Now()
		call := conn.Execute(p.Query, nil)
		if call == nil {
			resp.Error = "failed to create execution call"
			return resp
		}

		<-call.Done()
		dur := time.Since(start)

		if call.Err() != nil {
			resp.Error = call.Err().Error()
			return resp
		}

		res, err := call.GetResult()
		if err != nil {
			resp.Error = err.Error()
			return resp
		}

		headers := res.Header()
		rawRows, err := res.Rows(0, -1)
		if err != nil {
			resp.Error = err.Error()
			return resp
		}

		rows := make([][]string, len(rawRows))
		for i, r := range rawRows {
			rowStr := make([]string, len(r))
			for j, val := range r {
				if val == nil {
					rowStr[j] = "NULL"
				} else {
					rowStr[j] = fmt.Sprintf("%v", val)
				}
			}
			rows[i] = rowStr
		}

		resp.Result = map[string]any{
			"headers":     headers,
			"rows":        rows,
			"duration_ms": dur.Milliseconds(),
			"total_rows":  len(rows),
		}
		return resp

	case "list_notes":
		nList, err := notes.ListGlobalNotes()
		if err != nil {
			resp.Error = err.Error()
			return resp
		}
		resp.Result = nList
		return resp

	case "read_note":
		var p struct {
			Path string `json:"path"`
		}
		_ = json.Unmarshal(req.Params, &p)
		content, err := notes.ReadNote(p.Path)
		if err != nil {
			resp.Error = err.Error()
			return resp
		}
		resp.Result = content
		return resp

	case "save_note":
		var p struct {
			Name    string `json:"name"`
			Content string `json:"content"`
		}
		_ = json.Unmarshal(req.Params, &p)
		savedPath, err := notes.SaveNote(p.Name, p.Content)
		if err != nil {
			resp.Error = err.Error()
			return resp
		}
		resp.Result = savedPath
		return resp

	default:
		resp.Error = fmt.Sprintf("method %q not found", req.Method)
		return resp
	}
}

func main() {
	server := NewServer()
	scanner := bufio.NewScanner(os.Stdin)
	buf := make([]byte, 0, 1024*1024)
	scanner.Buffer(buf, 10*1024*1024)

	encoder := json.NewEncoder(os.Stdout)

	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			continue
		}

		var req Request
		if err := json.Unmarshal([]byte(line), &req); err != nil {
			_ = encoder.Encode(Response{
				ID:    nil,
				Error: "invalid json request: " + err.Error(),
			})
			continue
		}

		resp := server.Handle(req)
		if err := encoder.Encode(resp); err != nil {
			if err == io.EOF {
				break
			}
		}
	}
}
