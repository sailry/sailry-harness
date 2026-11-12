package claudecli

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const testWait = 10 * time.Second

// fakeClaude is a scripted Claude Code: it reads stdin user lines, writes
// stream-json stdout lines and calls tools through the real relay and bridge.
type fakeClaude struct {
	t      *testing.T
	launch Launch
	input  *io.PipeReader
	stdin  *io.PipeWriter
	output *io.PipeWriter
	stdout *io.PipeReader
	lines  *bufio.Reader
	ctx    context.Context
	cancel context.CancelFunc
	closed chan struct{}
	once   sync.Once

	mu          sync.Mutex
	diagnostics string
	session     *mcp.ClientSession
}

func (fake *fakeClaude) Write(data []byte) (int, error) { return fake.stdin.Write(data) }
func (fake *fakeClaude) Read(data []byte) (int, error)  { return fake.stdout.Read(data) }
func (fake *fakeClaude) Diagnostics() string {
	fake.mu.Lock()
	defer fake.mu.Unlock()
	return fake.diagnostics
}
func (fake *fakeClaude) Close() error {
	fake.once.Do(func() {
		fake.cancel()
		_ = fake.stdin.Close()
		_ = fake.input.Close()
		_ = fake.output.Close()
		_ = fake.stdout.Close()
		close(fake.closed)
	})
	return nil
}

// next returns the text of the next stdin user message, or false at its end.
func (fake *fakeClaude) next() (string, bool) {
	raw, err := fake.lines.ReadBytes('\n')
	if err != nil {
		return "", false
	}
	var line struct {
		Type    string `json:"type"`
		Message struct {
			Content []struct {
				Type string `json:"type"`
				Text string `json:"text"`
			} `json:"content"`
		} `json:"message"`
	}
	if json.Unmarshal(raw, &line) != nil || line.Type != "user" {
		fake.t.Errorf("stdin line is not a user message: %s", raw)
		return "", false
	}
	text := ""
	for _, block := range line.Message.Content {
		text += block.Text
	}
	return text, true
}

func (fake *fakeClaude) emit(lines ...string) {
	for _, line := range lines {
		_, _ = io.WriteString(fake.output, line+"\n")
	}
}

// exit ends the process with this error output.
func (fake *fakeClaude) exit(diagnostics string) {
	fake.mu.Lock()
	fake.diagnostics = diagnostics
	fake.mu.Unlock()
	_ = fake.output.Close()
}

// directory is the bridge folder the --mcp-config argument names.
func (fake *fakeClaude) directory() string {
	for index, argument := range fake.launch.Arguments {
		if argument == "--mcp-config" && index+1 < len(fake.launch.Arguments) {
			var config struct {
				Servers map[string]struct {
					Command string   `json:"command"`
					Args    []string `json:"args"`
				} `json:"mcpServers"`
			}
			if err := json.Unmarshal([]byte(fake.launch.Arguments[index+1]), &config); err != nil {
				fake.t.Errorf("mcp config: %v", err)
				return ""
			}
			server := config.Servers[ServerName]
			if server.Command != "sailry-test" || len(server.Args) != 2 || server.Args[0] != HelperCommand {
				fake.t.Errorf("mcp server = %+v", server)
				return ""
			}
			return server.Args[1]
		}
	}
	fake.t.Error("launch has no --mcp-config")
	return ""
}

// call makes one tools/call the way Claude Code does and returns its text.
func (fake *fakeClaude) call(identity, name string, arguments map[string]any) (string, bool) {
	if fake.session == nil {
		session, stop := connectClient(fake.ctx, fake.t, fake.directory())
		if session == nil {
			return "", false
		}
		go func() {
			<-fake.closed
			stop()
		}()
		fake.session = session
	}
	result, err := fake.session.CallTool(fake.ctx, &mcp.CallToolParams{Meta: mcp.Meta{callIdentity: identity}, Name: name, Arguments: arguments})
	if err != nil {
		return "", false
	}
	text := ""
	for _, content := range result.Content {
		if value, ok := content.(*mcp.TextContent); ok {
			text += value.Text
		}
	}
	return text, true
}

// connectClient joins an MCP client to a bridge through the helper's relay.
func connectClient(ctx context.Context, t *testing.T, directory string) (*mcp.ClientSession, func()) {
	t.Helper()
	toRelay, fromClient := io.Pipe()
	toClient, fromRelay := io.Pipe()
	stop := func() {
		_ = fromClient.Close()
		_ = toRelay.Close()
		_ = fromRelay.Close()
		_ = toClient.Close()
	}
	go func() { _ = Relay(directory, toRelay, fromRelay); _ = fromRelay.Close() }()
	client := mcp.NewClient(&mcp.Implementation{Name: "claude-code", Version: "test"}, nil)
	session, err := client.Connect(ctx, &mcp.IOTransport{Reader: toClient, Writer: fromClient}, nil)
	if err != nil {
		stop()
		t.Errorf("connect MCP client: %v", err)
		return nil, stop
	}
	return session, stop
}

