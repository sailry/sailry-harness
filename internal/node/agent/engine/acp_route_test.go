package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/model/acpfixture"
	"github.com/sailry/sailry-harness/internal/node/program"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The test binary is also the fake agent program (acpfixture).
func TestMain(suite *testing.M) {
	if scenario, fixture := os.LookupEnv(acpfixture.Variable); fixture {
		acpfixture.Serve(scenario)
		return
	}
	os.Exit(suite.Run())
}

type acpEngine struct {
	*engineFixture
	dir  string
	pool *program.Pool
}

// acpEngineFixture runs the engine over the fake agent as a plugin-bound
// model with a 1,000,000-token window and a 64 KiB replay bound.
func acpEngineFixture(t *testing.T, scenario string, script []acpfixture.Reply, tools ...Tool) *acpEngine {
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
	f := newFixture(t, &scriptedModel{}, tools...)
	capabilities := protocol.PluginModelAdapterCapabilities{Vision: true, Tools: true}
	selection := protocol.DefaultConversationPresetsBinding()
	selection.Package = &protocol.PluginReference{Name: "fixture", Digest: strings.Repeat("a", 64)}
	selection.Adapter = &protocol.PluginModelAdapterBinding{Adapter: "fixture", Capabilities: capabilities}
	encoded, _ := json.Marshal(selection)
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(f.ctx, `UPDATE providers SET body=json_set(body,'$.provider.api','plugin','$.provider.endpoint','https://example.test',
 '$.provider.models[0].context',1000000,'$.provider.models[0].output',65536,'$.provider.binding',json(?1))`, string(encoded))
		return err
	}); err != nil {
		t.Fatal(err)
	}
	meta := "vendor.systemPrompt.append"
	declaration := protocol.DefaultPluginModelAdapterDeclaration()
	declaration.ID, declaration.Framing, declaration.Capabilities = "fixture", protocol.PluginModelAdapterFramingNdjson, capabilities
	declaration.Acp = &protocol.PluginModelAdapterAcp{Program: "agent", SystemMeta: &meta, ReplayBytes: 64 * 1024, SignIn: "Sign in", ModelArgument: true}
	pool := program.NewPool(2, time.Minute)
	t.Cleanup(pool.Close)
	capture := model.ACPCapture{Adapter: declaration, Owner: "fixture", Dir: dir, Sessions: filepath.Join(t.TempDir(), "sessions"), Version: "0.0.0", Pool: pool,
		Host:    []string{"PATH=" + bin, "HOME=" + dir},
		Program: protocol.PluginProgramDeclaration{ID: "agent", Command: "fixture-agent", Args: []string{"acp"}, Env: map[string]string{acpfixture.Variable: scenario}},
	}
	f.r.model = func(_ context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
		return model.NewACP(binding, capture)
	}
	return &acpEngine{engineFixture: f, dir: dir, pool: pool}
}

func (f *acpEngine) received(method string) []map[string]any {
	f.t.Helper()
	raw, err := os.ReadFile(filepath.Join(f.dir, "requests.log"))
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		f.t.Fatal(err)
	}
	var lines []map[string]any
	for _, line := range strings.Split(strings.TrimSpace(string(raw)), "\n") {
		var value map[string]any
		if json.Unmarshal([]byte(line), &value) == nil && value["method"] == method {
			lines = append(lines, value)
		}
	}
	return lines
}

type acpPromptSent struct {
	session, text string
	// replay: the prompt opened its session, so it carried the whole history.
	replay bool
}

func (f *acpEngine) prompts() []acpPromptSent {
	f.t.Helper()
	known := map[string]bool{}
	for _, resumed := range append(f.received("session/resume"), f.received("session/load")...) {
		known[resumed["params"].(map[string]any)["sessionId"].(string)] = true
	}
	var sent []acpPromptSent
	for _, line := range f.received("session/prompt") {
		params := line["params"].(map[string]any)
		session := params["sessionId"].(string)
		text := params["prompt"].([]any)[0].(map[string]any)["text"].(string)
		sent = append(sent, acpPromptSent{session: session, text: text, replay: !known[session]})
		known[session] = true
	}
	return sent
}

func acpCall(name, arguments string) string {
	return `<tool_call>{"name":"` + name + `","arguments":` + arguments + `}</tool_call>`
}

func largeRead() Tool {
	return objectTool("read", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
		return ToolResult{Value: strings.Repeat("content line\n", 2500)}
	})
}

