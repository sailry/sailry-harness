// Package sshnode provides the compile-time native Node SSH agent tools
// (ssh_run, ssh_transfer, ssh_connections, ssh_save_connection) without
// running JS turn hooks.
package sshnode

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
	"github.com/sailry/sailry-harness/internal/node/agent/prompt"
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

type sshSummary struct {
	ID   string
	Name string
}

type module struct{}

// Module returns the native Node module providing SSH connection, execution
// and file-transfer tools.
func Module() moduleapi.Module { return module{} }

func (module) ID() string                { return "ssh" }
func (module) ReplacesPlugins() []string { return []string{"ssh"} }

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

	profiles := make([]sshSummary, 0, len(reachable.Ssh))
	state := &turnState{names: make(map[string]string, len(reachable.Ssh))}
	for _, profile := range reachable.Ssh {
		id := profile.ID.String()
		profiles = append(profiles, sshSummary{ID: id, Name: profile.Name})
		state.connections = append(state.connections, id)
		state.names[id] = profile.Name
	}

	primary := ""
	if reachable.Bound != nil && reachable.Bound.Kind == "ssh" {
		if id, ok := reachable.Bound.Data.(protocol.SshID); ok {
			candidate := id.String()
			if slices.Contains(state.connections, candidate) {
				primary = candidate
			}
		} else if id, ok := reachable.Bound.Data.(*protocol.SshID); ok && id != nil {
			candidate := id.String()
			if slices.Contains(state.connections, candidate) {
				primary = candidate
			}
		}
	}
	var turnInstruction prompt.Part
	if primary != "" {
		turnInstruction = prompt.Part{
			Key:  "module:ssh",
			Text: fmt.Sprintf("The primary SSH connection is %s. Use it by default unless the user requests another saved connection; it does not restrict other SSH operations.", primary),
		}
	}

	tools := make([]engine.Tool, 0, len(available))
	for _, spec := range available {
		params, err := dynamicParameters(spec.Name, profiles, primary)
		if err != nil {
			return nil, err
		}
		spec.Parameters = params
		var prepare func(context.Context, json.RawMessage) (json.RawMessage, error)
		var format func(context.Context, json.RawMessage, engine.ToolResult) engine.ToolResult
		switch spec.Name {
		case "ssh_connections":
			prepare = state.prepareConnections
		case "ssh_save_connection":
			prepare = state.prepareSave
			format = state.formatSaved
		case "ssh_run":
			prepare = state.prepareRun
			format = state.formatCompleted
		case "ssh_transfer":
			prepare = state.prepareTransfer
			format = state.formatCompleted
		}
		tool, err := host.ConnectionTool(inv, moduleapi.ConnectionToolConfig{
			Spec:            spec,
			Instruction:     catalog.Instructions,
			TurnInstruction: turnInstruction,
			Captured:        captured,
			PrepareArgs:     prepare,
			FormatResult:    format,
		})
		if err != nil {
			return nil, err
		}
		tools = append(tools, tool)
	}
	return tools, nil
}