// fakeLauncher starts one scripted child per launch, in order.
type fakeLauncher struct {
	t        *testing.T
	mu       sync.Mutex
	scripts  []func(*fakeClaude)
	children []*fakeClaude
}

func (launcher *fakeLauncher) launch(launch Launch) (Child, error) {
	launcher.mu.Lock()
	defer launcher.mu.Unlock()
	if len(launcher.children) >= len(launcher.scripts) {
		return nil, errors.New("unexpected launch")
	}
	script := launcher.scripts[len(launcher.children)]
	input, stdin := io.Pipe()
	stdout, output := io.Pipe()
	ctx, cancel := context.WithCancel(context.Background())
	fake := &fakeClaude{t: launcher.t, launch: launch, input: input, stdin: stdin, output: output, stdout: stdout,
		lines: bufio.NewReaderSize(input, 1<<20), ctx: ctx, cancel: cancel, closed: make(chan struct{})}
	launcher.children = append(launcher.children, fake)
	go script(fake)
	return fake, nil
}

func (launcher *fakeLauncher) child(index int) *fakeClaude {
	launcher.mu.Lock()
	defer launcher.mu.Unlock()
	if index >= len(launcher.children) {
		launcher.t.Fatalf("child %d was not launched", index)
	}
	return launcher.children[index]
}

func (launcher *fakeLauncher) count() int {
	launcher.mu.Lock()
	defer launcher.mu.Unlock()
	return len(launcher.children)
}

func newTestRoute(t *testing.T, scripts ...func(*fakeClaude)) (*Route, *fakeLauncher) {
	t.Helper()
	launcher := &fakeLauncher{t: t, scripts: scripts}
	route := NewRoute(Options{Launcher: launcher.launch, Executable: "sailry-test", Directory: t.TempDir()})
	t.Cleanup(route.Close)
	return route, launcher
}

func testBinding() model.Binding {
	return model.Binding{Config: protocol.SessionConfig{Model: "claude-test", Effort: protocol.Effort{Kind: "high"}}, Session: protocol.NewSessionID()}
}

var readTool = fantasy.FunctionTool{Name: "read", Description: "Read a file", InputSchema: map[string]any{"type": "object", "properties": map[string]any{"path": map[string]any{"type": "string"}}}}

func userMessage(text string) fantasy.Message {
	return fantasy.Message{Role: fantasy.MessageRoleUser, Content: []fantasy.MessagePart{fantasy.TextPart{Text: text}}}
}

func callMessage(calls ...fantasy.ToolCallPart) fantasy.Message {
	message := fantasy.Message{Role: fantasy.MessageRoleAssistant}
	for _, call := range calls {
		message.Content = append(message.Content, call)
	}
	return message
}

func resultMessage(identity, text string) fantasy.Message {
	return fantasy.Message{Role: fantasy.MessageRoleTool, Content: []fantasy.MessagePart{fantasy.ToolResultPart{ToolCallID: identity, Output: fantasy.ToolResultOutputContentText{Text: text}}}}
}

type stepOutput struct {
	text      string
	reasoning string
	calls     []fantasy.StreamPart
	finish    *fantasy.StreamPart
	failure   *model.Failure
}

func runStep(t *testing.T, ctx context.Context, route *Route, binding model.Binding, call fantasy.Call) stepOutput {
	t.Helper()
	stream, err := route.Model(binding).Stream(ctx, call)
	if err != nil {
		t.Fatalf("Stream: %v", err)
	}
	var output stepOutput
	for part := range stream {
		switch part.Type {
		case fantasy.StreamPartTypeTextDelta:
			output.text += part.Delta
		case fantasy.StreamPartTypeReasoningDelta:
			output.reasoning += part.Delta
		case fantasy.StreamPartTypeToolCall:
			output.calls = append(output.calls, part)
		case fantasy.StreamPartTypeFinish:
			finish := part
			output.finish = &finish
		case fantasy.StreamPartTypeError:
			if !errors.As(part.Error, &output.failure) {
				t.Fatalf("stream error is not a model failure: %v", part.Error)
			}
		}
	}
	return output
}

func requireFailure(t *testing.T, output stepOutput, kind model.FailureKind, contains string) {
	t.Helper()
	if output.failure == nil {
		t.Fatalf("step did not fail; text %q finish %+v", output.text, output.finish)
	}
	if output.failure.Kind != kind || !strings.Contains(output.failure.Message, contains) {
		t.Fatalf("failure = %s %q, want %s containing %q", output.failure.Kind, output.failure.Message, kind, contains)
	}
	if output.finish != nil {
		t.Fatalf("failed step also finished: %+v", output.finish)
	}
}

func waitClosed(t *testing.T, fake *fakeClaude) {
	t.Helper()
	select {
	case <-fake.closed:
	case <-time.After(testWait):
		t.Fatal("child was not closed")
	}
}

