package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/feed"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type script func(context.Context, fantasy.Call, func(fantasy.StreamPart) bool)
type scriptedModel struct {
	fantasy.LanguageModel
	mu      sync.Mutex
	calls   []fantasy.Call
	scripts []script
}

func (m *scriptedModel) Stream(ctx context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	m.mu.Lock()
	index := len(m.calls)
	m.calls = append(m.calls, call)
	if index >= len(m.scripts) {
		m.mu.Unlock()
		return nil, errors.New("unexpected extra model request")
	}
	run := m.scripts[index]
	m.mu.Unlock()
	return func(yield func(fantasy.StreamPart) bool) { run(ctx, call, yield) }, nil
}
func (m *scriptedModel) count() int { m.mu.Lock(); defer m.mu.Unlock(); return len(m.calls) }
func response(parts ...fantasy.StreamPart) script {
	return func(_ context.Context, _ fantasy.Call, yield func(fantasy.StreamPart) bool) {
		for _, part := range parts {
			if !yield(part) {
				return
			}
		}
	}
}
func finish() fantasy.StreamPart {
	return fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop}
}
func text(value string) fantasy.StreamPart {
	return fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "text", Delta: value}
}
func call(id, name, args string) fantasy.StreamPart {
	return fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: id, ToolCallName: name, ToolCallInput: args}
}

type manualClock struct {
	mu    sync.Mutex
	now   time.Time
	waits chan clockWait
}
type clockWait struct {
	duration time.Duration
	advance  chan struct{}
}