func dynamicParameters(name string, profiles []sshSummary, primary string) (json.RawMessage, error) {
	switch name {
	case "ssh_connections":
		return json.RawMessage(`{"type":"object","properties":{},"required":[],"additionalProperties":false}`), nil
	case "ssh_save_connection":
		return saveParameters(), nil
	}
	choices := make([][2]string, 0, len(profiles))
	for _, p := range profiles {
		choices = append(choices, [2]string{p.ID, p.Name})
	}
	choicesJSON, err := json.Marshal(choices)
	if err != nil {
		return nil, err
	}
	connProp := map[string]any{"type": "string", "pattern": uuidPattern, "description": string(choicesJSON)}
	if primary != "" {
		connProp["default"] = primary
	}
	properties := map[string]any{"connection": connProp}
	required := []string{"connection"}
	if name == "ssh_run" {
		properties["command"] = map[string]any{"type": "string"}
		properties["timeout_ms"] = map[string]any{"type": "integer", "minimum": 1, "maximum": 600000}
		required = append(required, "command")
	} else {
		properties["path"] = map[string]any{"type": "string"}
		properties["remote_path"] = map[string]any{"type": "string"}
		properties["direction"] = map[string]any{"type": "string", "enum": []string{"upload", "download"}}
		required = append(required, "path", "remote_path", "direction")
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
			"host":              map[string]any{"type": "string", "minLength": 1},
			"port":              map[string]any{"type": "integer", "minimum": 1, "maximum": 65535},
			"username":          map[string]any{"type": "string", "minLength": 1},
			"authentication":    map[string]any{"type": "string", "enum": []string{"password", "private_key", "key_path", "agent"}},
			"credential": map[string]any{"oneOf": []any{
				map[string]any{"type": "object", "properties": map[string]any{"kind": map[string]any{"const": "password"}, "password": map[string]any{"type": "string"}}, "required": []string{"kind", "password"}, "additionalProperties": false},
				map[string]any{"type": "object", "properties": map[string]any{"kind": map[string]any{"const": "private_key"}, "key": map[string]any{"type": "string"}, "passphrase": map[string]any{"type": []string{"string", "null"}}}, "required": []string{"kind", "key"}, "additionalProperties": false},
				map[string]any{"type": "object", "properties": map[string]any{"kind": map[string]any{"const": "key_path"}, "path": map[string]any{"type": "string"}, "passphrase": map[string]any{"type": []string{"string", "null"}}}, "required": []string{"kind", "path"}, "additionalProperties": false},
				map[string]any{"type": "object", "properties": map[string]any{"kind": map[string]any{"const": "agent"}}, "required": []string{"kind"}, "additionalProperties": false},
			}},
			"key_path": map[string]any{"type": "string", "minLength": 1, "description": "Path to an existing private key on the execution Node; never its contents"},
		},
		"allOf": []any{map[string]any{
			"if":   map[string]any{"required": []string{"connection"}, "properties": map[string]any{"connection": map[string]any{"type": "string"}}},
			"then": map[string]any{"required": []string{"expected_revision"}, "properties": map[string]any{"expected_revision": map[string]any{"type": "integer"}}},
			"else": map[string]any{
				"required":   []string{"name", "host", "port", "username", "authentication"},
				"properties": map[string]any{"expected_revision": map[string]any{"type": "null"}},
				"allOf": []any{
					map[string]any{
						"if":   map[string]any{"properties": map[string]any{"authentication": map[string]any{"enum": []string{"password", "private_key"}}}},
						"then": map[string]any{"required": []string{"credential"}},
					},
					map[string]any{
						"if":   map[string]any{"properties": map[string]any{"authentication": map[string]any{"const": "key_path"}}},
						"then": map[string]any{"anyOf": []any{map[string]any{"required": []string{"credential"}}, map[string]any{"required": []string{"key_path"}}}},
					},
				},
			},
		}},
	})
	return raw
}

type turnState struct {
	mu          sync.Mutex
	connections []string
	names       map[string]string
}

func (s *turnState) has(id string) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return slices.Contains(s.connections, id)
}

func (s *turnState) name(id string) string {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.names[id]
}

func (s *turnState) add(id, name string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if !slices.Contains(s.connections, id) {
		s.connections = append(s.connections, id)
	}
	if strings.TrimSpace(name) != "" {
		s.names[id] = name
	}
}

func invalidTool() error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "Invalid SSH tool arguments"}
}

func invalidConn() error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "Invalid SSH connection arguments"}
}

func unavailableConn() error {
	return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "SSH connection is unavailable to this session; use ssh_connections"}
}

func decodeObject(raw json.RawMessage) (map[string]any, error) {
	var obj map[string]any
	dec := json.NewDecoder(strings.NewReader(string(raw)))
	dec.UseNumber()
	if err := dec.Decode(&obj); err != nil || obj == nil {
		return nil, invalidTool()
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
	args, err := decodeObject(raw)
	if err != nil || len(args) != 0 {
		return nil, invalidConn()
	}
	return json.RawMessage(`{}`), nil
}

func (s *turnState) prepareRun(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw)
	if err != nil {
		return nil, invalidTool()
	}
	for k := range args {
		if k != "connection" && k != "command" && k != "timeout_ms" {
			return nil, invalidTool()
		}
	}
	conn, ok := args["connection"].(string)
	if !ok {
		return nil, invalidTool()
	}
	if _, ok := args["command"].(string); !ok {
		return nil, invalidTool()
	}
	if v, exists := args["timeout_ms"]; exists && v != nil {
		ms, ok := safeInt(v)
		if !ok || ms < 1 || ms > 600000 {
			return nil, invalidTool()
		}
	} else {
		args["timeout_ms"] = 120000
	}
	if !s.has(conn) {
		return nil, unavailableConn()
	}
	return json.Marshal(args)
}

func (s *turnState) prepareTransfer(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw)
	if err != nil {
		return nil, invalidTool()
	}
	for k := range args {
		if k != "connection" && k != "path" && k != "remote_path" && k != "direction" {
			return nil, invalidTool()
		}
	}
	conn, ok := args["connection"].(string)
	if !ok {
		return nil, invalidTool()
	}
	if _, ok := args["path"].(string); !ok {
		return nil, invalidTool()
	}
	if _, ok := args["remote_path"].(string); !ok {
		return nil, invalidTool()
	}
	dir, ok := args["direction"].(string)
	if !ok || (dir != "upload" && dir != "download") {
		return nil, invalidTool()
	}
	if !s.has(conn) {
		return nil, unavailableConn()
	}
	args["timeout_ms"] = 120000
	return json.Marshal(args)
}

