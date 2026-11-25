package node

import (
	"context"
	"database/sql"
	"encoding/binary"
	"errors"
	"log/slog"
	"maps"
	"net/http"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"testing/synctest"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestNodeTurnDiagnostics(t *testing.T) {
	t.Setenv("SAILRY_TELEMETRY", "off")
	type diagnostic struct {
		level  slog.Level
		fields logging.Fields
	}
	for _, test := range []struct {
		name        string
		responses   []int
		status      protocol.ConversationStatus
		faultKind   string
		hookFailure bool
		stop        bool
		want        []diagnostic
	}{
		{
			name: "502 attempts exhausted", responses: slices.Repeat([]int{http.StatusBadGateway}, model.MainRetries+1),
			status: protocol.ConversationStatusFailed, faultKind: "model_recovery",
			want: []diagnostic{{slog.LevelError, logging.Fields{"area": "agent", "operation": "model_request", "phase": logging.Failed, "code": "unavailable",
				"retry_outcome": "failed", "reason_code": "model_unavailable", "attempts": uint64(model.MainRetries + 1), "http_status": uint64(http.StatusBadGateway)}}},
		},
		{
			name: "first attempt permanent failure", responses: []int{http.StatusUnauthorized},
			status: protocol.ConversationStatusFailed, faultKind: "model_request",
			want: []diagnostic{{slog.LevelError, logging.Fields{"area": "agent", "operation": "model_request", "phase": logging.Failed, "code": "unavailable",
				"retry_outcome": "failed", "reason_code": "model_authentication", "attempts": uint64(1), "http_status": uint64(http.StatusUnauthorized)}}},
		},
		{
			name: "retry recovers", responses: []int{http.StatusBadGateway, http.StatusOK}, status: protocol.ConversationStatusCompleted,
			want: []diagnostic{{slog.LevelWarn, logging.Fields{"area": "agent", "operation": "model_request", "phase": logging.Completed, "code": "unavailable",
				"retry_outcome": "recovered", "reason_code": "model_unavailable", "attempts": uint64(2), "http_status": uint64(http.StatusBadGateway)}}},
		},
		{
			name: "empty output then success", responses: []int{turnDiagnosticEmpty, http.StatusOK}, status: protocol.ConversationStatusCompleted,
			want: []diagnostic{{slog.LevelWarn, logging.Fields{"area": "agent", "operation": "model_request", "phase": logging.Completed, "code": "unavailable",
				"retry_outcome": "recovered", "reason_code": "model_invalid_response", "attempts": uint64(2)}}},
		},
		{
			name: "four empty outputs", responses: slices.Repeat([]int{turnDiagnosticEmpty}, 4), status: protocol.ConversationStatusCompleted,
			want: []diagnostic{{slog.LevelError, logging.Fields{"area": "agent", "operation": "model_request", "phase": logging.Failed, "code": "unavailable",
				"retry_outcome": "failed", "reason_code": "model_invalid_response", "attempts": uint64(4)}}},
		},
		{
			name: "user stops retry", responses: []int{http.StatusBadGateway, 0}, status: protocol.ConversationStatusCancelled, stop: true,
		},
		{
			name: "turn end hook fails", responses: []int{http.StatusOK}, status: protocol.ConversationStatusFailed, hookFailure: true,
			want: []diagnostic{{slog.LevelError, logging.Fields{"area": "agent", "operation": "turn", "phase": logging.Failed, "code": "unavailable"}}},
		},
	} {
		t.Run(test.name, func(t *testing.T) {
			profile := filepath.Join(t.TempDir(), "profile")
			closeLog, err := logging.Install(profile, "host")
			if err != nil {
				t.Fatal(err)
			}
			defer closeLog()
			// Start lumberjack's rotation worker outside the synctest bubble.
			logging.Emit(slog.LevelDebug)

			synctest.Test(t, func(t *testing.T) {
				ctx, cancel := context.WithTimeout(t.Context(), 5*time.Minute)
				defer cancel()

				language := &turnDiagnosticModel{statuses: test.responses, waiting: make(chan struct{})}
				environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
				var hookCalls atomic.Int32
				value, err := StartWithOptions(ctx, profile, Options{
					Assembly: fixtureModuleAssembly(), MCP: fixtureMCPOptions(), Environment: &environment,
					Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return language, nil },
					Process: process.Options{Run: func(_ context.Context, launch process.Launch, _ func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
						if launch.Environment["SAILRY_HOOK_EVENT"] != "turn_end" || launch.Environment["SAILRY_HOOK_STATUS"] != "completed" {
							return protocol.ProcessCompletion{}, errors.New("unexpected fixture process")
						}
						hookCalls.Add(1)
						return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(7)}}, nil
					}},
				})
				if err != nil {
					t.Fatal(err)
				}
				defer func() {
					if err := value.Stop(context.Background()); err != nil {
						t.Error(err)
					}
				}()
				value.knowledge.Close()
				var mu sync.Mutex
				var observed []diagnostic
				logging.SetReport(func(level slog.Level, fields logging.Fields) {
					if fields["area"] == "agent" && (fields["operation"] == "model_request" || fields["operation"] == "turn") {
						mu.Lock()
						observed = append(observed, diagnostic{level, fields})
						mu.Unlock()
					}
				})
				defer logging.SetReport(nil)

				provider := protocol.DefaultConversationProvider()
				provider.ID, provider.Name, provider.Enabled = protocol.NewProviderID(), "Fixture", true
				provider.Api, provider.Authentication = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey
				provider.Endpoint, provider.DefaultModel = "https://fixture.invalid/v1", "fixture"
				selected := protocol.DefaultConversationModel()
				selected.ID, selected.Context, selected.Output = provider.DefaultModel, 64000, 8192
				provider.Models = []protocol.ConversationModel{selected}
				key := protocol.NewSecret("test-only-fixture-key")
				provider = diagnosticCommand(t, value, protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: provider, Secret: &key}}).Data.(protocol.ConversationProvider)
				config := protocol.DefaultSessionConfig()
				config.Provider, config.Model, config.Credential = provider.ID, provider.DefaultModel, provider.Credential
				config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
				create := protocol.CommandCreateSessionPayload{Config: &config}
				if test.hookFailure {
					draft := protocol.DefaultProjectsDraft()
					draft.Name, draft.Path, draft.Source = "Fixture", t.TempDir(), protocol.ProjectsSource{Kind: "local"}
					project := diagnosticCommand(t, value, protocol.Command{Kind: "create_project", Data: draft}).Data.(protocol.Project)
					create.Project = &project.ID
					settings := protocol.DefaultHooksSettings()
					settings.Enabled = true
					settings.TurnEnd = &protocol.HooksScript{Name: "Fixture", Enabled: true, Command: "fixture hook", TimeoutMs: 1000}
					diagnosticCommand(t, value, protocol.Command{Kind: "save_hooks", Data: protocol.CommandSaveHooksPayload{Settings: settings}})
				}
				session := diagnosticCommand(t, value, protocol.Command{Kind: "create_session", Data: create}).Data.(protocol.Session)
				turn := diagnosticCommand(t, value, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
					Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Complete the fixture task"},
				}}).Data.(protocol.QueuedTurn).ID
				if test.stop {
					select {
					case <-language.waiting:
					case <-ctx.Done():
						t.Fatal("retry did not reach its second attempt")
					}
					diagnosticCommand(t, value, protocol.Command{Kind: "stop_turn", Data: protocol.CommandStopTurnPayload{Turn: turn}})
				}
				run := waitDiagnosticTurn(t, ctx, value, turn)
				if run.Status != test.status || int(language.calls.Load()) != len(test.responses) {
					t.Fatalf("turn status=%s calls=%d fault=%+v; want %s with %d calls", run.Status, language.calls.Load(), run.Error, test.status, len(test.responses))
				}
				if run.Status == protocol.ConversationStatusCompleted && run.Error != nil {
					t.Fatalf("completed turn retained a fault: %+v", run.Error)
				}
				if test.faultKind != "" && (run.Error == nil || run.Error.Details == nil || run.Error.Details.Kind != test.faultKind) {
					t.Fatalf("turn fault = %+v, want %s", run.Error, test.faultKind)
				}
				if test.hookFailure && (hookCalls.Load() != 1 || run.Error == nil || !strings.Contains(run.Error.Message, "turn_end hook failed")) {
					t.Fatalf("hook calls=%d turn fault=%+v, want one failed turn end hook", hookCalls.Load(), run.Error)
				}
				mu.Lock()
				defer mu.Unlock()
				if !slices.EqualFunc(observed, test.want, func(got, want diagnostic) bool {
					return got.level == want.level && maps.Equal(got.fields, want.fields)
				}) {
					t.Fatalf("agent diagnostics = %+v, want %+v", observed, test.want)
				}
			})
		})
	}
}

