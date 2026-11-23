// Package databasesnode provides the compile-time native Node database agent
// tools (database_catalog, database_query, database_execute,
// database_connections, database_save_connection) without running JS turn hooks.
package databasesnode

import (
	"context"
	_ "embed"
	"encoding/json"
	"fmt"
	"math"
	"regexp"
	"slices"
	"strings"
	"sync"

	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

//go:embed tools.json
var catalogJSON []byte

var loadCatalog = sync.OnceValues(func() (moduleapi.CatalogSpec, error) {
	return moduleapi.LoadCatalog(catalogJSON)
})

const uuidPattern = `^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$`

var uuidRE = regexp.MustCompile(uuidPattern)

type dbSummary struct {
	ID       string
	Name     string
	Engine   protocol.DatabaseEngine
	Database string
	ReadOnly bool
}

type module struct{}

// Module returns the native Node module providing database connection and query
// tools.
func Module() moduleapi.Module { return module{} }

func (module) ID() string                { return "databases" }
func (module) ReplacesPlugins() []string { return []string{"databases"} }

func (module) Catalog() []moduleapi.ToolMeta {
	catalog, err := loadCatalog()
	if err != nil {
		return nil
	}
	metas := make([]moduleapi.ToolMeta, 0, len(catalog.Tools))
	for _, spec := range catalog.Tools {
		metas = append(metas, spec.Meta())
	}
	return metas
}

func (module) ToolsForTurn(ctx context.Context, host moduleapi.Host, inv *store.AgentInvocation) ([]engine.Tool, error) {
	catalog, err := loadCatalog()
	if err != nil {
		return nil, err
	}
	var available []moduleapi.ToolSpec
	for _, spec := range catalog.Tools {
		if spec.Available(inv) {
			available = append(available, spec)
		}
	}
	if len(available) == 0 {
		return nil, nil
	}
	reachable, err := host.ConnectionCatalog(ctx, inv)
	if err != nil {
		return nil, err
	}
	captured := reachable.Resources()
	host.CaptureConnectionTargets(inv, captured)

	profiles := summarizeDatabases(reachable.Databases)
	state := &turnState{connections: make([]string, 0, len(profiles))}
	for _, profile := range profiles {
		state.connections = append(state.connections, profile.ID)
	}

	tools := make([]engine.Tool, 0, len(available))
	for _, spec := range available {
		params, err := dynamicParameters(spec.Name, profiles)
		if err != nil {
			return nil, err
		}
		spec.Parameters = params
		var prepare func(context.Context, json.RawMessage) (json.RawMessage, error)
		var format func(context.Context, json.RawMessage, engine.ToolResult) engine.ToolResult
		switch spec.Name {
		case "database_connections":
			prepare = state.prepareConnections
		case "database_save_connection":
			prepare = state.prepareSave
			format = state.formatSaved
		case "database_catalog":
			prepare = state.prepareOp(nil)
		case "database_query":
			readOnly := false
			prepare = state.prepareOp(&readOnly)
		case "database_execute":
			write := true
			prepare = state.prepareOp(&write)
		}
		tool, err := host.ConnectionTool(inv, moduleapi.ConnectionToolConfig{
			Spec:         spec,
			Instruction:  catalog.Instructions,
			Captured:     captured,
			PrepareArgs:  prepare,
			FormatResult: format,
		})
		if err != nil {
			return nil, err
		}
		tools = append(tools, tool)
	}
	return tools, nil
}

func summarizeDatabases(profiles []protocol.DatabaseProfile) []dbSummary {
	out := make([]dbSummary, 0, len(profiles))
	for _, profile := range profiles {
		body, err := json.Marshal(profile.Connection)
		if err != nil {
			continue
		}
		var target struct {
			Engine   protocol.DatabaseEngine `json:"engine"`
			Database string                  `json:"database"`
		}
		if json.Unmarshal(body, &target) != nil {
			continue
		}
		if profile.Connection.Kind == "sqlite" {
			target.Engine, target.Database = protocol.DatabaseEngineSqlite, "main"
		}
		out = append(out, dbSummary{
			ID:       profile.ID.String(),
			Name:     profile.Name,
			Engine:   target.Engine,
			Database: target.Database,
			ReadOnly: profile.ReadOnly,
		})
	}
	return out
}

func profileLabel(p dbSummary) string {
	engineName := ""
	quoting := "double-quoted identifiers"
	switch p.Engine {
	case protocol.DatabaseEngineSqlite:
		engineName = "SQLite"
	case protocol.DatabaseEngineMysql:
		engineName = "MySQL"
		quoting = "backtick-quoted identifiers; do not use double quotes for table or database names"
	case protocol.DatabaseEnginePostgres:
		engineName = "PostgreSQL"
		quoting = "double-quoted identifiers; qualify tables with schema, not database"
	}
	return fmt.Sprintf("%s; engine=%s; default_database=%s; read_only=%t; %s", p.Name, engineName, p.Database, p.ReadOnly, quoting)
}

func dynamicParameters(name string, profiles []dbSummary) (json.RawMessage, error) {
	switch name {
	case "database_connections":
		return json.RawMessage(`{"type":"object","properties":{},"required":[],"additionalProperties":false}`), nil
	case "database_save_connection":
		return saveParameters(), nil
	}
	choices := make([][2]string, 0, len(profiles))
	for _, p := range profiles {
		choices = append(choices, [2]string{p.ID, profileLabel(p)})
	}
	choicesJSON, err := json.Marshal(choices)
	if err != nil {
		return nil, err
	}
	properties := map[string]any{
		"connection": map[string]any{"type": "string", "pattern": uuidPattern, "description": string(choicesJSON)},
		"database":   map[string]any{"type": "string", "description": "Database name; omit to use the saved default"},
	}
	required := []string{"connection"}
	if name != "database_catalog" {
		properties["sql"] = map[string]any{"type": "string"}
		required = append(required, "sql")
	}
	return json.Marshal(map[string]any{
		"type": "object", "properties": properties, "required": required, "additionalProperties": false,
	})
}

func saveParameters() json.RawMessage {
	raw, _ := json.Marshal(map[string]any{
		"type": "object", "additionalProperties": false, "required": []string{},
		"properties": map[string]any{
			"connection":        map[string]any{"type": []string{"string", "null"}, "pattern": uuidPattern, "description": "Saved connection ID; omit with expected_revision to create"},
			"expected_revision": map[string]any{"type": []string{"integer", "null"}, "minimum": 1},
			"name":              map[string]any{"type": "string", "minLength": 1},
			"engine":            map[string]any{"type": "string", "enum": []string{"sqlite", "mysql", "postgres"}},
			"path":              map[string]any{"type": "string", "minLength": 1, "description": "SQLite file path on the execution Node; relative paths use the captured worktree. Saving does not create the file"},
			"host":              map[string]any{"type": "string", "minLength": 1},
			"port":              map[string]any{"type": "integer", "minimum": 1, "maximum": 65535},
			"username":          map[string]any{"type": "string", "minLength": 1},
			"database":          map[string]any{"type": "string"},
			"read_only":         map[string]any{"type": "boolean"},
		},
		"allOf": []any{map[string]any{
			"if":   map[string]any{"required": []string{"connection"}, "properties": map[string]any{"connection": map[string]any{"type": "string"}}},
			"then": map[string]any{"required": []string{"expected_revision"}, "properties": map[string]any{"expected_revision": map[string]any{"type": "integer"}}},
			"else": map[string]any{
				"required":   []string{"name", "engine"},
				"properties": map[string]any{"expected_revision": map[string]any{"type": "null"}},
				"allOf": []any{map[string]any{
					"if":   map[string]any{"properties": map[string]any{"engine": map[string]any{"const": "sqlite"}}},
					"then": map[string]any{"required": []string{"path"}},
					"else": map[string]any{"required": []string{"host", "port", "username", "database"}},
				}},
			},
		}},
	})
	return raw
}

type turnState struct {
	mu          sync.Mutex
	connections []string
}

func (s *turnState) has(id string) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return slices.Contains(s.connections, id)
}

