package model

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"syscall"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/program"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The test binary is its own agent: started with SAILRY_ACP_FIXTURE it
// speaks the Agent Client Protocol on its standard streams, replies from
// script.json in its working directory and logs every line it receives to
// requests.log there.
func TestMain(suite *testing.M) {
	if scenario, fixture := os.LookupEnv("SAILRY_ACP_FIXTURE"); fixture {
		acpFixtureAgent(scenario)
		return
	}
	os.Exit(suite.Run())
}

type acpFixtureReply struct {
	Thoughts   []string       `json:"thoughts"`
	Chunks     []string       `json:"chunks"`
	Stop       string         `json:"stop"`
	Usage      map[string]any `json:"usage"`
	Permission bool           `json:"permission"`
	Hang       bool           `json:"hang"`
	Crash      bool           `json:"crash"`
}

func acpFixtureAgent(scenario string) {
	previous, _ := os.ReadFile("requests.log")
	prompts := strings.Count(string(previous), `"method":"session/prompt"`)
	log, err := os.OpenFile("requests.log", os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
	if err != nil {
		os.Exit(2)
	}
	record := func(value any) {
		line, _ := json.Marshal(value)
		log.Write(append(line, '\n'))
	}
	record(map[string]any{"started": os.Args[1:], "pid": os.Getpid(), "key": os.Getenv("FIXTURE_KEY"), "home": os.Getenv("HOME"), "leak": os.Getenv("FIXTURE_LEAK")})
	var script []acpFixtureReply
	if raw, err := os.ReadFile("script.json"); err == nil {
		json.Unmarshal(raw, &script)
	}
	send := func(value map[string]any) {
		value["jsonrpc"] = "2.0"
		line, _ := json.Marshal(value)
		os.Stdout.Write(append(line, '\n'))
	}
	has := func(flag string) bool { return strings.Contains(","+scenario+",", ","+flag+",") }
	sessions, authenticated := 0, false
	var pending json.RawMessage
	var waiting *acpFixtureReply
	session := ""
	answer := func(reply acpFixtureReply) {
		update := func(kind, text string) {
			send(map[string]any{"method": "session/update", "params": map[string]any{"sessionId": session, "update": map[string]any{
				"sessionUpdate": kind, "content": map[string]any{"type": "text", "text": text}}}})
		}
		for _, text := range reply.Thoughts {
			update("agent_thought_chunk", text)
		}
		for _, text := range reply.Chunks {
			update("agent_message_chunk", text)
		}
		if reply.Crash {
			fmt.Fprintln(os.Stderr, "fixture agent crashed")
			os.Exit(3)
		}
		if reply.Hang {
			return
		}
		stop := reply.Stop
		if stop == "" {
			stop = "end_turn"
		}
		result := map[string]any{"stopReason": stop}
		if reply.Usage != nil {
			result["usage"] = reply.Usage
		}
		send(map[string]any{"id": pending, "result": result})
		pending = nil
	}
	lines := bufio.NewScanner(os.Stdin)
	lines.Buffer(make([]byte, 0, 1024*1024), 16*1024*1024)
	for lines.Scan() {
		log.Write(append(append([]byte(nil), lines.Bytes()...), '\n'))
		var message struct {
			ID     json.RawMessage `json:"id"`
			Method string          `json:"method"`
			Params struct {
				SessionID string `json:"sessionId"`
			} `json:"params"`
		}
		if json.Unmarshal(lines.Bytes(), &message) != nil {
			continue
		}
		switch message.Method {
		case "initialize":
			capabilities := map[string]any{"promptCapabilities": map[string]any{"image": true}}
			if has("meta") {
				capabilities["_meta"] = map[string]any{"vendor": map[string]any{"systemPrompt": map[string]any{"append": true}}}
			}
			send(map[string]any{"id": message.ID, "result": map[string]any{"protocolVersion": 1, "agentCapabilities": capabilities, "authMethods": []any{}}})
		case "authenticate":
			authenticated = true
			send(map[string]any{"id": message.ID, "result": map[string]any{}})
		case "session/new":
			if has("signed-out") || has("login") && !authenticated {
				send(map[string]any{"id": message.ID, "error": map[string]any{"code": -32000, "message": "Authentication required"}})
				continue
			}
			sessions++
			session = fmt.Sprintf("session-%d-%d", os.Getpid(), sessions)
			result := map[string]any{"sessionId": session}
			switch {
			case has("variants"):
				result["models"] = map[string]any{"availableModels": []any{
					map[string]any{"modelId": "captured-low", "name": "Captured (Low)"},
					map[string]any{"modelId": "captured-high", "name": "Captured (High)"},
					map[string]any{"modelId": "plain", "name": "Plain"},
				}}
			case has("options"):
				result["configOptions"] = []any{
					map[string]any{"id": "model", "category": "model", "type": "select", "currentValue": "other", "options": []any{
						map[string]any{"value": "captured", "name": "Captured"}, map[string]any{"value": "other", "name": "Other"}}},
					map[string]any{"id": "thought_level", "category": "thought_level", "type": "select", "currentValue": "low", "options": []any{
						map[string]any{"value": "low", "name": "Low"}, map[string]any{"value": "high", "name": "High"}}},
				}
			case !has("unlisted"):
				result["models"] = map[string]any{"availableModels": []any{map[string]any{"modelId": "captured", "name": "Captured"}}}
			}
			send(map[string]any{"id": message.ID, "result": result})
		case "session/set_model":
			send(map[string]any{"id": message.ID, "result": map[string]any{}})
		case "session/set_config_option":
			send(map[string]any{"id": message.ID, "result": map[string]any{"configOptions": []any{}}})
		case "session/prompt":
			reply := acpFixtureReply{Chunks: []string{"ok"}}
			if prompts < len(script) {
				reply = script[prompts]
			}
			prompts++
			pending, session = message.ID, message.Params.SessionID
			if reply.Permission {
				waiting = &reply
				send(map[string]any{"id": 900, "method": "session/request_permission", "params": map[string]any{"sessionId": session,
					"toolCall": map[string]any{"toolCallId": "builtin"},
					"options":  []any{map[string]any{"optionId": "yes", "name": "Allow", "kind": "allow_once"}, map[string]any{"optionId": "no", "name": "Reject", "kind": "reject_once"}}}})
				continue
			}
			answer(reply)
		case "session/cancel":
			if pending != nil {
				send(map[string]any{"id": pending, "result": map[string]any{"stopReason": "cancelled"}})
				pending = nil
			}
		case "":
			// The answer to the permission request releases the held reply.
			if waiting != nil {
				reply := *waiting
				waiting = nil
				answer(reply)
			}
		default:
			if message.ID != nil {
				send(map[string]any{"id": message.ID, "error": map[string]any{"code": -32601, "message": "Method not found"}})
			}
		}
	}
}

type acpFixture struct {
	t     *testing.T
	model *acpModel
	dir   string
}

// acpFixtureModel builds the route over the fixture agent. The declaration
// is the first vendor shape: system text in session metadata, models as
// effort variants selected in the session.
func acpFixtureModel(t *testing.T, scenario string, script []acpFixtureReply, mutate func(*ACPCapture)) *acpFixture {
	t.Helper()
	if runtime.GOOS == "windows" {
		t.Skip("the fixture agent is linked by a symbolic link")
	}
	self, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	bin, dir := t.TempDir(), t.TempDir()
	if err := os.Symlink(self, filepath.Join(bin, "fixture-agent")); err != nil {
		t.Fatal(err)
	}
	raw, _ := json.Marshal(script)
	if err := os.WriteFile(filepath.Join(dir, "script.json"), raw, 0600); err != nil {
		t.Fatal(err)
	}
	_, binding, _ := pluginFixture(t, "", nil)
	meta := "vendor.systemPrompt.append"
	declaration := protocol.DefaultPluginModelAdapterDeclaration()
	declaration.ID, declaration.Framing = "fixture", protocol.PluginModelAdapterFramingNdjson
	declaration.Capabilities = protocol.PluginModelAdapterCapabilities{Vision: true, Files: true, Tools: true, Reasoning: true}
	declaration.Acp = &protocol.PluginModelAdapterAcp{Program: "agent", SystemMeta: &meta, ReplayBytes: 64 * 1024,
		SignIn: "Run fixture-agent once to sign in", Efforts: map[string]string{"low": "-low", "high": "-high"}}
	pool := program.NewPool(2, time.Minute)
	t.Cleanup(pool.Close)
	capture := ACPCapture{Adapter: declaration, Owner: "fixture", Dir: dir, Version: "0.0.0", Pool: pool,
		Host: []string{"PATH=" + bin, "HOME=" + dir, "FIXTURE_LEAK=host"},
		Program: protocol.PluginProgramDeclaration{ID: "agent", Command: "fixture-agent", Args: []string{"acp"},
			Env:   map[string]string{"SAILRY_ACP_FIXTURE": scenario},
			Files: []protocol.PluginProgramFile{{Path: "config/agent.md", Content: "stripped"}}},
	}
	if mutate != nil {
		mutate(&capture)
	}
	model, err := NewACP(binding, capture)
	if err != nil {
		t.Fatal(err)
	}
	return &acpFixture{t: t, model: model.(*acpModel), dir: dir}
}

type acpResult struct {
	text, reasoning string
	calls           []fantasy.StreamPart
	finish          fantasy.FinishReason
	usage           fantasy.Usage
	failure         *Failure
}

func acpRequest(t *testing.T, system, messages string, tools ...string) protocol.PluginModelAdapterWireRequest {
	t.Helper()
	request := protocol.DefaultPluginModelAdapterWireRequest()
	request.Version, request.Model, request.Stream = "v1", "captured", true
	request.ToolChoice = protocol.PluginModelAdapterWireChoice{Kind: "auto"}
	if system != "" {
		request.System = &system
	}
	if err := json.Unmarshal([]byte(messages), &request.Messages); err != nil {
		t.Fatal(err)
	}
	for _, name := range tools {
		request.Tools = append(request.Tools, protocol.PluginModelAdapterWireTool{Name: name, Parameters: json.RawMessage(`{"type":"object"}`)})
	}
	return request
}

func (fixture *acpFixture) run(ctx context.Context, request protocol.PluginModelAdapterWireRequest, each func(fantasy.StreamPart)) acpResult {
	fixture.t.Helper()
	var result acpResult
	err := fixture.model.stream(ctx, request, func(part fantasy.StreamPart) bool {
		switch part.Type {
		case fantasy.StreamPartTypeTextDelta:
			result.text += part.Delta
		case fantasy.StreamPartTypeReasoningDelta:
			result.reasoning += part.Delta
		case fantasy.StreamPartTypeToolCall:
			result.calls = append(result.calls, part)
		case fantasy.StreamPartTypeFinish:
			result.finish, result.usage = part.FinishReason, part.Usage
		}
		if each != nil {
			each(part)
		}
		return true
	})
	if err != nil {
		result.failure = Classify(err)
	}
	return result
}

// received returns the logged lines whose method is method ("" for all).
func (fixture *acpFixture) received(method string) []map[string]any {
	fixture.t.Helper()
	raw, err := os.ReadFile(filepath.Join(fixture.dir, "requests.log"))
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		fixture.t.Fatal(err)
	}
	var lines []map[string]any
	for _, line := range strings.Split(strings.TrimSpace(string(raw)), "\n") {
		var value map[string]any
		if json.Unmarshal([]byte(line), &value) == nil && (method == "" || value["method"] == method) {
			lines = append(lines, value)
		}
	}
	return lines
}