// Stream-json lines as Claude Code prints them.
func messageStart(identity string, input, cacheRead int) string {
	return fmt.Sprintf(`{"type":"stream_event","event":{"type":"message_start","message":{"id":%q,"usage":{"input_tokens":%d,"cache_read_input_tokens":%d,"output_tokens":1}}}}`, identity, input, cacheRead)
}

func textBlock(index int, text string) []string {
	delta, _ := json.Marshal(text)
	return []string{
		fmt.Sprintf(`{"type":"stream_event","event":{"type":"content_block_start","index":%d,"content_block":{"type":"text","text":""}}}`, index),
		fmt.Sprintf(`{"type":"stream_event","event":{"type":"content_block_delta","index":%d,"delta":{"type":"text_delta","text":%s}}}`, index, delta),
		fmt.Sprintf(`{"type":"stream_event","event":{"type":"content_block_stop","index":%d}}`, index),
	}
}

func thinkingBlock(index int, text string) []string {
	delta, _ := json.Marshal(text)
	return []string{
		fmt.Sprintf(`{"type":"stream_event","event":{"type":"content_block_start","index":%d,"content_block":{"type":"thinking","thinking":""}}}`, index),
		fmt.Sprintf(`{"type":"stream_event","event":{"type":"content_block_delta","index":%d,"delta":{"type":"thinking_delta","thinking":%s}}}`, index, delta),
		fmt.Sprintf(`{"type":"stream_event","event":{"type":"content_block_delta","index":%d,"delta":{"type":"signature_delta","signature":"sig"}}}`, index),
		fmt.Sprintf(`{"type":"stream_event","event":{"type":"content_block_stop","index":%d}}`, index),
	}
}

// callBlock streams a tool call with its arguments split in two deltas.
func callBlock(index int, identity, name, arguments string) []string {
	half := len(arguments) / 2
	first, _ := json.Marshal(arguments[:half])
	second, _ := json.Marshal(arguments[half:])
	return []string{
		fmt.Sprintf(`{"type":"stream_event","event":{"type":"content_block_start","index":%d,"content_block":{"type":"tool_use","id":%q,"name":%q,"input":%s`+"}}}", index, identity, name, "{"+"}"),
		fmt.Sprintf(`{"type":"stream_event","event":{"type":"content_block_delta","index":%d,"delta":{"type":"input_json_delta","partial_json":%s}}}`, index, first),
		fmt.Sprintf(`{"type":"stream_event","event":{"type":"content_block_delta","index":%d,"delta":{"type":"input_json_delta","partial_json":%s}}}`, index, second),
		fmt.Sprintf(`{"type":"stream_event","event":{"type":"content_block_stop","index":%d}}`, index),
	}
}

func messageEnd(stop string, output int) []string {
	return []string{
		fmt.Sprintf(`{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":%q},"usage":{"output_tokens":%d}}}`, stop, output),
		`{"type":"stream_event","event":{"type":"message_stop"}}`,
	}
}

const turnSucceeded = `{"type":"result","subtype":"success","is_error":false,"result":"done"}`

func turnFailed(text string, status int) string {
	quoted, _ := json.Marshal(text)
	return fmt.Sprintf(`{"type":"result","subtype":"error_during_execution","is_error":true,"result":%s,"api_error_status":%d}`, quoted, status)
}

func TestToolName(t *testing.T) {
	for name, want := range map[string]string{
		"read_file": "read_file",
		"a-b_C9":    "a-b_C9",
	} {
		if got := ToolName(name); got != want {
			t.Errorf("ToolName(%q) = %q, want %q", name, got, want)
		}
	}
	dotted, underscored := ToolName("spawn.agent"), ToolName("spawn_agent")
	if dotted == underscored || !strings.HasPrefix(dotted, "spawn_agent_") || !toolNamePlain.MatchString(dotted) {
		t.Errorf("ToolName(spawn.agent) = %q beside %q", dotted, underscored)
	}
	if again := ToolName("spawn.agent"); again != dotted {
		t.Errorf("ToolName is not stable: %q then %q", dotted, again)
	}
	long := ToolName(strings.Repeat("x", 80))
	if len(toolPrefix+long) > 64 {
		t.Errorf("long name is %d characters with its prefix", len(toolPrefix+long))
	}
	// A plain name shaped like a hashed one is hashed too, so the two never collide.
	if shaped := "tool_0123456789ab"; ToolName(shaped) == shaped {
		t.Errorf("ToolName(%q) kept a hashed shape", shaped)
	}
}