func overflowed(records []canonical.Record) int {
	count := 0
	for _, record := range records {
		if message := record.Message(); message != nil && message.Error != nil && message.Error.Code != nil && *message.Error.Code == "context_overflow" {
			count++
		}
	}
	return count
}

func compactions(records []canonical.Record) int {
	count := 0
	for _, record := range records {
		if record.Kind == "compaction" {
			count++
		}
	}
	return count
}

const acpHandoff = `{"user_intent":["Read the files"],"current_work":"Read the requested files"}`

// Two turns of tool steps whose history grows past the replay bound: the pooled
// session receives only new messages, so neither a compaction nor an overflow
// occurs on a 1,000,000-token model.
func TestACPRouteSendsDeltasAcrossStepsAndTurns(t *testing.T) {
	t.Parallel()
	f := acpEngineFixture(t, "meta", []acpfixture.Reply{
		{Chunks: []string{"Reading ", acpCall("read", `{"path":"a"}`)}},
		{Chunks: []string{acpCall("read", `{"path":"b"}`)}},
		{Chunks: []string{acpCall("read", `{"path":"c"}`)}},
		{Chunks: []string{"Done"}},
		{Chunks: []string{acpCall("read", `{"path":"d"}`)}},
		{Chunks: []string{"Done again"}},
	}, largeRead())
	f.start()
	f.waitRun(f.submit("Read three files"), protocol.ConversationStatusCompleted)
	f.waitRun(f.submit("Read one more"), protocol.ConversationStatusCompleted)
	sent := f.prompts()
	if len(sent) != 6 || len(f.received("session/new")) != 1 {
		t.Fatalf("prompts = %d, sessions = %d", len(sent), len(f.received("session/new")))
	}
	total := 0
	for index, prompt := range sent {
		total += len(prompt.text)
		if prompt.replay != (index == 0) || prompt.session != sent[0].session {
			t.Fatalf("prompt %d replay = %v in %s", index, prompt.replay, prompt.session)
		}
	}
	if total <= 64*1024 {
		t.Fatalf("the history (%d bytes) must exceed the replay bound", total)
	}
	records := f.state().Records
	if compactions(records) != 0 || overflowed(records) != 0 {
		t.Fatalf("compactions = %d, overflows = %d", compactions(records), overflowed(records))
	}
}

// A program that stopped between turns loses its session. Without resume the
// next request replays the whole history; past the bound that is the one
// overflow, which compaction recovers. With resume it continues the session.
func TestACPRouteAfterProgramRestart(t *testing.T) {
	t.Parallel()
	for _, scenario := range []string{"meta", "meta,resume", "meta,load", "meta,resume,load,forget"} {
		t.Run(scenario, func(t *testing.T) {
			t.Parallel()
			resume := strings.Contains(scenario, "resume") && !strings.Contains(scenario, "forget") || scenario == "meta,load"
			script := []acpfixture.Reply{
				{Chunks: []string{acpCall("read", `{"path":"a"}`)}},
				{Chunks: []string{acpCall("read", `{"path":"b"}`)}},
				{Chunks: []string{acpCall("read", `{"path":"c"}`)}},
				{Chunks: []string{"Done"}},
			}
			if !resume {
				script = append(script, acpfixture.Reply{Chunks: []string{acpHandoff}})
			}
			script = append(script, acpfixture.Reply{Chunks: []string{"Done again"}})
			f := acpEngineFixture(t, scenario, script, largeRead())
			f.start()
			f.waitRun(f.submit("Read three files"), protocol.ConversationStatusCompleted)
			f.pool.Invalidate("fixture")
			f.waitRun(f.submit("Anything else?"), protocol.ConversationStatusCompleted)
			records, sent := f.state().Records, f.prompts()
			if resume {
				if overflowed(records) != 0 || compactions(records) != 0 || len(sent) != 5 || sent[4].replay || sent[4].session != sent[0].session {
					t.Fatalf("overflows = %d, compactions = %d, prompts = %#v", overflowed(records), compactions(records), sent)
				}
				return
			}
			// The summary request and the compacted history each open a session.
			if overflowed(records) != 1 || compactions(records) != 1 || len(sent) != 6 || !sent[4].replay || !sent[5].replay ||
				len(sent[5].text) > 64*1024 || !strings.Contains(sent[5].text, "Anything else?") {
				t.Fatalf("overflows = %d, compactions = %d, prompts = %d", overflowed(records), compactions(records), len(sent))
			}
		})
	}
}

