package model

import (
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
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/compact"
	"github.com/sailry/sailry-harness/internal/node/agent/model/acpfixture"
	"github.com/sailry/sailry-harness/internal/node/program"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The test binary is its own agent: started with the fixture variable it
// speaks the Agent Client Protocol (acpfixture).
func TestMain(suite *testing.M) {
	if scenario, fixture := os.LookupEnv(acpfixture.Variable); fixture {
		acpfixture.Serve(scenario)
		return
	}
	os.Exit(suite.Run())
}

type acpFixtureReply = acpfixture.Reply

func mustJSON(t *testing.T, value any) json.RawMessage {
	t.Helper()
	raw, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	return raw
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
	capture := ACPCapture{Adapter: declaration, Owner: "fixture", Dir: dir, Sessions: filepath.Join(t.TempDir(), "sessions"), Version: "0.0.0", Pool: pool,
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
	context         *int64
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
			if metadata, ok := part.ProviderMetadata[UsageMetadataKey].(*UsageMetadata); ok {
				result.context = metadata.Context
			}
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
	t.Parallel()
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

	t.Run("summarizes history beyond the replay budget", func(t *testing.T) {
		const replayBytes = 112 * 1024
		const contextLimit, outputLimit = 128000, 8000
		const latest = "Keep the current deployment constraint"
		fixture := acpFixtureModel(t, "variants", []acpFixtureReply{{Chunks: []string{`{"user_intent":["Keep the deployment constraint"]}`}}}, func(capture *ACPCapture) {
			capture.Adapter.Acp.ReplayBytes = replayBytes
		})
		record := func(turn protocol.TurnID, role canonical.Role, text string) canonical.Record {
			return canonical.NewRecord(turn, "message", canonical.NewMessage(role, canonical.Text(0, text)), time.Time{})
		}
		earlier := protocol.NewTurnID()
		history := []canonical.Record{
			record(earlier, canonical.User, "Inspect the deployment"),
			record(earlier, canonical.Assistant, strings.Repeat("Earlier detail. 此前记录。\n", 6000)),
			record(protocol.NewTurnID(), canonical.User, latest),
		}
		message, err := json.Marshal(compact.Request)
		if err != nil {
			t.Fatal(err)
		}
		user := `[{"role":"user","parts":[{"kind":"text","text":` + string(message) + `}]}]`
		tokenOnly, err := compact.Prepare(history, contextLimit, outputLimit, compact.Request, 0)
		if err != nil || len(tokenOnly) <= replayBytes {
			t.Fatalf("fixture must exceed %d bytes under a token-only budget, got %d: %v", replayBytes, len(tokenOnly), err)
		}
		if refused := fixture.run(t.Context(), acpRequest(t, tokenOnly, user), nil); refused.failure == nil || refused.failure.Kind != ContextOverflow {
			t.Fatalf("oversized summary request was not rejected: %v", refused.failure)
		}
		system, err := compact.Prepare(history, contextLimit, outputLimit, compact.Request, SummarySystemBytes(fixture.model, compact.Request))
		if err != nil {
			t.Fatal(err)
		}
		result := fixture.run(t.Context(), acpRequest(t, system, user), nil)
		if result.failure != nil || !strings.Contains(result.text, "Keep the deployment constraint") {
			t.Fatalf("budgeted request did not return a summary: %#v", result)
		}
		if sent := fixture.prompt(0); len(sent) > replayBytes || !strings.Contains(sent, latest) {
			t.Fatalf("summary request was %d bytes or lost the latest user instruction", len(sent))
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

	t.Run("emits a complete tool before the prompt ends", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta", []acpFixtureReply{{
			Chunks: []string{`<tool_call>{"name":"read","arguments":{"path":`, `"sample.go"}}</tool_call>`}, Hang: true,
		}}, nil)
		ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
		defer cancel()
		result := fixture.run(ctx, acpRequest(t, "", acpUser, "read"), func(part fantasy.StreamPart) {
			if part.Type == fantasy.StreamPartTypeToolCall {
				cancel()
			}
		})
		if len(result.calls) != 1 || result.calls[0].ToolCallInput != `{"path":"sample.go"}` || result.failure == nil || result.failure.Kind != Cancelled {
			t.Fatalf("complete call not received during the held prompt: %#v", result)
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

	t.Run("repairs raw control characters and trailing commas in a call", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta", []acpFixtureReply{
			{Chunks: []string{"<tool_call>{\"name\":\"read\",\"arguments\":{\"path\":\"a\n\tb\",},}</tool_call>"}},
		}, nil)
		result := fixture.run(t.Context(), acpRequest(t, "", acpUser, "read"), nil)
		if result.failure != nil || len(result.calls) != 1 || result.calls[0].ToolCallInput != `{"path":"a\n\tb"}` || result.finish != fantasy.FinishReasonToolCalls {
			t.Fatalf("reply = %#v", result)
		}
		if prompts := fixture.received("session/prompt"); len(prompts) != 1 {
			t.Fatalf("a repaired call was asked for again: %d prompts", len(prompts))
		}
	})

	t.Run("asks again in the same session for a call it cannot read", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta", []acpFixtureReply{
			{Chunks: []string{`<tool_call>{"name":"read","arguments":{}}</tool_call><tool_call>{"name":</tool_call><tool_call>{"name":"read","arguments":{"path":"x"}}</tool_call>`}},
			{Chunks: []string{`Sorry. <tool_call>{"name":"read","arguments":{"path":"y"}}</tool_call>`}},
		}, nil)
		result := fixture.run(t.Context(), acpRequest(t, "", acpUser, "read"), nil)
		if result.failure != nil || len(result.calls) != 2 || result.calls[0].ToolCallInput != `{}` || result.calls[1].ToolCallInput != `{"path":"y"}` ||
			result.text != "" || result.finish != fantasy.FinishReasonToolCalls {
			t.Fatalf("reply = %#v", result)
		}
		prompts := fixture.received("session/prompt")
		if len(prompts) != 2 || !strings.Contains(fixture.prompt(1), "Tool call 2 ") || len(fixture.received("session/new")) != 1 {
			t.Fatalf("prompts = %d, correction = %q", len(prompts), fixture.prompt(1))
		}
		if prompts[0]["params"].(map[string]any)["sessionId"] != prompts[1]["params"].(map[string]any)["sessionId"] {
			t.Fatal("the correction went to another session")
		}
	})

	t.Run("counts the usage of every prompt of a corrected reply", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta", []acpFixtureReply{
			{Chunks: []string{`<tool_call>{"name":</tool_call><tool_call>{}</tool_call>`}, Usage: map[string]any{"inputTokens": 10, "outputTokens": 4}},
			{Chunks: []string{`<tool_call>{"name":"read","arguments":{}}</tool_call>`}, Usage: map[string]any{"inputTokens": 12, "outputTokens": 3}},
		}, nil)
		result := fixture.run(t.Context(), acpRequest(t, "", acpUser, "read"), nil)
		if result.failure != nil || len(result.calls) != 1 || result.usage.InputTokens != 22 || result.usage.OutputTokens != 7 {
			t.Fatalf("reply = %#v", result)
		}
		// The window is the last prompt's, not the sum.
		if result.context == nil || *result.context != 15 {
			t.Fatalf("context = %v, want 15", result.context)
		}
	})

	t.Run("names the JSON error of a last call that closed", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta", []acpFixtureReply{
			{Chunks: []string{`<tool_call>{"name":</tool_call>`}},
			{Chunks: []string{`<tool_call>{"name":"read","arguments":{}}</tool_call>`}},
		}, nil)
		if result := fixture.run(t.Context(), acpRequest(t, "", acpUser, "read"), nil); result.failure != nil || len(result.calls) != 1 {
			t.Fatalf("reply = %#v", result)
		}
		if correction := fixture.prompt(1); strings.Contains(correction, "ended inside it") || !strings.Contains(correction, "Tool call 1 ") {
			t.Fatalf("correction = %q", correction)
		}
	})

	t.Run("ends the agent session of a request without a session", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta,close", nil, nil)
		binding, err := Capture(fixture.model.binding.Provider, fixture.model.binding.Config, fixture.model.binding.Secret, protocol.SessionID{})
		if err != nil {
			t.Fatal(err)
		}
		language, err := NewACP(binding, fixture.model.capture)
		if err != nil {
			t.Fatal(err)
		}
		sessionless := &acpFixture{t: t, model: language.(*acpModel), dir: fixture.dir}
		for range 2 {
			if result := sessionless.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil || result.text != "ok" {
				t.Fatalf("reply = %#v", result)
			}
		}
		if opened, closed := len(fixture.received("session/new")), len(fixture.received("session/close")); opened != 2 || closed != 2 {
			t.Fatalf("sessions opened = %d, closed = %d", opened, closed)
		}
		entry, agent, err := sessionless.model.lease(t.Context(), false)
		if err != nil {
			t.Fatal(err)
		}
		agent.mu.Lock()
		slots, commands := len(agent.slots), len(agent.commands)
		agent.mu.Unlock()
		sessionless.model.capture.Pool.Release(entry, true)
		if slots != 0 || commands != 0 {
			t.Fatalf("a sessionless request left slots = %d, commands = %d", slots, commands)
		}
	})

	t.Run("reports a call still malformed after its corrections", func(t *testing.T) {
		cut := acpFixtureReply{Chunks: []string{`<tool_call>{"name":"read","argu`}}
		fixture := acpFixtureModel(t, "meta", []acpFixtureReply{cut, cut, cut}, nil)
		result := fixture.run(t.Context(), acpRequest(t, "", acpUser, "read"), nil)
		if result.failure == nil || result.failure.Kind != InvalidResponse || len(result.calls) != 0 {
			t.Fatalf("reply = %#v", result)
		}
		if prompts := fixture.received("session/prompt"); len(prompts) != 1+acpResends {
			t.Fatalf("prompts = %d", len(prompts))
		}
	})

	for _, ending := range []acpFixtureReply{
		{Chunks: []string{`Sorry, <tool_call>{"name":"read","argu`}, Stop: "max_tokens"},
		{Stop: "refusal"},
	} {
		t.Run("reports a correction that ends by "+ending.Stop+" without its call", func(t *testing.T) {
			fixture := acpFixtureModel(t, "meta", []acpFixtureReply{
				{Chunks: []string{`<tool_call>{"name":"read","arguments":{}}</tool_call><tool_call>{"name":</tool_call>`}}, ending,
			}, nil)
			result := fixture.run(t.Context(), acpRequest(t, "", acpUser, "read"), nil)
			if result.failure == nil || result.failure.Kind != InvalidResponse || len(fixture.received("session/prompt")) != 2 {
				t.Fatalf("reply = %#v", result)
			}
		})
	}

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