func TestArguments(t *testing.T) {
	arguments := Arguments(Settings{Model: "claude-test", Effort: "high", Config: `{"mcpServers":{}}`, System: "/tmp/system.md"})
	joined := strings.Join(arguments, "\x00")
	for _, want := range []string{
		"-p", "--input-format\x00stream-json", "--output-format\x00stream-json", "--include-partial-messages", "--verbose",
		"--model\x00claude-test", "--tools\x00\x00--strict-mcp-config", "--setting-sources\x00\x00--settings",
		"--no-session-persistence", "--permission-mode\x00dontAsk", "--allowedTools\x00mcp__sailry",
		"--append-system-prompt-file\x00/tmp/system.md", "--effort\x00high",
	} {
		if !strings.Contains(joined, want) {
			t.Errorf("arguments lack %q: %q", want, arguments)
		}
	}
	for _, refused := range []string{"--dangerously-skip-permissions", "--system-prompt", "--system-prompt-file", "--bare"} {
		for _, argument := range arguments {
			if argument == refused {
				t.Errorf("arguments contain %s", refused)
			}
		}
	}
	bare := Arguments(Settings{Model: "claude-test"})
	for _, argument := range bare {
		if argument == "--mcp-config" || argument == "--effort" || argument == "--append-system-prompt-file" {
			t.Errorf("empty settings still pass %s", argument)
		}
	}
	set, unset := Environment()
	for name := range set {
		if strings.Contains(name, "ENTRYPOINT") || name == "BUN_OPTIONS" || name == "IS_SANDBOX" || name == "CLAUDE_CONFIG_DIR" {
			t.Errorf("environment sets %s", name)
		}
	}
	if !strings.Contains(strings.Join(unset, ","), "ANTHROPIC_API_KEY") || !strings.Contains(strings.Join(unset, ","), "CLAUDE_CODE_ENTRYPOINT") {
		t.Errorf("environment keeps a stray key or entrypoint: %v", unset)
	}
}

func TestFailureClasses(t *testing.T) {
	for _, test := range []struct {
		text   string
		status int
		kind   model.FailureKind
	}{
		{"Not logged in · Please run /login", 0, model.Authentication},
		{"API Error: 401 authentication_error", 401, model.Authentication},
		{"API Error: 400 Third-party apps now draw from your extra usage, not your plan limits.", 400, model.Quota},
		{"API Error: 400 Third-party apps now draw from extra usage, not plan limits. Ask your workspace admin.", 400, model.Quota},
		{"Prompt is too long", 400, model.ContextOverflow},
		{"API Error: 429 rate_limit_error", 429, model.RateLimited},
		{"API Error: 529 overloaded", 529, model.Unavailable},
		{"something else", 0, model.Other},
	} {
		if got := failure(test.text, test.status); got.Kind != test.kind {
			t.Errorf("failure(%q, %d) = %s, want %s", test.text, test.status, got.Kind, test.kind)
		}
	}
}

func TestDecoder(t *testing.T) {
	t.Run("a whole message that was not streamed is told once", func(t *testing.T) {
		decoder := newDecoder()
		line := `{"type":"assistant","message":{"id":"m1","model":"claude-test","stop_reason":"tool_use","usage":{"input_tokens":3,"output_tokens":4},"content":[{"type":"thinking","thinking":"hm"},{"type":"text","text":"hi"},{"type":"tool_use","id":"toolu_1","name":"mcp__sailry__read","input":{"path":"a"}}]}}`
		events := decoder.line([]byte(line))
		var kinds []eventKind
		for _, received := range events {
			kinds = append(kinds, received.kind)
		}
		want := []eventKind{eventMessage, eventUsage, eventReasoning, eventReasoningEnd, eventText, eventCall, eventUsage, eventMessageEnd}
		if fmt.Sprint(kinds) != fmt.Sprint(want) {
			t.Fatalf("kinds = %v, want %v", kinds, want)
		}
		if call := events[5]; call.identity != "toolu_1" || call.name != "mcp__sailry__read" || string(call.arguments) != `{"path":"a"}` {
			t.Fatalf("call = %+v", call)
		}
	})
	t.Run("a streamed message's whole line says nothing more", func(t *testing.T) {
		decoder := newDecoder()
		decoder.line([]byte(messageStart("m1", 1, 0)))
		if events := decoder.line([]byte(`{"type":"assistant","message":{"id":"m1","model":"claude-test","content":[{"type":"text","text":"hi"}]}}`)); len(events) != 0 {
			t.Fatalf("events = %+v", events)
		}
	})
	t.Run("a synthetic message and an unknown line are ignored", func(t *testing.T) {
		decoder := newDecoder()
		for _, line := range []string{
			`{"type":"assistant","message":{"id":"e1","model":"<synthetic>","content":[{"type":"text","text":"API Error"}]}}`,
			`{"type":"prompt_suggestion","text":"x"}`,
			`not json`,
		} {
			if events := decoder.line([]byte(line)); len(events) != 0 {
				t.Errorf("%s gave %+v", line, events)
			}
		}
	})
	t.Run("a rejected limit carries its reset", func(t *testing.T) {
		events := newDecoder().line([]byte(`{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","rateLimitType":"five_hour","resetsAt":1800000000}}`))
		if len(events) != 1 || !events[0].rejected || events[0].resets.Unix() != 1800000000 {
			t.Fatalf("events = %+v", events)
		}
	})
}