func (s *turnState) add(id string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if !slices.Contains(s.connections, id) {
		s.connections = append(s.connections, id)
	}
}

func invalidFault(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}

func decodeJSONObject(raw json.RawMessage) (map[string]any, error) {
	var obj map[string]any
	dec := json.NewDecoder(strings.NewReader(string(raw)))
	dec.UseNumber()
	if err := dec.Decode(&obj); err != nil || obj == nil {
		return nil, invalidFault("Invalid database tool arguments")
	}
	return obj, nil
}

func safeInt(v any) (int64, bool) {
	const maxSafe = (1 << 53) - 1
	switch n := v.(type) {
	case json.Number:
		if i, err := n.Int64(); err == nil && i >= -maxSafe && i <= maxSafe {
			return i, true
		}
		if f, err := n.Float64(); err == nil && !math.IsNaN(f) && !math.IsInf(f, 0) && math.Trunc(f) == f && f >= -maxSafe && f <= maxSafe {
			return int64(f), true
		}
	case float64:
		if !math.IsNaN(n) && !math.IsInf(n, 0) && math.Trunc(n) == n && n >= -maxSafe && n <= maxSafe {
			return int64(n), true
		}
	}
	return 0, false
}

func nonEmptyText(v any) bool {
	s, ok := v.(string)
	return ok && strings.TrimSpace(s) != ""
}