// The threshold follows the model's settings. A held session that advertises
// /compact records Sailry's handoff in place, compacts itself and keeps
// serving the conversation; one that does not is replaced by a new session
// that replays the handoff. /compact is never sent unadvertised.
func TestACPRouteThresholdCompaction(t *testing.T) {
	t.Parallel()
	for _, advertised := range []bool{true, false} {
		scenario := "meta,resume,close"
		if advertised {
			scenario += ",compact"
		}
		t.Run(scenario, func(t *testing.T) {
			t.Parallel()
			f := acpEngineFixture(t, scenario, []acpfixture.Reply{
				{Chunks: []string{acpCall("read", `{"path":"a"}`)}},
				{Chunks: []string{acpCall("read", `{"path":"b"}`)}},
				{Chunks: []string{acpCall("read", `{"path":"c"}`)}},
				{Chunks: []string{acpHandoff}},
				{Chunks: []string{"Done"}},
			}, largeRead())
			if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
				_, err := db.ExecContext(f.ctx, `UPDATE providers SET body=json_set(body,'$.provider.models[0].compaction_tokens',20000)`)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			f.start()
			f.waitRun(f.submit("Read three files"), protocol.ConversationStatusCompleted)
			records, sent := f.state().Records, f.prompts()
			if compactions(records) != 1 || overflowed(records) != 0 || len(sent) < 5 ||
				!strings.Contains(sent[3].text, "replaces the conversation above") || sent[3].replay {
				t.Fatalf("compactions = %d, prompts = %d", compactions(records), len(sent))
			}
			commands := 0
			for _, prompt := range sent {
				if prompt.text == acpfixture.Compact {
					commands++
				}
			}
			if !advertised {
				if commands != 0 || len(sent) != 5 || !sent[4].replay || len(f.received("session/close")) != 1 {
					t.Fatalf("commands = %d, prompts = %d, last replay = %v", commands, len(sent), sent[len(sent)-1].replay)
				}
				return
			}
			if commands != 1 || len(sent) != 6 || sent[4].text != acpfixture.Compact || sent[5].replay ||
				sent[5].session != sent[0].session || len(f.received("session/new")) != 1 {
				t.Fatalf("commands = %d, prompts = %d, last = %#v", commands, len(sent), sent[len(sent)-1])
			}
		})
	}
}

// A held session summarizes in place: it receives only the summary
// instructions. The compacted history then starts a new session, the replaced
// one is closed, and no native compact command is sent.
func TestACPRouteCompactsAHeldSession(t *testing.T) {
	t.Parallel()
	f := acpEngineFixture(t, "meta,close", []acpfixture.Reply{
		{Chunks: []string{acpCall("read", `{"path":"a"}`)}},
		{Chunks: []string{"Done"}},
		{Chunks: []string{acpHandoff}},
		{Chunks: []string{"Continued"}},
	}, largeRead())
	f.start()
	f.waitRun(f.submit("Read a file"), protocol.ConversationStatusCompleted)
	output := f.command("compact_context", &protocol.CommandCompactContextPayload{Session: f.session.ID, ExpectedRevision: f.session.Revision})
	if turn, ok := output.Data.(protocol.QueuedTurn); ok {
		f.waitRun(turn.ID, protocol.ConversationStatusCompleted)
	} else {
		t.Fatalf("compaction output = %#v", output.Data)
	}
	f.waitRun(f.submit("Continue"), protocol.ConversationStatusCompleted)
	sent := f.prompts()
	if len(sent) != 4 || compactions(f.state().Records) != 1 {
		t.Fatalf("prompts = %d, compactions = %d", len(sent), compactions(f.state().Records))
	}
	if sent[2].replay || sent[2].session != sent[0].session || !strings.Contains(sent[2].text, "replaces the conversation above") {
		t.Fatalf("summary prompt = %#v", sent[2])
	}
	closed := f.received("session/close")
	if !sent[3].replay || len(closed) != 1 || closed[0]["params"].(map[string]any)["sessionId"] != sent[0].session {
		t.Fatalf("after compaction: prompt %#v, closed %#v", sent[3], closed)
	}
	for _, prompt := range sent {
		if strings.TrimSpace(prompt.text) == "/compact" {
			t.Fatal("a native compact command was sent")
		}
	}
}