func TestBridge(t *testing.T) {
	bridge, err := NewBridge(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(bridge.Close)
	changed, err := bridge.SetTools([]Tool{{Name: "read", Description: "Read", Schema: json.RawMessage(`{"type":"object"}`)}, {Name: "spawn.agent", Description: "Spawn", Schema: json.RawMessage(`{"type":"object"}`)}})
	if err != nil || !changed {
		t.Fatalf("SetTools = %v, %v", changed, err)
	}
	if changed, err := bridge.SetTools([]Tool{{Name: "spawn.agent", Description: "Spawn", Schema: json.RawMessage(`{"type":"object"}`)}, {Name: "read", Description: "Read", Schema: json.RawMessage(`{"type":"object"}`)}}); err != nil || changed {
		t.Fatalf("the same tools in another order changed: %v, %v", changed, err)
	}
	if _, err := bridge.SetTools([]Tool{{Name: "bad", Schema: json.RawMessage(`{"oneOf":[]}`)}}); err == nil {
		t.Fatal("a schema without an object root was accepted")
	}

	ctx, cancel := context.WithTimeout(context.Background(), testWait)
	defer cancel()
	session, stop := connectClient(ctx, t, bridge.Directory())
	if session == nil {
		t.FailNow()
	}
	defer stop()

	t.Run("lists the offered names", func(t *testing.T) {
		listed, err := session.ListTools(ctx, nil)
		if err != nil {
			t.Fatal(err)
		}
		names := map[string]bool{}
		for _, tool := range listed.Tools {
			names[tool.Name] = true
		}
		if len(names) != 2 || !names["read"] || !names[ToolName("spawn.agent")] {
			t.Fatalf("listed %v", names)
		}
	})
	t.Run("a call parks until its result", func(t *testing.T) {
		answered := make(chan *mcp.CallToolResult, 1)
		go func() {
			result, err := session.CallTool(ctx, &mcp.CallToolParams{Meta: mcp.Meta{callIdentity: "toolu_a"}, Name: "read", Arguments: map[string]any{"path": "a"}})
			if err != nil {
				t.Errorf("CallTool: %v", err)
			}
			answered <- result
		}()
		select {
		case <-answered:
			t.Fatal("the call returned before its result")
		case <-time.After(50 * time.Millisecond):
		}
		bridge.Resolve("toolu_a", Result{Text: "contents", Media: []Media{{Mime: "image/png", Data: []byte{1, 2, 3}}}})
		result := <-answered
		if result == nil || result.IsError || len(result.Content) != 2 {
			t.Fatalf("result = %+v", result)
		}
		if text, ok := result.Content[0].(*mcp.TextContent); !ok || text.Text != "contents" {
			t.Fatalf("text = %+v", result.Content[0])
		}
		if image, ok := result.Content[1].(*mcp.ImageContent); !ok || image.MIMEType != "image/png" || string(image.Data) != "\x01\x02\x03" {
			t.Fatalf("image = %+v", result.Content[1])
		}
	})
	t.Run("a result that came first answers its call at once", func(t *testing.T) {
		bridge.Resolve("toolu_b", Result{Text: "failed", Error: true})
		result, err := session.CallTool(ctx, &mcp.CallToolParams{Meta: mcp.Meta{callIdentity: "toolu_b"}, Name: "read", Arguments: map[string]any{}})
		if err != nil || !result.IsError {
			t.Fatalf("result = %+v, %v", result, err)
		}
	})
	t.Run("a call without an identity is answered as an error", func(t *testing.T) {
		result, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "read", Arguments: map[string]any{}})
		if err != nil || !result.IsError {
			t.Fatalf("result = %+v, %v", result, err)
		}
	})
	t.Run("a wrong token is refused", func(t *testing.T) {
		connection, err := net.Dial("unix", filepath.Join(bridge.Directory(), socketFile))
		if err != nil {
			t.Fatal(err)
		}
		defer connection.Close()
		if _, err := io.WriteString(connection, strings.Repeat("0", 64)+"\n"); err != nil {
			t.Fatal(err)
		}
		_ = connection.SetReadDeadline(time.Now().Add(testWait))
		// The bridge closes without answering; a deadline error would mean it kept the connection.
		count, err := connection.Read(make([]byte, 1))
		var timeout net.Error
		if count != 0 || err == nil || (errors.As(err, &timeout) && timeout.Timeout()) {
			t.Fatalf("read after a wrong token = %d, %v; want the connection closed", count, err)
		}
	})
}