var saveFields = []string{"connection", "expected_revision", "name", "host", "port", "username", "authentication", "credential", "key_path"}

func (s *turnState) prepareSave(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	args, err := decodeObject(raw)
	if err != nil {
		return nil, invalidConn()
	}
	for k := range args {
		if !slices.Contains(saveFields, k) {
			return nil, invalidConn()
		}
	}
	connVal, hasConn := args["connection"]
	revVal, hasRev := args["expected_revision"]
	create := (!hasConn || connVal == nil) && (!hasRev || revVal == nil)
	if !create {
		conn, ok := connVal.(string)
		rev, revOK := safeInt(revVal)
		if !ok || !uuidRE.MatchString(conn) || !s.has(conn) || !revOK || rev < 1 {
			return nil, invalidConn()
		}
	}
	for _, field := range []string{"name", "host", "username", "key_path"} {
		if v, exists := args[field]; exists && !nonEmptyText(v) {
			return nil, invalidConn()
		}
	}
	auth := ""
	if v, exists := args["authentication"]; exists {
		a, ok := v.(string)
		if !ok || (a != "password" && a != "private_key" && a != "key_path" && a != "agent") {
			return nil, invalidConn()
		}
		auth = a
	}
	if v, exists := args["port"]; exists {
		p, ok := safeInt(v)
		if !ok || p < 1 || p > 65535 {
			return nil, invalidConn()
		}
	}
	_, hasKeyPath := args["key_path"]
	_, hasCred := args["credential"]
	if hasKeyPath && auth != "" && auth != "key_path" {
		return nil, invalidConn()
	}
	if hasCred && hasKeyPath {
		return nil, invalidConn()
	}
	if hasCred {
		credKind, ok := validCredential(args["credential"])
		if !ok || (auth != "" && credKind != auth) {
			return nil, invalidConn()
		}
	}
	if create {
		p, ok := safeInt(args["port"])
		if !nonEmptyText(args["name"]) || !nonEmptyText(args["host"]) || !ok || p < 1 || p > 65535 || !nonEmptyText(args["username"]) || auth == "" {
			return nil, invalidConn()
		}
		if auth != "agent" && !hasCred && !(auth == "key_path" && nonEmptyText(args["key_path"])) {
			return nil, invalidConn()
		}
		delete(args, "connection")
		delete(args, "expected_revision")
	}
	return json.Marshal(args)
}

func validCredential(v any) (string, bool) {
	m, ok := v.(map[string]any)
	if !ok {
		return "", false
	}
	kind, ok := m["kind"].(string)
	if !ok {
		return "", false
	}
	var allowed []string
	required := ""
	switch kind {
	case "password":
		allowed, required = []string{"kind", "password"}, "password"
	case "private_key":
		allowed, required = []string{"kind", "key", "passphrase"}, "key"
	case "key_path":
		allowed, required = []string{"kind", "path", "passphrase"}, "path"
	case "agent":
		allowed = []string{"kind"}
	default:
		return "", false
	}
	for k := range m {
		if !slices.Contains(allowed, k) {
			return "", false
		}
	}
	if required != "" {
		if _, ok := m[required].(string); !ok {
			return "", false
		}
	}
	if pp, exists := m["passphrase"]; exists && pp != nil {
		if _, ok := pp.(string); !ok {
			return "", false
		}
	}
	return kind, true
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
			ID   string `json:"id"`
			Name string `json:"name"`
		} `json:"data"`
	}
	if json.Unmarshal(body, &out) == nil && out.Kind == "ssh_profile" && uuidRE.MatchString(out.Data.ID) {
		if conn, ok := args["connection"].(string); !ok || conn == "" || conn == out.Data.ID {
			s.add(out.Data.ID, out.Data.Name)
		}
	}
	return res
}

func (s *turnState) formatCompleted(_ context.Context, preparedArgs json.RawMessage, res engine.ToolResult) engine.ToolResult {
	if res.Failure != nil {
		return res
	}
	var args struct {
		Connection string `json:"connection"`
	}
	if json.Unmarshal(preparedArgs, &args) != nil {
		return res
	}
	name := s.name(args.Connection)
	if strings.TrimSpace(name) == "" {
		return res
	}
	body, err := json.Marshal(res.Value)
	if err != nil {
		return res
	}
	var obj map[string]any
	if json.Unmarshal(body, &obj) == nil && obj != nil {
		obj["connection_name"] = name
		res.Value = obj
	}
	return res
}