func (fixture *acpFixture) prompt(index int) string {
	fixture.t.Helper()
	prompts := fixture.received("session/prompt")
	if index >= len(prompts) {
		fixture.t.Fatalf("the agent received %d prompts", len(prompts))
	}
	blocks := prompts[index]["params"].(map[string]any)["prompt"].([]any)
	return blocks[0].(map[string]any)["text"].(string)
}

const acpUser = `[{"role":"user","parts":[{"kind":"text","text":"hi"}]}]`

func TestACPRoute(t *testing.T) {
	t.Run("streams a plain reply with reasoning and usage", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta,variants", []acpFixtureReply{{Thoughts: []string{"thin", "king"}, Chunks: []string{"Hel", "lo"},
			Usage: map[string]any{"inputTokens": 10, "cachedReadTokens": 30, "outputTokens": 7, "thoughtTokens": 5, "totalTokens": 47}}}, nil)
		request := acpRequest(t, "Be brief", acpUser)
		request.Reasoning.Effort = protocol.Effort{Kind: "high"}
		result := fixture.run(t.Context(), request, nil)
		if result.failure != nil || result.text != "Hello" || result.reasoning != "thinking" || result.finish != fantasy.FinishReasonStop {
			t.Fatalf("reply = %#v", result)
		}
		if result.usage.InputTokens != 40 || result.usage.OutputTokens != 7 || result.usage.CacheReadTokens != 30 || result.usage.ReasoningTokens != 5 {
			t.Fatalf("usage = %#v", result.usage)
		}
		initialize := fixture.received("initialize")[0]["params"].(map[string]any)["clientCapabilities"].(map[string]any)
		if initialize["terminal"] != false || initialize["fs"].(map[string]any)["writeTextFile"] != false {
			t.Fatalf("client capabilities = %#v", initialize)
		}
		created := fixture.received("session/new")[0]["params"].(map[string]any)
		system := created["_meta"].(map[string]any)["vendor"].(map[string]any)["systemPrompt"].(map[string]any)["append"].(string)
		if servers, ok := created["mcpServers"].([]any); !ok || len(servers) != 0 || created["cwd"] != fixture.dir ||
			!strings.HasPrefix(system, acpDiscipline) || !strings.HasSuffix(system, "Be brief") || strings.Contains(system, "available_tools") {
			t.Fatalf("session = %#v", created)
		}
		if selected := fixture.received("session/set_model"); len(selected) != 1 || selected[0]["params"].(map[string]any)["modelId"] != "captured-high" {
			t.Fatalf("model selection = %#v", selected)
		}
		if fixture.prompt(0) != "hi" {
			t.Fatalf("prompt = %q", fixture.prompt(0))
		}
		started := fixture.received("")[0]
		if started["home"] != fixture.dir || started["leak"] != "" {
			t.Fatalf("environment = %#v", started)
		}
		if written, err := os.ReadFile(filepath.Join(fixture.dir, "config", "agent.md")); err != nil || string(written) != "stripped" {
			t.Fatalf("declared file = %q, %v", written, err)
		}
	})

	t.Run("parses calls split across chunks and reuses the session for their results", func(t *testing.T) {
		call := `<tool_call>{"name":"read","arguments":{"path":"a </tool_call> b"}}</tool_call>`
		fixture := acpFixtureModel(t, "meta", []acpFixtureReply{
			{Chunks: []string{"Looking <", "tool", "_call>\n{\"name\":\"read\",\"argu", "ments\":{\"path\":\"a </tool_call> b\"}}\n</tool_", "call>" + call + " trailing chatter"}},
			{Chunks: []string{"done"}},
		}, nil)
		first := fixture.run(t.Context(), acpRequest(t, "", acpUser, "read"), nil)
		if first.failure != nil || first.text != "Looking " || first.finish != fantasy.FinishReasonToolCalls || len(first.calls) != 2 {
			t.Fatalf("reply = %#v", first)
		}
		for _, part := range first.calls {
			if part.ToolCallName != "read" || part.ToolCallInput != `{"path":"a </tool_call> b"}` || !strings.HasPrefix(part.ID, "call_") {
				t.Fatalf("call = %#v", part)
			}
		}
		// Two identical calls are two calls.
		if first.calls[0].ID == first.calls[1].ID {
			t.Fatal("identical parallel calls share an identifier")
		}
		if system := fixture.received("session/new")[0]["params"].(map[string]any)["_meta"]; !strings.Contains(fmt.Sprint(system), `"name":"read"`) {
			t.Fatalf("tools were not declared: %v", system)
		}
		history := fmt.Sprintf(`[{"role":"user","parts":[{"kind":"text","text":"hi"}]},
			{"role":"assistant","parts":[{"kind":"text","text":"Looking "},{"kind":"tool_call","id":%q,"name":"read","arguments":{"path":"x"}},{"kind":"tool_call","id":%q,"name":"read","arguments":{"path":"x"}}]},
			{"role":"tool","call_id":%q,"name":"read","is_error":false,"parts":[{"kind":"json","value":"first"}]},
			{"role":"tool","call_id":%q,"name":"read","is_error":true,"parts":[{"kind":"json","value":{"content":[{"type":"text","text":"second"}]}}]}]`,
			first.calls[0].ID, first.calls[1].ID, first.calls[0].ID, first.calls[1].ID)
		second := fixture.run(t.Context(), acpRequest(t, "", history, "read"), nil)
		if second.failure != nil || second.text != "done" || second.finish != fantasy.FinishReasonStop {
			t.Fatalf("reply = %#v", second)
		}
		if sessions := fixture.received("session/new"); len(sessions) != 1 {
			t.Fatalf("the session was not reused: %d sessions", len(sessions))
		}
		want := fmt.Sprintf("<tool_response id=%q name=\"read\">\nfirst\n</tool_response>\n\n<tool_response id=%q name=\"read\" error=\"true\">\nsecond\n</tool_response>", first.calls[0].ID, first.calls[1].ID)
		if fixture.prompt(1) != want {
			t.Fatalf("prompt = %q", fixture.prompt(1))
		}
	})

	t.Run("replays an edited history into a new session", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta", []acpFixtureReply{{Chunks: []string{"one"}}, {Chunks: []string{"two"}}}, nil)
		if result := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil {
			t.Fatal(result.failure)
		}
		edited := `[{"role":"user","parts":[{"kind":"text","text":"hello"}]},{"role":"assistant","parts":[{"kind":"text","text":"one"}]},{"role":"user","parts":[{"kind":"text","text":"again"}]}]`
		if result := fixture.run(t.Context(), acpRequest(t, "", edited), nil); result.failure != nil || result.text != "two" {
			t.Fatalf("reply = %#v", result)
		}
		if sessions := fixture.received("session/new"); len(sessions) != 2 {
			t.Fatalf("an edited history reused its session: %d sessions", len(sessions))
		}
		if fixture.prompt(1) != "[USER]\nhello\n\n[ASSISTANT]\none\n\nagain" {
			t.Fatalf("prompt = %q", fixture.prompt(1))
		}
		if started := fixture.received(""); len(fixture.received("initialize")) != 1 || started[0]["pid"] == nil {
			t.Fatal("a replay started another program")
		}
	})

	t.Run("refuses a history beyond the byte budget", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta", nil, func(capture *ACPCapture) { capture.Adapter.Acp.ReplayBytes = 4096 })
		large, _ := json.Marshal(strings.Repeat("x", 5000))
		result := fixture.run(t.Context(), acpRequest(t, "", `[{"role":"user","parts":[{"kind":"text","text":`+string(large)+`}]}]`), nil)
		if result.failure == nil || result.failure.Kind != ContextOverflow || len(fixture.received("session/prompt")) != 0 {
			t.Fatalf("failure = %#v", result.failure)
		}
	})

	t.Run("refuses the agent's permission requests", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta", []acpFixtureReply{{Permission: true, Chunks: []string{"refused"}}}, nil)
		if result := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil || result.text != "refused" {
			t.Fatalf("reply = %#v", result)
		}
		answered := false
		for _, line := range fixture.received("") {
			result, _ := line["result"].(map[string]any)
			if outcome, ok := result["outcome"].(map[string]any); ok {
				answered = outcome["outcome"] == "selected" && outcome["optionId"] == "no"
			}
		}
		if !answered {
			t.Fatal("the permission request was not rejected")
		}
	})

	t.Run("cancels the agent's turn", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta", []acpFixtureReply{{Chunks: []string{"partial"}, Hang: true}, {Chunks: []string{"next"}}}, nil)
		ctx, cancel := context.WithCancel(t.Context())
		result := fixture.run(ctx, acpRequest(t, "", acpUser), func(part fantasy.StreamPart) {
			if part.Type == fantasy.StreamPartTypeTextDelta {
				cancel()
			}
		})
		if result.failure == nil || result.failure.Kind != Cancelled || len(fixture.received("session/cancel")) != 1 {
			t.Fatalf("failure = %#v", result.failure)
		}
		// The program survived; what its session holds is unknown, so the next step replays.
		if next := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); next.failure != nil || next.text != "next" ||
			len(fixture.received("initialize")) != 1 || len(fixture.received("session/new")) != 2 {
			t.Fatalf("reply after cancellation = %#v", next)
		}
	})

	t.Run("reports a program that ends in the middle of a reply", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta", []acpFixtureReply{{Chunks: []string{"partial"}, Crash: true}, {Chunks: []string{"again"}}}, nil)
		result := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil)
		if result.failure == nil || result.failure.Kind != Network || !strings.Contains(result.failure.Message, "fixture-agent ended unexpectedly") {
			t.Fatalf("failure = %#v", result.failure)
		}
		if next := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); next.failure != nil || next.text != "again" || len(fixture.received("initialize")) != 2 {
			t.Fatalf("reply after the crash = %#v", next)
		}
	})

	t.Run("reports a malformed call and a reply cut inside a call as errors", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta", []acpFixtureReply{
			{Chunks: []string{`<tool_call>{"name":</tool_call><tool_call>{"name":"read","arguments":{}}</tool_call>`}},
			{Chunks: []string{`<tool_call>{"name":"read","argu`}},
		}, nil)
		for range 2 {
			result := fixture.run(t.Context(), acpRequest(t, "", acpUser, "read"), nil)
			if result.failure == nil || result.failure.Kind != InvalidResponse || len(result.calls) != 0 {
				t.Fatalf("reply = %#v", result)
			}
		}
	})

	t.Run("maps the stop reasons", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta", []acpFixtureReply{{Chunks: []string{"cut"}, Stop: "max_tokens"}, {Stop: "refusal"}}, nil)
		if result := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil || result.finish != fantasy.FinishReasonLength || result.text != "cut" {
			t.Fatalf("reply = %#v", result)
		}
		if result := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil || result.finish != fantasy.FinishReasonContentFilter {
			t.Fatalf("reply = %#v", result)
		}
	})

	t.Run("tells a missing program from a signed-out one", func(t *testing.T) {
		missing := acpFixtureModel(t, "meta", nil, func(capture *ACPCapture) { capture.Host = []string{"PATH=" + t.TempDir()} })
		if result := missing.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure == nil || result.failure.Kind != NotConfigured ||
			result.failure.Message != "fixture-agent is not installed or not on the search path" {
			t.Fatalf("failure = %#v", result.failure)
		}
		signedOut := acpFixtureModel(t, "signed-out", nil, nil)
		if result := signedOut.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure == nil || result.failure.Kind != Authentication ||
			result.failure.Message != "Run fixture-agent once to sign in" || len(signedOut.received("authenticate")) != 0 {
			t.Fatalf("failure = %#v", result.failure)
		}
	})

	t.Run("stops an idle program and leaves none behind", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta", nil, func(capture *ACPCapture) { capture.Pool = program.NewPool(1, 50*time.Millisecond) })
		t.Cleanup(fixture.model.capture.Pool.Close)
		if result := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil {
			t.Fatal(result.failure)
		}
		process, err := os.FindProcess(int(fixture.received("")[0]["pid"].(float64)))
		if err != nil {
			t.Fatal(err)
		}
		deadline := time.Now().Add(10 * time.Second)
		for process.Signal(syscall.Signal(0)) == nil {
			if time.Now().After(deadline) {
				t.Fatal("the idle program is still running")
			}
			time.Sleep(20 * time.Millisecond)
		}
		// The next step starts the program again.
		if result := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil || len(fixture.received("initialize")) != 2 {
			t.Fatalf("reply after the idle stop = %#v", result)
		}
	})
}