func diagnosticCommand(t *testing.T, node *Node, command protocol.Command) protocol.Output {
	t.Helper()
	id := protocol.NewRequestID()
	binary.BigEndian.PutUint64(id[:8], uint64(time.Now().UnixMilli())<<16|0x7000|uint64(binary.BigEndian.Uint16(id[6:8])&0x0fff))
	request := protocol.Request{Version: protocol.VERSION, ID: id, Target: node.ID(), Command: command}
	result := dispatchFixture(t, node, node.ID(), request)
	if result.Fault != nil {
		t.Fatalf("%s: %v", command.Kind, result.Fault)
	}
	return result.Output
}

const turnDiagnosticEmpty = -1

type turnDiagnosticModel struct {
	fantasy.LanguageModel
	statuses []int
	waiting  chan struct{}
	calls    atomic.Int32
}

func (m *turnDiagnosticModel) Stream(ctx context.Context, _ fantasy.Call) (fantasy.StreamResponse, error) {
	index := int(m.calls.Add(1)) - 1
	if index >= len(m.statuses) {
		return nil, errors.New("unexpected extra model request")
	}
	switch status := m.statuses[index]; status {
	case 0:
		return func(func(fantasy.StreamPart) bool) {
			close(m.waiting)
			<-ctx.Done()
		}, nil
	case turnDiagnosticEmpty, http.StatusOK:
		return func(yield func(fantasy.StreamPart) bool) {
			if status == http.StatusOK && !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Done"}) {
				return
			}
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
		}, nil
	default:
		return nil, &fantasy.ProviderError{StatusCode: status, Message: "scripted provider failure"}
	}
}

func waitDiagnosticTurn(t *testing.T, ctx context.Context, node *Node, turn protocol.TurnID) protocol.ConversationRun {
	t.Helper()
	for {
		changed := node.agent.ActivitySignal()
		unlock := node.agent.LockPublication()
		var run protocol.ConversationRun
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			run, err = store.ReadAgentRun(ctx, db, turn)
			return err
		})
		unlock()
		if err != nil {
			t.Fatal(err)
		}
		if run.FinishedMs != nil {
			return run
		}
		select {
		case <-changed:
		case <-ctx.Done():
			t.Fatalf("waiting for turn to settle: %+v", run)
		}
	}
}