func (c *manualClock) Now() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.now.IsZero() {
		c.now = time.Date(2026, 11, 10, 0, 0, 0, 0, time.UTC)
	}
	c.now = c.now.Add(time.Millisecond)
	return c.now
}
func (c *manualClock) Sleep(ctx context.Context, d time.Duration) error {
	advance := make(chan struct{})
	select {
	case c.waits <- clockWait{d, advance}:
	case <-ctx.Done():
		return ctx.Err()
	}
	select {
	case <-advance:
		c.mu.Lock()
		c.now = c.now.Add(d)
		c.mu.Unlock()
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

type engineFixture struct {
	t       *testing.T
	r       *Runtime
	db      *store.Store
	node    protocol.NodeID
	session protocol.Session
	changed chan struct{}
	ctx     context.Context
}

func newFixture(t *testing.T, m *scriptedModel, tools ...Tool) *engineFixture {
	t.Helper()
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	t.Cleanup(cancel)
	profile, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	var node protocol.NodeID
	node[0] = 1
	storage, err := store.Open(ctx, profile, node)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := storage.Close(); err != nil {
			t.Error(err)
		}
	})
	f := &engineFixture{t: t, db: storage, node: node, changed: make(chan struct{}, 1), ctx: ctx}
	err = storage.Node.Write(ctx, func(db *sql.Conn) error {
		provider := protocol.DefaultConversationProvider()
		provider.ID = protocol.NewProviderID()
		provider.Revision = 1
		provider.Name = "Fixture"
		provider.Api = protocol.ConversationModelApiChatCompletions
		provider.Authentication = protocol.AuthenticationApiKey
		provider.Endpoint = "http://fixture.invalid/v1"
		provider.Enabled = true
		provider.DefaultModel = "fixture"
		selected := protocol.DefaultConversationModel()
		selected.ID = "fixture"
		selected.Context = 32000
		selected.Output = 8000
		selected.Tools = true
		provider.Models = []protocol.ConversationModel{selected}
		body, err := json.Marshal(map[string]any{"id": provider.ID, "provider": provider, "listed": true, "authorizations": map[string]any{}, "quota": nil})
		if err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "INSERT INTO providers(id,body) VALUES(?1,?2)", provider.ID.String(), body); err != nil {
			return err
		}
		config := protocol.DefaultSessionConfig()
		config.Provider = provider.ID
		config.Model = "fixture"
		config.Mode = protocol.WorkModeCode
		config.Permission = protocol.PermissionAsk
		request := protocol.NewRequest(node, protocol.Command{Kind: "create_session", Data: &protocol.CommandCreateSessionPayload{Config: &config}})
		output, _, err := store.ExecuteCommand(ctx, db, node, profile, request, store.PreparedPaths{})
		if err != nil {
			return err
		}
		f.session = output.Data.(protocol.Session)
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	timer := &manualClock{waits: make(chan clockWait, 8)}
	f.r = New(storage.Node, node, profile, feed.New(node), func() {
		select {
		case f.changed <- struct{}{}:
		default:
		}
	}, Options{Clock: timer, Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return m, nil }, Tools: tools})
	return f
}
func (f *engineFixture) start() { f.t.Helper(); f.r.Start(); f.t.Cleanup(f.r.Stop) }
func (f *engineFixture) command(kind string, data any) protocol.Output {
	f.t.Helper()
	request := protocol.NewRequest(f.node, protocol.Command{Kind: kind, Data: data})
	raw, err := json.Marshal(request)
	if err != nil {
		f.t.Fatal(err)
	}
	unlock := f.r.LockDecision(request.Command)
	defer unlock()
	publication := f.r.LockPublication()
	var output protocol.Output
	err = f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		if _, err := admission.Admit(f.ctx, db, f.node, request, raw); err != nil {
			return err
		}
		var err error
		output, _, err = store.ExecuteAgentCommand(f.ctx, db, f.node, f.r.profile, f.node, request, f.r.clock.Now())
		return err
	})
	publication()
	if err != nil {
		f.t.Fatal(err)
	}
	f.r.Committed()
	return output
}
func (f *engineFixture) submit(value string) protocol.TurnID {
	f.t.Helper()
	input := protocol.DefaultConversationInput()
	input.Text = value
	output := f.command("submit_turn", &protocol.CommandSubmitTurnPayload{Session: f.session.ID, ExpectedRevision: f.session.Revision, Message: input})
	return output.Data.(protocol.QueuedTurn).ID
}
func (f *engineFixture) waitRun(turn protocol.TurnID, status protocol.ConversationStatus) protocol.ConversationRun {
	f.t.Helper()
	for {
		var run protocol.ConversationRun
		err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error { var err error; run, err = store.ReadAgentRun(f.ctx, db, turn); return err })
		if err != nil {
			f.t.Fatal(err)
		}
		if run.Status == status {
			return run
		}
		if run.Status == protocol.ConversationStatusFailed {
			f.t.Fatalf("run failed: %+v", run.Error)
		}
		select {
		case <-f.changed:
		case <-f.ctx.Done():
			f.t.Fatalf("waiting for %s: %+v", status, run)
		}
	}
}
func (f *engineFixture) state() store.AgentState {
	f.t.Helper()
	state, err := f.r.load(f.ctx, f.session.ID)
	if err != nil {
		f.t.Fatal(err)
	}
	return state
}
func (f *engineFixture) readQuery(query string) uint64 {
	f.t.Helper()
	var value uint64
	if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error { return db.QueryRowContext(f.ctx, query).Scan(&value) }); err != nil {
		f.t.Fatal(err)
	}
	return value
}
func objectTool(name string, readOnly, concurrent bool, run func(context.Context, canonical.ToolRequest) ToolResult) Tool {
	return jsonTool(name, "Fixture tool", `{"type":"object"}`, readOnly, concurrent, run)
}
func mustHaveText(t *testing.T, records []canonical.Record, want string) {
	t.Helper()
	for _, record := range records {
		if m := record.Message(); m != nil {
			for _, block := range m.Content {
				if value, ok := block.Data.(*canonical.TextContent); ok && value.Text == want {
					return
				}
			}
		}
	}
	t.Fatal(fmt.Sprintf("missing text %q", want))
}
