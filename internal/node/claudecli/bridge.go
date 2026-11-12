// Package claudecli runs the user's own Claude Code as a per-step model. See
// docs/claude-cli.md.
//
// The tool bridge, the tool name rule and the conversation rendering derive
// from magpie 0af73f4 (internal/claudebridge, internal/gateway). MIT; see
// LICENSE.magpie.
package claudecli

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

const (
	// HelperCommand is the hidden mode of a Sailry executable that Claude Code
	// starts as its one MCP server.
	HelperCommand = "claude-bridge"
	// ServerName is the MCP server's name; Claude Code names each tool
	// mcp__sailry__<name>.
	ServerName = "sailry"
	toolPrefix = "mcp__" + ServerName + "__"

	socketFile = "s"
	tokenFile  = "token"
	// callIdentity is where Claude Code names the tool call it is making.
	callIdentity = "claudecode/toolUseId"
	// socketLimit is the shortest Unix socket path limit among supported systems.
	socketLimit = 100
	greeting    = 5 * time.Second
)

// Tool is one declaration offered to Claude Code.
type Tool struct {
	Name        string
	Description string
	Schema      json.RawMessage
}

// Media is one image a tool result carries.
type Media struct {
	Mime string
	Data []byte
}

// Result answers one parked tool call.
type Result struct {
	Text  string
	Error bool
	Media []Media
}

var (
	toolNamePlain  = regexp.MustCompile(`^[A-Za-z0-9_-]{1,51}$`)
	toolNameOther  = regexp.MustCompile(`[^A-Za-z0-9_-]`)
	toolNameHashed = regexp.MustCompile(`_[0-9a-f]{12}$`)
)

// ToolName is the name a tool is offered under. Claude Code keeps only
// [A-Za-z0-9_-] of an MCP tool name and 64 characters with its prefix, so a
// name it would change, or one shaped like a changed name, is cut and told
// apart by a hash of the whole name, the same in every run.
func ToolName(name string) string {
	if toolNamePlain.MatchString(name) && !toolNameHashed.MatchString(name) {
		return name
	}
	sum := sha256.Sum256([]byte(name))
	short := toolNameOther.ReplaceAllString(name, "_")
	if len(short) > 38 {
		short = short[:38]
	}
	return short + "_" + hex.EncodeToString(sum[:6])
}

// Bridge is the Node side of the tool boundary: an MCP server on a private
// Unix socket whose tools/call requests park until the next step brings their
// results.
type Bridge struct {
	directory string
	token     string
	listener  net.Listener
	server    *mcp.Server
	closed    chan struct{}
	once      sync.Once

	mu       sync.Mutex
	offered  map[string]string // offered name to its declaration digest
	parked   map[string]chan Result
	answered map[string]Result
	listed   chan struct{}
}

// NewBridge listens inside directory, which must be private to the Node.
func NewBridge(directory string) (*Bridge, error) {
	path := filepath.Join(directory, socketFile)
	if len(path) > socketLimit {
		return nil, fmt.Errorf("tool bridge socket path is longer than %d bytes", socketLimit)
	}
	secret := make([]byte, 32)
	if _, err := rand.Read(secret); err != nil {
		return nil, err
	}
	token := hex.EncodeToString(secret)
	if err := os.WriteFile(filepath.Join(directory, tokenFile), []byte(token), 0o600); err != nil {
		return nil, err
	}
	listener, err := net.Listen("unix", path)
	if err != nil {
		return nil, err
	}
	bridge := &Bridge{directory: directory, token: token, listener: listener, closed: make(chan struct{}),
		offered: map[string]string{}, parked: map[string]chan Result{}, answered: map[string]Result{}}
	bridge.server = mcp.NewServer(&mcp.Implementation{Name: ServerName, Version: "1"}, nil)
	bridge.server.AddReceivingMiddleware(func(next mcp.MethodHandler) mcp.MethodHandler {
		return func(ctx context.Context, method string, request mcp.Request) (mcp.Result, error) {
			result, err := next(ctx, method, request)
			if method == "tools/list" {
				bridge.mu.Lock()
				if bridge.listed != nil {
					close(bridge.listed)
					bridge.listed = nil
				}
				bridge.mu.Unlock()
			}
			return result, err
		}
	})
	go bridge.accept()
	return bridge, nil
}

func (bridge *Bridge) accept() {
	for {
		connection, err := bridge.listener.Accept()
		if err != nil {
			return
		}
		go bridge.serve(connection)
	}
}

// serve authenticates one helper and runs an MCP session over its connection.
func (bridge *Bridge) serve(connection net.Conn) {
	defer connection.Close()
	_ = connection.SetReadDeadline(time.Now().Add(greeting))
	// The token is read a byte at a time so nothing of the session is consumed.
	offered := make([]byte, 0, len(bridge.token))
	for one := make([]byte, 1); ; {
		if _, err := io.ReadFull(connection, one); err != nil {
			return
		}
		if one[0] == '\n' {
			break
		}
		if len(offered) > len(bridge.token) {
			return
		}
		offered = append(offered, one[0])
	}
	if subtle.ConstantTimeCompare(offered, []byte(bridge.token)) != 1 {
		return
	}
	_ = connection.SetReadDeadline(time.Time{})
	session, err := bridge.server.Connect(context.Background(), &mcp.IOTransport{Reader: connection, Writer: connection}, nil)
	if err != nil {
		return
	}
	finished := make(chan struct{})
	go func() {
		_ = session.Wait()
		close(finished)
	}()
	select {
	case <-finished:
	case <-bridge.closed:
		_ = session.Close()
		<-finished
	}
}