// A second vendor shape: no session metadata for the system text, the model
// chosen by a start argument, a key in the declared environment and an
// authentication method the agent is asked to use.
func TestACPRouteSecondVendor(t *testing.T) {
	method := "fixture_login"
	shape := func(capture *ACPCapture) {
		capture.Adapter.Acp.SystemMeta, capture.Adapter.Acp.Efforts = nil, nil
		capture.Adapter.Acp.ModelArgument, capture.Adapter.Acp.AuthMethod = true, &method
		capture.Program.Args = []string{"acp", "--model", "{model}"}
		capture.Program.Env["FIXTURE_KEY"] = "{key}"
	}
	fixture := acpFixtureModel(t, "login,unlisted", []acpFixtureReply{{Chunks: []string{"one"}}, {Chunks: []string{"two"}}}, shape)
	first := fixture.run(t.Context(), acpRequest(t, "Be brief", acpUser), nil)
	if first.failure != nil || first.text != "one" {
		t.Fatalf("reply = %#v", first)
	}
	started := fixture.received("")[0]
	if fmt.Sprint(started["started"]) != "[acp --model captured]" || started["key"] != "private-token" {
		t.Fatalf("start = %#v", started)
	}
	if authenticated := fixture.received("authenticate"); len(authenticated) != 1 || authenticated[0]["params"].(map[string]any)["methodId"] != "fixture_login" {
		t.Fatalf("authentication = %#v", authenticated)
	}
	if created := fixture.received("session/new"); len(created) != 2 || created[1]["params"].(map[string]any)["_meta"] != nil || len(fixture.received("session/set_model")) != 0 {
		t.Fatalf("sessions = %#v", created)
	}
	if prompt := fixture.prompt(0); !strings.HasPrefix(prompt, "<system>\n"+acpDiscipline) || !strings.HasSuffix(prompt, "Be brief\n</system>\n\nhi") {
		t.Fatalf("prompt = %q", prompt)
	}
	history := `[{"role":"user","parts":[{"kind":"text","text":"hi"}]},{"role":"assistant","parts":[{"kind":"text","text":"one"}]},{"role":"user","parts":[{"kind":"text","text":"more"}]}]`
	if second := fixture.run(t.Context(), acpRequest(t, "Be brief", history), nil); second.failure != nil || second.text != "two" || fixture.prompt(1) != "more" {
		t.Fatalf("reply = %#v, prompt = %q", second, fixture.prompt(1))
	}
	// The private key never reaches a failure message.
	if failure := Classify(&Failure{Kind: Other, Message: "private-token"}, fixture.model.secrets()...); strings.Contains(failure.Message, "private-token") {
		t.Fatal("a failure exposed the key")
	}
}