func TestACPSessionContinuation(t *testing.T) {
	t.Parallel()
	const second = `[{"role":"user","parts":[{"kind":"text","text":"hi"}]},{"role":"assistant","parts":[{"kind":"text","text":"one"}]},{"role":"user","parts":[{"kind":"text","text":"more"}]}]`
	restart := func(fixture *acpFixture) { fixture.model.capture.Pool.Invalidate(fixture.model.capture.Owner) }

	t.Run("passes a reply quoting tool tags through when no tools are offered", func(t *testing.T) {
		summary := `{"user_intent":["Explain the <tool_call> convention"],"notes":"<tool_call>{\"name\":"}`
		fixture := acpFixtureModel(t, "meta", []acpFixtureReply{{Chunks: []string{summary[:30], summary[30:]}}, {Chunks: []string{summary}}}, nil)
		result := fixture.run(t.Context(), acpRequest(t, "Summarize", acpUser), nil)
		if result.failure != nil || result.text != summary || result.finish != fantasy.FinishReasonStop || len(result.calls) != 0 {
			t.Fatalf("summary = %#v", result)
		}
		// With tools offered, the same reply ends inside a call.
		if offered := fixture.run(t.Context(), acpRequest(t, "Summarize", acpUser, "read"), nil); offered.failure == nil || offered.failure.Kind != InvalidResponse {
			t.Fatalf("reply with tools = %#v", offered)
		}
	})

	t.Run("reports residency and the prompt bound", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta", []acpFixtureReply{{Chunks: []string{"one"}}}, nil)
		session := fixture.model.binding.Session
		if route := RouteOf(fixture.model, session); !route.Session || route.Resident || route.PromptBytes != 64*1024 {
			t.Fatalf("route before a request = %#v", route)
		}
		if result := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil {
			t.Fatal(result.failure)
		}
		if route := RouteOf(fixture.model, session); !route.Resident || route.ReplayWindow(1_000_000) != 16*1024 {
			t.Fatalf("route of the held session = %#v", route)
		}
		if route := RouteOf(fixture.model, protocol.NewSessionID()); route.Resident || route.ReplayWindow(1_000_000) != 16*1024 {
			t.Fatalf("route of another session = %#v", route)
		}
		// Without a resumable session a restarted program must replay.
		restart(fixture)
		if route := RouteOf(fixture.model, session); route.Resident {
			t.Fatalf("route after the program stopped = %#v", route)
		}
		if route := RouteOf(&pluginModel{}, session); route != (Route{}) || route.ReplayWindow(1_000_000) != 1_000_000 {
			t.Fatalf("translated route = %#v", route)
		}
	})

	for _, method := range []string{"resume", "load"} {
		t.Run("continues a restarted program by session/"+method, func(t *testing.T) {
			fixture := acpFixtureModel(t, "meta,"+method, []acpFixtureReply{{Chunks: []string{"one"}}, {Chunks: []string{"two"}}}, nil)
			if result := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil {
				t.Fatal(result.failure)
			}
			first := fixture.received("session/prompt")[0]["params"].(map[string]any)["sessionId"]
			restart(fixture)
			if route := RouteOf(fixture.model, fixture.model.binding.Session); !route.Resident {
				t.Fatalf("a resumable session is not resident: %#v", route)
			}
			result := fixture.run(t.Context(), acpRequest(t, "", second), nil)
			if result.failure != nil || result.text != "two" {
				t.Fatalf("reply = %#v", result)
			}
			resumed := fixture.received("session/" + method)
			if len(fixture.received("initialize")) != 2 || len(fixture.received("session/new")) != 1 || len(resumed) != 1 {
				t.Fatalf("the restarted program did not %s the session", method)
			}
			if id := resumed[0]["params"].(map[string]any)["sessionId"]; id != first || fixture.prompt(1) != "more" {
				t.Fatalf("resumed %v instead of %v, prompt = %q", id, first, fixture.prompt(1))
			}
			if selected := fixture.received("session/set_model"); len(selected) != 2 {
				t.Fatalf("the model was not selected in the resumed session: %d", len(selected))
			}
		})
	}

	t.Run("tries load after resume and replays when both fail", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta,resume,load,forget", []acpFixtureReply{{Chunks: []string{"one"}}, {Chunks: []string{"two"}}}, nil)
		if result := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil {
			t.Fatal(result.failure)
		}
		restart(fixture)
		if result := fixture.run(t.Context(), acpRequest(t, "", second), nil); result.failure != nil || result.text != "two" {
			t.Fatalf("reply = %#v", result)
		}
		if len(fixture.received("session/resume")) != 1 || len(fixture.received("session/load")) != 1 || len(fixture.received("session/new")) != 2 ||
			fixture.prompt(1) != "[USER]\nhi\n\n[ASSISTANT]\none\n\nmore" {
			t.Fatalf("prompt = %q", fixture.prompt(1))
		}
	})

	t.Run("resumes the session after a Node restart", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta,resume", []acpFixtureReply{{Chunks: []string{"one"}}, {Chunks: []string{"two"}}}, nil)
		if result := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil {
			t.Fatal(result.failure)
		}
		// A new Node has a new pool and a new model; only the record remains.
		capture := fixture.model.capture
		capture.Pool = program.NewPool(2, time.Minute)
		t.Cleanup(capture.Pool.Close)
		fixture.model.capture.Pool.Close()
		restarted, err := NewACP(fixture.model.binding, capture)
		if err != nil {
			t.Fatal(err)
		}
		fixture.model = restarted.(*acpModel)
		if result := fixture.run(t.Context(), acpRequest(t, "", second), nil); result.failure != nil || result.text != "two" ||
			len(fixture.received("session/resume")) != 1 || len(fixture.received("session/new")) != 1 || fixture.prompt(1) != "more" {
			t.Fatalf("reply = %#v, prompt = %q", result, fixture.prompt(1))
		}
	})

	for _, advertised := range []bool{true, false} {
		name := "sends /compact only where the session advertises it"
		scenario := "meta,resume,close"
		if advertised {
			name, scenario = "continues the session after /compact", scenario+",compact"
		}
		t.Run(name, func(t *testing.T) {
			fixture := acpFixtureModel(t, scenario, []acpFixtureReply{{Chunks: []string{"one"}}, {Chunks: []string{"two"}}}, nil)
			if result := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil {
				t.Fatal(result.failure)
			}
			if route := RouteOf(fixture.model, fixture.model.binding.Session); route.Compact != advertised {
				t.Fatalf("route = %#v", route)
			}
			// The compacted history: the handoff, then the retained exchange.
			const handoff = `{"role":"user","parts":[{"kind":"text","text":"Summary of the earlier work"}]},`
			compacted := "[" + handoff + `{"role":"user","parts":[{"kind":"text","text":"hi"}]},{"role":"assistant","parts":[{"kind":"text","text":"one"}]}]`
			call := fantasy.Call{ProviderOptions: fantasy.ProviderOptions{pluginRequestKey: &pluginCallRequest{Body: mustJSON(t, acpRequest(t, "", compacted))}}}
			if err := CompactSession(t.Context(), fixture.model, call); err != nil {
				t.Fatal(err)
			}
			next := "[" + handoff + strings.TrimPrefix(second, "[")
			if result := fixture.run(t.Context(), acpRequest(t, "", next), nil); result.failure != nil || result.text != "two" {
				t.Fatalf("reply = %#v", result)
			}
			sent := fixture.received("session/prompt")
			if !advertised {
				if len(sent) != 2 || len(fixture.received("session/new")) != 2 || strings.Contains(fixture.prompt(1), "/compact") {
					t.Fatalf("prompts = %d, sessions = %d", len(sent), len(fixture.received("session/new")))
				}
				return
			}
			session := sent[0]["params"].(map[string]any)["sessionId"]
			if len(sent) != 3 || fixture.prompt(1) != "/compact" || fixture.prompt(2) != "more" || len(fixture.received("session/new")) != 1 ||
				sent[2]["params"].(map[string]any)["sessionId"] != session {
				t.Fatalf("prompts = %d, second = %q", len(sent), fixture.prompt(len(sent)-1))
			}
		})
	}

	t.Run("counts only the retained messages as seen after /compact", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta,resume,compact", []acpFixtureReply{{Chunks: []string{`<tool_call>{"name":"read","arguments":{}}</tool_call>`}}, {Chunks: []string{"done"}}}, nil)
		first := fixture.run(t.Context(), acpRequest(t, "", acpUser, "read"), nil)
		if first.failure != nil || len(first.calls) != 1 {
			t.Fatalf("reply = %#v", first)
		}
		id := first.calls[0].ID
		// The handoff, the retained call, its result the session has not
		// received and the carried user input, which reads as the first did.
		compacted := fmt.Sprintf(`[{"role":"user","parts":[{"kind":"text","text":"Summary of the earlier work"}]},
			{"role":"assistant","parts":[{"kind":"tool_call","id":%q,"name":"read","arguments":{}}]},
			{"role":"tool","call_id":%q,"name":"read","is_error":false,"parts":[{"kind":"json","value":"content"}]},
			{"role":"user","parts":[{"kind":"text","text":"hi"}]}]`, id, id)
		request := acpRequest(t, "", compacted, "read")
		call := fantasy.Call{ProviderOptions: fantasy.ProviderOptions{pluginRequestKey: &pluginCallRequest{Body: mustJSON(t, request)}}}
		if err := CompactSession(t.Context(), fixture.model, call); err != nil {
			t.Fatal(err)
		}
		if result := fixture.run(t.Context(), request, nil); result.failure != nil || result.text != "done" {
			t.Fatalf("reply = %#v", result)
		}
		want := fmt.Sprintf("<tool_response id=%q name=\"read\">\ncontent\n</tool_response>\n\nhi", id)
		if len(fixture.received("session/new")) != 1 || fixture.prompt(1) != acpfixture.Compact || fixture.prompt(2) != want {
			t.Fatalf("sessions = %d, prompt = %q", len(fixture.received("session/new")), fixture.prompt(2))
		}
	})

	compacted := func(t *testing.T) fantasy.Call {
		history := `[{"role":"user","parts":[{"kind":"text","text":"Summary of the earlier work"}]},{"role":"user","parts":[{"kind":"text","text":"hi"}]},{"role":"assistant","parts":[{"kind":"text","text":"one"}]}]`
		return fantasy.Call{ProviderOptions: fantasy.ProviderOptions{pluginRequestKey: &pluginCallRequest{Body: mustJSON(t, acpRequest(t, "", history))}}}
	}

	t.Run("cancels only the /compact prompt", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta,resume,compact,compact-hang", []acpFixtureReply{{Chunks: []string{"one"}}, {Chunks: []string{"two"}}}, nil)
		if result := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil {
			t.Fatal(result.failure)
		}
		ctx, cancel := context.WithCancel(t.Context())
		go func() {
			defer cancel()
			for range 500 {
				if raw, _ := os.ReadFile(filepath.Join(fixture.dir, "requests.log")); strings.Contains(string(raw), `"text":"`+acpfixture.Compact+`"`) {
					return
				}
				time.Sleep(10 * time.Millisecond)
			}
		}()
		if err := CompactSession(ctx, fixture.model, compacted(t)); !errors.Is(err, context.Canceled) || len(fixture.received("session/cancel")) != 1 {
			t.Fatalf("error = %v, cancellations = %d", err, len(fixture.received("session/cancel")))
		}
		// The program serves on; the session's content is unknown, so it replays.
		if result := fixture.run(t.Context(), acpRequest(t, "", second), nil); result.failure != nil || result.text != "two" || len(fixture.received("initialize")) != 1 {
			t.Fatalf("reply = %#v, programs = %d", result, len(fixture.received("initialize")))
		}
	})

	t.Run("cancels a /compact that goes silent and keeps its program", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta,resume,compact,compact-hang", []acpFixtureReply{{Chunks: []string{"one"}}, {Chunks: []string{"two"}}}, func(capture *ACPCapture) {
			capture.stall = 300 * time.Millisecond
		})
		if result := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil {
			t.Fatal(result.failure)
		}
		ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
		defer cancel()
		if failure := Classify(CompactSession(ctx, fixture.model, compacted(t))); failure == nil || failure.Kind != Network || ctx.Err() != nil {
			t.Fatalf("failure = %#v, waited until %v", failure, ctx.Err())
		}
		// The agent answered the cancel, so its program serves the next request.
		if result := fixture.run(t.Context(), acpRequest(t, "", second), nil); result.failure != nil || result.text != "two" || len(fixture.received("initialize")) != 1 || len(fixture.received("session/cancel")) != 1 {
			t.Fatalf("reply = %#v, programs = %d, cancels = %d", result, len(fixture.received("initialize")), len(fixture.received("session/cancel")))
		}
	})

	t.Run("replays a history another model continued", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta", []acpFixtureReply{{Chunks: []string{"one"}}, {Chunks: []string{"three"}}}, nil)
		if result := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil {
			t.Fatal(result.failure)
		}
		switched := strings.TrimSuffix(second, "]") + `,{"role":"assistant","parts":[{"kind":"text","text":"from another model"}]},{"role":"user","parts":[{"kind":"text","text":"again"}]}]`
		if result := fixture.run(t.Context(), acpRequest(t, "", switched), nil); result.failure != nil || result.text != "three" {
			t.Fatalf("reply = %#v", result)
		}
		if want := "[USER]\nhi\n\n[ASSISTANT]\none\n\n[USER]\nmore\n\n[ASSISTANT]\nfrom another model\n\nagain"; len(fixture.received("session/new")) != 2 || fixture.prompt(1) != want {
			t.Fatalf("sessions = %d, prompt = %q", len(fixture.received("session/new")), fixture.prompt(1))
		}
	})

	// elsewhere is the same route served by another program, as when the
	// program holding the session was busy.
	elsewhere := func(t *testing.T, fixture *acpFixture) *acpFixture {
		other := *fixture.model
		other.capture.Pool = program.NewPool(2, time.Minute)
		t.Cleanup(other.capture.Pool.Close)
		return &acpFixture{t: t, model: &other, dir: fixture.dir}
	}
	sessionOf := func(line map[string]any) any { return line["params"].(map[string]any)["sessionId"] }

	t.Run("resumes a session another program continued", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta,resume,close", []acpFixtureReply{{Chunks: []string{"one"}}, {Chunks: []string{"two"}}, {Chunks: []string{"three"}}}, nil)
		if result := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil {
			t.Fatal(result.failure)
		}
		if result := elsewhere(t, fixture).run(t.Context(), acpRequest(t, "", second), nil); result.failure != nil || result.text != "two" {
			t.Fatalf("reply elsewhere = %#v", result)
		}
		third := strings.TrimSuffix(second, "]") + `,{"role":"assistant","parts":[{"kind":"text","text":"two"}]},{"role":"user","parts":[{"kind":"text","text":"again"}]}]`
		// Each program counts the scripted replies from its own start.
		if result := fixture.run(t.Context(), acpRequest(t, "", third), nil); result.failure != nil {
			t.Fatalf("reply = %#v", result)
		}
		// This program closes its outdated copy, then resumes the recorded one.
		prompts, closed := fixture.received("session/prompt"), fixture.received("session/close")
		if len(fixture.received("session/new")) != 1 || len(closed) != 1 || sessionOf(closed[0]) != sessionOf(prompts[0]) || fixture.prompt(2) != "again" || sessionOf(prompts[2]) != sessionOf(prompts[0]) {
			t.Fatalf("sessions = %d, closed = %d, prompt = %q", len(fixture.received("session/new")), len(closed), fixture.prompt(2))
		}
	})

	t.Run("closes its session when it resumes another program's", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta,resume,close", []acpFixtureReply{{Chunks: []string{"one"}}, {Chunks: []string{"two"}}, {Chunks: []string{"three"}}}, nil)
		if result := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil {
			t.Fatal(result.failure)
		}
		// An edited history opens a new session in the other program.
		edited := `[{"role":"user","parts":[{"kind":"text","text":"hello"}]},{"role":"assistant","parts":[{"kind":"text","text":"one"}]},{"role":"user","parts":[{"kind":"text","text":"again"}]}]`
		if result := elsewhere(t, fixture).run(t.Context(), acpRequest(t, "", edited), nil); result.failure != nil || result.text != "two" {
			t.Fatalf("reply elsewhere = %#v", result)
		}
		next := strings.TrimSuffix(edited, "]") + `,{"role":"assistant","parts":[{"kind":"text","text":"two"}]},{"role":"user","parts":[{"kind":"text","text":"more"}]}]`
		if result := fixture.run(t.Context(), acpRequest(t, "", next), nil); result.failure != nil {
			t.Fatalf("reply = %#v", result)
		}
		prompts, closed := fixture.received("session/prompt"), fixture.received("session/close")
		if len(closed) != 1 || sessionOf(closed[0]) != sessionOf(prompts[0]) || sessionOf(prompts[2]) != sessionOf(prompts[1]) || fixture.prompt(2) != "more" {
			t.Fatalf("closed = %#v, prompt = %q", closed, fixture.prompt(2))
		}
	})

	t.Run("closes a replaced session and never sends a compact command", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta,close", []acpFixtureReply{{Chunks: []string{"one"}}, {Chunks: []string{"two"}}}, nil)
		if result := fixture.run(t.Context(), acpRequest(t, "", acpUser), nil); result.failure != nil {
			t.Fatal(result.failure)
		}
		// A compacted history begins with its summary: no prefix is shared.
		compacted := `[{"role":"user","parts":[{"kind":"text","text":"Summary of the earlier work"}]},{"role":"user","parts":[{"kind":"text","text":"more"}]}]`
		if result := fixture.run(t.Context(), acpRequest(t, "", compacted), nil); result.failure != nil || result.text != "two" {
			t.Fatalf("reply = %#v", result)
		}
		sessions, closed := fixture.received("session/new"), fixture.received("session/close")
		if len(sessions) != 2 || len(closed) != 1 || closed[0]["params"].(map[string]any)["sessionId"] != fixture.received("session/prompt")[0]["params"].(map[string]any)["sessionId"] {
			t.Fatalf("sessions = %d, closed = %#v", len(sessions), closed)
		}
		for index := range fixture.received("session/prompt") {
			if strings.TrimSpace(fixture.prompt(index)) == "/compact" {
				t.Fatal("a compact command was sent")
			}
		}
	})
}

// A second vendor shape: no session metadata for the system text, the model
// chosen by a start argument, a key in the declared environment and an
// authentication method the agent is asked to use.
func TestACPRouteSecondVendor(t *testing.T) {
	t.Parallel()
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
	t.Parallel()
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
	t.Run("ends the session it opened", func(t *testing.T) {
		fixture := acpFixtureModel(t, "meta,variants,close,compact", nil, nil)
		if _, err := DiscoverACP(t.Context(), fixture.model.binding, fixture.model.capture); err != nil {
			t.Fatal(err)
		}
		if opened, closed := len(fixture.received("session/new")), len(fixture.received("session/close")); opened != 1 || closed != 1 {
			t.Fatalf("sessions opened = %d, closed = %d", opened, closed)
		}
		entry, agent, err := fixture.model.lease(t.Context(), true)
		if err != nil {
			t.Fatal(err)
		}
		agent.mu.Lock()
		commands := len(agent.commands)
		agent.mu.Unlock()
		fixture.model.capture.Pool.Release(entry, true)
		if len(fixture.received("initialize")) != 1 || commands != 0 {
			t.Fatalf("programs = %d, commands kept = %d", len(fixture.received("initialize")), commands)
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