func (s *turnState) prepareConnections(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeJSONObject(raw)
	if err != nil || len(args) != 0 {
		return nil, invalidFault("Invalid database connection arguments")
	}
	return json.RawMessage(`{}`), nil
}

func (s *turnState) prepareOp(write *bool) func(context.Context, json.RawMessage) (json.RawMessage, error) {
	allowed := []string{"connection", "database"}
	if write != nil {
		allowed = []string{"connection", "database", "sql"}
	}
	return func(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
		args, err := decodeJSONObject(raw)
		if err != nil {
			return nil, invalidFault("Invalid database tool arguments")
		}
		for k := range args {
			if !slices.Contains(allowed, k) {
				return nil, invalidFault("Invalid database tool arguments")
			}
		}
		conn, ok := args["connection"].(string)
		if !ok || !s.has(conn) {
			return nil, invalidFault("Invalid database tool arguments")
		}
		if db, exists := args["database"]; exists && db != nil {
			if _, ok := db.(string); !ok {
				return nil, invalidFault("Invalid database tool arguments")
			}
		}
		if write != nil {
			if _, ok := args["sql"].(string); !ok {
				return nil, invalidFault("Invalid database tool arguments")
			}
			args["row_limit"] = 1000
			args["timeout_ms"] = 30000
		}
		return json.Marshal(args)
	}
}

var saveFields = []string{"connection", "expected_revision", "name", "engine", "path", "host", "port", "username", "database", "read_only"}

func (s *turnState) prepareSave(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeJSONObject(raw)
	if err != nil {
		return nil, invalidFault("Invalid database connection arguments")
	}
	for k := range args {
		if !slices.Contains(saveFields, k) {
			return nil, invalidFault("Invalid database connection arguments")
		}
	}
	connVal, hasConn := args["connection"]
	revVal, hasRev := args["expected_revision"]
	create := (!hasConn || connVal == nil) && (!hasRev || revVal == nil)
	if !create {
		conn, ok := connVal.(string)
		rev, revOK := safeInt(revVal)
		if !ok || !uuidRE.MatchString(conn) || !revOK || rev < 1 {
			return nil, invalidFault("Invalid database connection arguments")
		}
		if !s.has(conn) {
			return nil, invalidFault("Connection is not available in this session; use database_connections or create a connection without connection and expected_revision")
		}
	}
	for _, field := range []string{"name", "path", "host", "username", "database"} {
		if v, exists := args[field]; exists {
			str, ok := v.(string)
			if !ok || (field != "database" && strings.TrimSpace(str) == "") {
				return nil, invalidFault("Invalid database connection arguments")
			}
		}
	}
	if v, exists := args["engine"]; exists {
		eng, ok := v.(string)
		if !ok || (eng != "sqlite" && eng != "mysql" && eng != "postgres") {
			return nil, invalidFault("Invalid database connection arguments")
		}
	}
	if v, exists := args["port"]; exists {
		p, ok := safeInt(v)
		if !ok || p < 1 || p > 65535 {
			return nil, invalidFault("Invalid database connection arguments")
		}
	}
	if v, exists := args["read_only"]; exists {
		if _, ok := v.(bool); !ok {
			return nil, invalidFault("Invalid database connection arguments")
		}
	}
	if create {
		eng, _ := args["engine"].(string)
		if !nonEmptyText(args["name"]) || eng == "" {
			return nil, invalidFault("Invalid database connection arguments")
		}
		if eng == "sqlite" {
			if !nonEmptyText(args["path"]) {
				return nil, invalidFault("Invalid database connection arguments")
			}
		} else {
			p, ok := safeInt(args["port"])
			_, dbOK := args["database"].(string)
			if !nonEmptyText(args["host"]) || !ok || p < 1 || p > 65535 || !nonEmptyText(args["username"]) || !dbOK {
				return nil, invalidFault("Invalid database connection arguments")
			}
		}
		delete(args, "connection")
		delete(args, "expected_revision")
		if _, exists := args["read_only"]; !exists {
			args["read_only"] = true
		}
	}
	return json.Marshal(args)
}

func (s *turnState) formatSaved(_ context.Context, preparedArgs json.RawMessage, res engine.ToolResult) engine.ToolResult {
	if res.Failure != nil || res.Error || res.ReturnedError {
		return res
	}
	var args map[string]any
	_ = json.Unmarshal(preparedArgs, &args)
	body, err := json.Marshal(res.Value)
	if err != nil {
		return res
	}
	var out struct {
		Kind string `json:"kind"`
		Data struct {
			ID string `json:"id"`
		} `json:"data"`
	}
	if json.Unmarshal(body, &out) == nil && out.Kind == "database_profile" && uuidRE.MatchString(out.Data.ID) {
		if conn, ok := args["connection"].(string); !ok || conn == "" || conn == out.Data.ID {
			s.add(out.Data.ID)
		}
	}
	return res
}