// SetTools replaces the offered declarations and reports whether they
// changed. A connected Claude Code is notified by the server.
func (bridge *Bridge) SetTools(tools []Tool) (bool, error) {
	next := map[string]Tool{}
	digests := map[string]string{}
	for _, tool := range tools {
		var schema map[string]json.RawMessage
		if json.Unmarshal(tool.Schema, &schema) != nil || string(schema["type"]) != `"object"` {
			return false, fmt.Errorf("tool %q does not declare an object input schema", tool.Name)
		}
		name := ToolName(tool.Name)
		if _, exists := next[name]; exists {
			return false, fmt.Errorf("tool %q is declared twice", tool.Name)
		}
		sum := sha256.Sum256([]byte(tool.Description + "\x00" + string(tool.Schema)))
		next[name], digests[name] = tool, hex.EncodeToString(sum[:])
	}
	bridge.mu.Lock()
	var removed []string
	for name, digest := range bridge.offered {
		if digests[name] != digest {
			removed = append(removed, name)
		}
	}
	var added []string
	for name, digest := range digests {
		if bridge.offered[name] != digest {
			added = append(added, name)
		}
	}
	changed := len(removed) > 0 || len(added) > 0
	if changed {
		bridge.offered = digests
		if bridge.listed == nil {
			bridge.listed = make(chan struct{})
		}
	}
	bridge.mu.Unlock()
	if len(removed) > 0 {
		bridge.server.RemoveTools(removed...)
	}
	for _, name := range added {
		tool := next[name]
		bridge.server.AddTool(&mcp.Tool{Name: name, Description: tool.Description, InputSchema: tool.Schema}, bridge.call)
	}
	return changed, nil
}

// Listed waits until Claude Code has listed the tools since their last
// change, the bridge closed, or the wait passed.
func (bridge *Bridge) Listed(ctx context.Context, wait time.Duration) {
	bridge.mu.Lock()
	listed := bridge.listed
	bridge.mu.Unlock()
	if listed == nil {
		return
	}
	timer := time.NewTimer(wait)
	defer timer.Stop()
	select {
	case <-listed:
	case <-timer.C:
	case <-ctx.Done():
	case <-bridge.closed:
	}
}

// call parks one tools/call until its result arrives.
func (bridge *Bridge) call(ctx context.Context, request *mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	identity, _ := request.Params.Meta[callIdentity].(string)
	if identity == "" {
		return toolResult(Result{Text: "tool call was not run: it carries no call identity", Error: true}), nil
	}
	bridge.mu.Lock()
	result, ready := bridge.answered[identity]
	var waiting chan Result
	if ready {
		delete(bridge.answered, identity)
	} else {
		waiting = make(chan Result, 1)
		bridge.parked[identity] = waiting
	}
	bridge.mu.Unlock()
	if ready {
		return toolResult(result), nil
	}
	select {
	case result = <-waiting:
		return toolResult(result), nil
	case <-ctx.Done():
	case <-bridge.closed:
	}
	bridge.mu.Lock()
	delete(bridge.parked, identity)
	bridge.mu.Unlock()
	return nil, errors.New("tool call was abandoned")
}

// Resolve answers the call with this identity, now or when it arrives.
func (bridge *Bridge) Resolve(identity string, result Result) {
	bridge.mu.Lock()
	defer bridge.mu.Unlock()
	if waiting, parked := bridge.parked[identity]; parked {
		delete(bridge.parked, identity)
		waiting <- result
		return
	}
	bridge.answered[identity] = result
}

// Config is the --mcp-config value that has Claude Code start the helper.
func (bridge *Bridge) Config(executable string) string {
	raw, _ := json.Marshal(map[string]any{"mcpServers": map[string]any{
		ServerName: map[string]any{"command": executable, "args": []string{HelperCommand, bridge.directory}},
	}})
	return string(raw)
}

// Close ends the sessions and releases every parked call.
func (bridge *Bridge) Close() {
	bridge.once.Do(func() {
		close(bridge.closed)
		_ = bridge.listener.Close()
	})
}

func toolResult(result Result) *mcp.CallToolResult {
	content := []mcp.Content{&mcp.TextContent{Text: result.Text}}
	for _, media := range result.Media {
		content = append(content, &mcp.ImageContent{MIMEType: media.Mime, Data: media.Data})
	}
	return &mcp.CallToolResult{Content: content, IsError: result.Error}
}

// IsHelper reports whether a Sailry executable was started as the relay.
func IsHelper(arguments []string) bool {
	return len(arguments) == 2 && arguments[0] == HelperCommand
}

// RunHelper is the hidden mode's whole process: it joins Claude Code's stdio
// to the bridge and returns the exit code.
func RunHelper(arguments []string) int {
	if err := Relay(arguments[1], os.Stdin, os.Stdout); err != nil {
		_, _ = fmt.Fprintln(os.Stderr, "sailry "+HelperCommand+": "+err.Error())
		return 1
	}
	return 0
}

// Relay authenticates to the bridge in directory and copies both directions
// until either side ends. It holds no tool logic.
func Relay(directory string, input io.Reader, output io.Writer) error {
	token, err := os.ReadFile(filepath.Join(directory, tokenFile))
	if err != nil {
		return err
	}
	connection, err := net.Dial("unix", filepath.Join(directory, socketFile))
	if err != nil {
		return err
	}
	defer connection.Close()
	if _, err := io.WriteString(connection, strings.TrimSpace(string(token))+"\n"); err != nil {
		return err
	}
	ended := make(chan error, 2)
	go func() {
		_, err := io.Copy(connection, input)
		ended <- err
	}()
	go func() {
		_, err := io.Copy(output, connection)
		ended <- err
	}()
	return <-ended
}