func TestRoute(t *testing.T) {
	t.Run("plain reply with thinking", func(t *testing.T) {
		route, launcher := newTestRoute(t, func(fake *fakeClaude) {
			prompt, ok := fake.next()
			if !ok || prompt != "hello" {
				fake.t.Errorf("prompt = %q, %v", prompt, ok)
			}
			fake.emit(`{"type":"system","subtype":"init","session_id":"s1"}`, messageStart("m1", 10, 5))
			fake.emit(thinkingBlock(0, "let me think")...)
			fake.emit(textBlock(1, "hi there")...)
			fake.emit(messageEnd("end_turn", 7)...)
			fake.emit(turnSucceeded)
			fake.next()
		})
		call := fantasy.Call{Prompt: fantasy.Prompt{
			{Role: fantasy.MessageRoleSystem, Content: []fantasy.MessagePart{fantasy.TextPart{Text: "be brief"}}},
			userMessage("hello"),
		}}
		output := runStep(t, context.Background(), route, testBinding(), call)
		if output.failure != nil {
			t.Fatalf("failure: %v", output.failure)
		}
		if output.text != "hi there" || output.reasoning != "let me think" || len(output.calls) != 0 {
			t.Fatalf("output = %+v", output)
		}
		if output.finish == nil || output.finish.FinishReason != fantasy.FinishReasonStop {
			t.Fatalf("finish = %+v", output.finish)
		}
		// Input is the uncached count plus the cache read, as on the Messages route.
		if usage := output.finish.Usage; usage.InputTokens != 15 || usage.CacheReadTokens != 5 || usage.OutputTokens != 7 || usage.TotalTokens != 22 {
			t.Fatalf("usage = %+v", usage)
		}
		child := launcher.child(0)
		// A request without tools never keeps its child.
		waitClosed(t, child)
		arguments := strings.Join(child.launch.Arguments, "\x00")
		if strings.Contains(arguments, "--mcp-config") || !strings.Contains(arguments, "--effort\x00high") || !strings.Contains(arguments, "--append-system-prompt-file") {
			t.Fatalf("arguments = %q", child.launch.Arguments)
		}
		if filepath.Base(child.launch.Directory) != "work" {
			t.Fatalf("working folder = %q", child.launch.Directory)
		}
	})

	t.Run("tool calls park, and later steps go on in the same process", func(t *testing.T) {
		second := make(chan string, 1)
		route, launcher := newTestRoute(t, func(fake *fakeClaude) {
			if prompt, ok := fake.next(); !ok || prompt != "read both" {
				fake.t.Errorf("prompt = %q, %v", prompt, ok)
			}
			fake.emit(messageStart("m1", 20, 0))
			fake.emit(textBlock(0, "reading")...)
			fake.emit(callBlock(1, "toolu_1", toolPrefix+"read", `{"path":"a"}`)...)
			fake.emit(callBlock(2, "toolu_2", toolPrefix+"read", `{"path":"b"}`)...)
			fake.emit(messageEnd("tool_use", 9)...)
			// Claude Code makes a message's calls one after another.
			first, ok := fake.call("toolu_1", "read", map[string]any{"path": "a"})
			if !ok {
				return
			}
			other, ok := fake.call("toolu_2", "read", map[string]any{"path": "b"})
			if !ok {
				return
			}
			fake.emit(messageStart("m2", 30, 20))
			fake.emit(textBlock(0, first+"+"+other)...)
			fake.emit(messageEnd("end_turn", 3)...)
			fake.emit(turnSucceeded)
			prompt, ok := fake.next()
			if !ok {
				return
			}
			second <- prompt
			fake.emit(messageStart("m3", 40, 30))
			fake.emit(textBlock(0, "again")...)
			fake.emit(messageEnd("end_turn", 2)...)
			fake.emit(turnSucceeded)
			fake.next()
		})
		binding := testBinding()
		tools := []fantasy.Tool{readTool}
		first := userMessage("read both")
		output := runStep(t, context.Background(), route, binding, fantasy.Call{Prompt: fantasy.Prompt{first}, Tools: tools})
		if output.failure != nil {
			t.Fatalf("failure: %v", output.failure)
		}
		if output.text != "reading" || len(output.calls) != 2 || output.finish == nil || output.finish.FinishReason != fantasy.FinishReasonToolCalls {
			t.Fatalf("first step = %+v", output)
		}
		if call := output.calls[0]; call.ID != "toolu_1" || call.ToolCallName != "read" || call.ToolCallInput != `{"path":"a"}` {
			t.Fatalf("first call = %+v", call)
		}
		if call := output.calls[1]; call.ID != "toolu_2" || call.ToolCallInput != `{"path":"b"}` {
			t.Fatalf("second call = %+v", call)
		}

		calls := callMessage(
			fantasy.ToolCallPart{ToolCallID: "toolu_1", ToolName: "read", Input: `{"path":"a"}`},
			fantasy.ToolCallPart{ToolCallID: "toolu_2", ToolName: "read", Input: `{"path":"b"}`})
		prompt := fantasy.Prompt{first, calls, resultMessage("toolu_1", "alpha"), resultMessage("toolu_2", "beta")}
		output = runStep(t, context.Background(), route, binding, fantasy.Call{Prompt: prompt, Tools: tools})
		if output.failure != nil {
			t.Fatalf("failure: %v", output.failure)
		}
		if output.text != "alpha+beta" || output.finish == nil || output.finish.FinishReason != fantasy.FinishReasonStop {
			t.Fatalf("second step = %+v", output)
		}

		reply := fantasy.Message{Role: fantasy.MessageRoleAssistant, Content: []fantasy.MessagePart{fantasy.TextPart{Text: "alpha+beta"}}}
		prompt = append(prompt, reply, userMessage("and again"))
		output = runStep(t, context.Background(), route, binding, fantasy.Call{Prompt: prompt, Tools: tools})
		if output.failure != nil {
			t.Fatalf("failure: %v", output.failure)
		}
		if output.text != "again" {
			t.Fatalf("third step = %+v", output)
		}
		// The living child is told only the new message, not the conversation.
		if told := <-second; told != "and again" {
			t.Fatalf("second turn was told %q", told)
		}
		if launcher.count() != 1 {
			t.Fatalf("%d children were launched for one conversation", launcher.count())
		}
		arguments := strings.Join(launcher.child(0).launch.Arguments, "\x00")
		if !strings.Contains(arguments, "--allowedTools\x00mcp__sailry") {
			t.Fatalf("arguments = %q", launcher.child(0).launch.Arguments)
		}
	})

	t.Run("a diverged history starts a new process told the whole conversation", func(t *testing.T) {
		replayed := make(chan string, 1)
		route, launcher := newTestRoute(t,
			func(fake *fakeClaude) {
				fake.next()
				fake.emit(messageStart("m1", 5, 0))
				fake.emit(textBlock(0, "first answer")...)
				fake.emit(messageEnd("end_turn", 2)...)
				fake.emit(turnSucceeded)
				fake.next()
			},
			func(fake *fakeClaude) {
				prompt, ok := fake.next()
				if !ok {
					return
				}
				replayed <- prompt
				fake.emit(messageStart("m9", 5, 0))
				fake.emit(textBlock(0, "second answer")...)
				fake.emit(messageEnd("end_turn", 2)...)
				fake.emit(turnSucceeded)
				fake.next()
			})
		binding := testBinding()
		tools := []fantasy.Tool{readTool}
		output := runStep(t, context.Background(), route, binding, fantasy.Call{Prompt: fantasy.Prompt{userMessage("original")}, Tools: tools})
		if output.failure != nil || output.text != "first answer" {
			t.Fatalf("first step = %+v", output)
		}
		// The user edited the first message: the child's history no longer matches.
		edited := fantasy.Prompt{
			userMessage("edited"),
			callMessage(fantasy.ToolCallPart{ToolCallID: "toolu_old", ToolName: "read", Input: `{"path":"x"}`}),
			resultMessage("toolu_old", "old contents"),
			{Role: fantasy.MessageRoleAssistant, Content: []fantasy.MessagePart{fantasy.TextPart{Text: "an earlier answer"}}},
			userMessage("go on"),
		}
		output = runStep(t, context.Background(), route, binding, fantasy.Call{Prompt: edited, Tools: tools})
		if output.failure != nil || output.text != "second answer" {
			t.Fatalf("second step = %+v", output)
		}
		if launcher.count() != 2 {
			t.Fatalf("%d children were launched", launcher.count())
		}
		waitClosed(t, launcher.child(0))
		told := <-replayed
		for _, want := range []string{
			"Human: edited",
			"[tool call mcp__sailry__read id=toolu_old args={\"path\":\"x\"}]",
			"[tool result id=toolu_old]\nold contents\n[end of tool result id=toolu_old]",
			"Assistant: an earlier answer",
			"Human: go on",
		} {
			if !strings.Contains(told, want) {
				t.Errorf("replay lacks %q:\n%s", want, told)
			}
		}
		if strings.Contains(told, "original") {
			t.Errorf("replay holds the replaced message:\n%s", told)
		}
	})

	t.Run("another model starts a new process", func(t *testing.T) {
		script := func(fake *fakeClaude) {
			fake.next()
			fake.emit(messageStart("m1", 5, 0))
			fake.emit(textBlock(0, "ok")...)
			fake.emit(messageEnd("end_turn", 2)...)
			fake.emit(turnSucceeded)
			fake.next()
		}
		route, launcher := newTestRoute(t, script, script)
		binding := testBinding()
		tools := []fantasy.Tool{readTool}
		first := userMessage("one")
		if output := runStep(t, context.Background(), route, binding, fantasy.Call{Prompt: fantasy.Prompt{first}, Tools: tools}); output.failure != nil {
			t.Fatalf("failure: %v", output.failure)
		}
		binding.Config.Model = "claude-other"
		prompt := fantasy.Prompt{first, {Role: fantasy.MessageRoleAssistant, Content: []fantasy.MessagePart{fantasy.TextPart{Text: "ok"}}}, userMessage("two")}
		if output := runStep(t, context.Background(), route, binding, fantasy.Call{Prompt: prompt, Tools: tools}); output.failure != nil {
			t.Fatalf("failure: %v", output.failure)
		}
		if launcher.count() != 2 {
			t.Fatalf("%d children were launched", launcher.count())
		}
		if arguments := strings.Join(launcher.child(1).launch.Arguments, "\x00"); !strings.Contains(arguments, "--model\x00claude-other") {
			t.Fatalf("arguments = %q", launcher.child(1).launch.Arguments)
		}
	})

	t.Run("stopping a step ends the process", func(t *testing.T) {
		route, launcher := newTestRoute(t, func(fake *fakeClaude) {
			fake.next()
			fake.emit(messageStart("m1", 5, 0))
			fake.emit(textBlock(0, "partial")...)
			fake.next()
		})
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		stream, err := route.Model(testBinding()).Stream(ctx, fantasy.Call{Prompt: fantasy.Prompt{userMessage("long task")}, Tools: []fantasy.Tool{readTool}})
		if err != nil {
			t.Fatal(err)
		}
		var failure *model.Failure
		text := ""
		for part := range stream {
			switch part.Type {
			case fantasy.StreamPartTypeTextDelta:
				text += part.Delta
				cancel()
			case fantasy.StreamPartTypeError:
				if !errors.As(part.Error, &failure) {
					t.Fatalf("error = %v", part.Error)
				}
			}
		}
		if text != "partial" || failure == nil || failure.Kind != model.Cancelled {
			t.Fatalf("text %q, failure %+v", text, failure)
		}
		waitClosed(t, launcher.child(0))
	})

	t.Run("a process that ends mid-reply fails the step with its error output", func(t *testing.T) {
		route, launcher := newTestRoute(t, func(fake *fakeClaude) {
			fake.next()
			fake.emit(messageStart("m1", 5, 0))
			fake.exit("panic: boom")
		})
		output := runStep(t, context.Background(), route, testBinding(), fantasy.Call{Prompt: fantasy.Prompt{userMessage("hi")}, Tools: []fantasy.Tool{readTool}})
		requireFailure(t, output, model.Unavailable, "boom")
		waitClosed(t, launcher.child(0))
	})

	t.Run("not signed in", func(t *testing.T) {
		route, _ := newTestRoute(t, func(fake *fakeClaude) {
			fake.next()
			fake.emit(`{"type":"assistant","message":{"id":"e1","model":"<synthetic>","content":[{"type":"text","text":"Not logged in · Please run /login"}]}}`)
			fake.emit(turnFailed("Not logged in · Please run /login", 0))
			fake.next()
		})
		output := runStep(t, context.Background(), route, testBinding(), fantasy.Call{Prompt: fantasy.Prompt{userMessage("hi")}})
		requireFailure(t, output, model.Authentication, "not signed in")
		if output.text != "" {
			t.Fatalf("the refusal was shown as a reply: %q", output.text)
		}
	})

	t.Run("refused as third-party use", func(t *testing.T) {
		const refusal = "API Error: 400 Third-party apps now draw from your extra usage, not your plan limits."
		route, _ := newTestRoute(t, func(fake *fakeClaude) {
			fake.next()
			fake.emit(turnFailed(refusal, 400))
			fake.next()
		})
		output := runStep(t, context.Background(), route, testBinding(), fantasy.Call{Prompt: fantasy.Prompt{userMessage("hi")}})
		requireFailure(t, output, model.Quota, "Third-party apps")
		if output.failure.Retryable() {
			t.Fatal("a content refusal must not be retried")
		}
	})

	t.Run("a rejected limit is a rate limit with its reset", func(t *testing.T) {
		resets := time.Now().Add(time.Hour).Unix()
		route, _ := newTestRoute(t, func(fake *fakeClaude) {
			fake.next()
			fake.emit(fmt.Sprintf(`{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","rateLimitType":"five_hour","resetsAt":%d}}`, resets))
			fake.emit(turnFailed("You've hit your session limit", 0))
			fake.next()
		})
		output := runStep(t, context.Background(), route, testBinding(), fantasy.Call{Prompt: fantasy.Prompt{userMessage("hi")}})
		requireFailure(t, output, model.RateLimited, "limit")
		if output.failure.RetryAfter == nil || *output.failure.RetryAfter <= 0 {
			t.Fatalf("retry after = %v", output.failure.RetryAfter)
		}
	})

	t.Run("not installed", func(t *testing.T) {
		route := NewRoute(Options{Launcher: func(Launch) (Child, error) { return nil, ErrNotInstalled }, Executable: "sailry-test", Directory: t.TempDir()})
		t.Cleanup(route.Close)
		output := runStep(t, context.Background(), route, testBinding(), fantasy.Call{Prompt: fantasy.Prompt{userMessage("hi")}})
		requireFailure(t, output, model.NotConfigured, "not installed")
	})

	t.Run("a reply cut at the output limit is an answer", func(t *testing.T) {
		route, _ := newTestRoute(t, func(fake *fakeClaude) {
			fake.next()
			fake.emit(messageStart("m1", 5, 0))
			fake.emit(textBlock(0, "cut he")...)
			fake.emit(messageEnd("max_tokens", 64)...)
			fake.emit(turnSucceeded)
			fake.next()
		})
		output := runStep(t, context.Background(), route, testBinding(), fantasy.Call{Prompt: fantasy.Prompt{userMessage("hi")}})
		if output.failure != nil || output.text != "cut he" || output.finish == nil || output.finish.FinishReason != fantasy.FinishReasonLength {
			t.Fatalf("output = %+v", output)
		}
	})
}