func TestACPDiscovery(t *testing.T) {
	t.Run("folds effort variants and sends no prompt", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta,variants", nil, nil)
		catalog, err := DiscoverACP(t.Context(), fixture.model.binding, fixture.model.capture)
		if err != nil || len(catalog.Models) != 2 || len(fixture.received("session/prompt")) != 0 {
			t.Fatalf("catalog = %#v, %v", catalog, err)
		}
		folded, plain := catalog.Models[0], catalog.Models[1]
		if folded.ID != "captured" || *folded.DisplayName != "Captured" || !*folded.Capabilities.Reasoning || !*folded.Capabilities.Vision ||
			fmt.Sprint(*folded.Capabilities.Efforts) != "[{low <nil>} {high <nil>}]" || folded.Capabilities.DefaultEffort.Kind != "low" {
			t.Fatalf("folded model = %#v %#v", folded, folded.Capabilities)
		}
		if plain.ID != "plain" || *plain.Capabilities.Reasoning || plain.Capabilities.Efforts != nil || plain.Context != nil {
			t.Fatalf("plain model = %#v", plain)
		}
	})
	t.Run("reads models and levels from session options", func(t *testing.T) {
		fixture := acpFixtureModel(t, "options", []acpFixtureReply{{Chunks: []string{"ok"}}}, func(capture *ACPCapture) { capture.Adapter.Acp.Efforts = nil })
		catalog, err := DiscoverACP(t.Context(), fixture.model.binding, fixture.model.capture)
		if err != nil || len(catalog.Models) != 2 || catalog.Models[0].ID != "captured" || fmt.Sprint(*catalog.Models[0].Capabilities.Efforts) != "[{low <nil>} {high <nil>}]" {
			t.Fatalf("catalog = %#v, %v", catalog, err)
		}
		request := acpRequest(t, "", acpUser)
		request.Reasoning.Effort = protocol.Effort{Kind: "high"}
		if result := fixture.run(t.Context(), request, nil); result.failure != nil {
			t.Fatal(result.failure)
		}
		set := fixture.received("session/set_config_option")
		if len(set) != 2 || set[0]["params"].(map[string]any)["value"] != "captured" || set[1]["params"].(map[string]any)["configId"] != "thought_level" || set[1]["params"].(map[string]any)["value"] != "high" {
			t.Fatalf("selection = %#v", set)
		}
	})
	t.Run("invents no model when the agent lists none", func(t *testing.T) {
		fixture := acpFixtureModel(t, "unlisted", nil, nil)
		catalog, err := DiscoverACP(t.Context(), fixture.model.binding, fixture.model.capture)
		if failure := Classify(err); failure == nil || failure.Kind != Unavailable || failure.Message != "fixture-agent did not list its models" || len(catalog.Models) != 0 {
			t.Fatalf("catalog = %#v, %v", catalog, err)
		}
	})
	t.Run("reports a signed-out agent", func(t *testing.T) {
		fixture := acpFixtureModel(t, "signed-out", nil, nil)
		if _, err := DiscoverACP(t.Context(), fixture.model.binding, fixture.model.capture); Classify(err) == nil || Classify(err).Kind != Authentication {
			t.Fatalf("failure = %v", err)
		}
	})
}
