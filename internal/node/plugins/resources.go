package plugins

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"net"
	"net/url"
	"slices"
	"strings"

	"github.com/MoeForge/moego/base64"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func packageIcon(data []byte) *string {
	if len(data) < 33 || len(data) > 64*1024 || !bytes.HasPrefix(data, []byte("\x89PNG\r\n\x1a\n")) || string(data[12:16]) != "IHDR" {
		return nil
	}
	width, height := binary.BigEndian.Uint32(data[16:20]), binary.BigEndian.Uint32(data[20:24])
	if width == 0 || height == 0 || width > 512 || height > 512 {
		return nil
	}
	encoded := base64.StdEncoding.EncodeToString(data)
	return &encoded
}

func ReadMCP(data []byte) ([]protocol.PluginMcp, map[string]protocol.PluginMcpServer, []protocol.PluginIssue) {
	entries := []protocol.PluginMcp{}
	servers := map[string]protocol.PluginMcpServer{}
	issues := []protocol.PluginIssue{}
	if len(data) == 0 {
		return entries, servers, issues
	}
	var document struct {
		Schema  string                     `json:"$schema"`
		Servers map[string]json.RawMessage `json:"mcpServers"`
	}
	decoder := json.NewDecoder(strings.NewReader(string(data)))
	decoder.DisallowUnknownFields()
	if len(data) > 256*1024 || decoder.Decode(&document) != nil ||
		document.Schema != "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json" || len(document.Servers) > 16 {
		return entries, servers, []protocol.PluginIssue{{Path: "mcp.json", Kind: protocol.PluginIssueKindInvalidMcp}}
	}
	names := []string{}
	for name := range document.Servers {
		names = append(names, name)
	}
	slices.Sort(names)
	for _, name := range names {
		var server protocol.PluginMcpServer
		if name == "" || len(name) > 256 || strings.ContainsRune(name, 0) || json.Unmarshal(document.Servers[name], &server) != nil || ValidateMCPServer(server) != nil {
			pointer := strings.ReplaceAll(strings.ReplaceAll(name, "~", "~0"), "/", "~1")
			issues = append(issues, protocol.PluginIssue{Path: "mcp.json#/mcpServers/" + pointer, Kind: protocol.PluginIssueKindInvalidMcpServer})
			continue
		}
		servers[name] = server
		entry := protocol.DefaultPluginMcp()
		entry.Name, entry.Allowed, entry.Availability = name, true, protocol.PluginMcpAvailabilityAvailable
		switch server.Kind {
		case "stdio":
			entry.Transport = protocol.PluginMcpTransportStdio
		case "streamable-http":
			entry.Transport = protocol.PluginMcpTransportStreamableHttp
		case "sse":
			entry.Transport = protocol.PluginMcpTransportSse
		}
		entries = append(entries, entry)
	}
	return entries, servers, issues
}

func ValidateMCPServer(server protocol.PluginMcpServer) error {
	var tools *[]string
	switch payload := server.Data.(type) {
	case *protocol.PluginMcpServerStdioPayload:
		tools = payload.Tools
		relative := strings.HasPrefix(payload.Command, "./")
		if payload.Command == "" || strings.ContainsRune(payload.Command, 0) || relative && !resourcePath(strings.TrimPrefix(payload.Command, "./")) ||
			!relative && (strings.ContainsAny(payload.Command, "/\\ \r\n\t") || payload.Command == "." || payload.Command == "..") {
			return packageFault("invalid MCP process command")
		}
		for _, argument := range payload.Args {
			if strings.ContainsRune(argument, 0) {
				return packageFault("invalid MCP process argument")
			}
		}
		for name, value := range payload.Env {
			if name == "" || strings.ContainsAny(name, "=\x00") || strings.ContainsRune(value, 0) ||
				slices.Contains([]string{"PLUGIN_ROOT", "PLUGIN_DATA"}, strings.ToUpper(name)) {
				return packageFault("invalid MCP process environment")
			}
		}
		cwd := payload.Cwd
		valid := false
		for _, prefix := range []string{"${PLUGIN_ROOT}", "${PLUGIN_DATA}", "."} {
			if cwd == prefix || strings.HasPrefix(cwd, prefix+"/") && resourcePath(strings.TrimPrefix(cwd, prefix+"/")) {
				valid = true
			}
		}
		if !valid {
			return packageFault("MCP process directory must belong to package or data")
		}
	case *protocol.PluginMcpServerHttpPayload:
		tools = payload.Tools
		if err := validateMCPEndpoint(payload.Url, payload.Headers); err != nil {
			return err
		}
	case *protocol.PluginMcpServerSsePayload:
		tools = payload.Tools
		if err := validateMCPEndpoint(payload.Url, payload.Headers); err != nil {
			return err
		}
	default:
		return packageFault("unsupported MCP server declaration")
	}
	if tools != nil {
		if len(*tools) == 0 || len(*tools) > 64 {
			return packageFault("invalid MCP tool selection")
		}
		seen := map[string]bool{}
		for _, name := range *tools {
			if name == "" || len(name) > 128 || strings.ContainsRune(name, 0) || seen[name] {
				return packageFault("invalid MCP tool selection")
			}
			seen[name] = true
		}
	}
	return nil
}

func validateMCPEndpoint(endpoint string, headers map[string]string) error {
	address, err := url.Parse(endpoint)
	if err != nil || address.Host == "" || address.User != nil || address.Fragment != "" || address.Scheme != "https" && address.Scheme != "http" {
		return packageFault("invalid MCP endpoint")
	}
	if address.Scheme == "http" && address.Hostname() != "localhost" {
		host := net.ParseIP(address.Hostname())
		if host == nil || !host.IsLoopback() {
			return packageFault("MCP endpoint requires HTTPS or loopback")
		}
	}
	seen := map[string]bool{}
	for name, value := range headers {
		key := strings.ToLower(name)
		if !headerName(name) || strings.ContainsAny(value, "\r\n\x00") || protocolHeader(name) || seen[key] {
			return packageFault("invalid MCP header declaration")
		}
		seen[key] = true
	}
	return nil
}
