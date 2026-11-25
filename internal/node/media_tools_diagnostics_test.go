package node

import (
	"bytes"
	"context"
	"database/sql"
	"fmt"
	"image"
	"image/png"
	"io"
	"log/slog"
	"maps"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestInspectMediaToolDiagnostics(t *testing.T) {
	t.Setenv("SAILRY_TELEMETRY", "off")
	ctx, cancel := context.WithTimeout(t.Context(), 20*time.Second)
	defer cancel()
	profile := filepath.Join(t.TempDir(), "profile")
	closeLog, err := logging.Install(profile, "host")
	if err != nil {
		t.Fatal(err)
	}
	defer closeLog()

	var calls [][3]string
	for _, prompt := range []string{"upstream", "stream_error", "truncated", "empty", "rejected", "cancelled"} {
		calls = append(calls, [3]string{prompt, "inspect_image", fmt.Sprintf(`{"path":"image.png","prompt":%q}`, prompt)})
	}
	calls = append(calls, [3]string{"bad_file", "inspect_image", `{"path":"bad.dat","prompt":"invalid image"}`})
	main := &mediaScript{steps: [][][3]string{calls}, results: map[string]string{}}
	language := &inspectMediaDiagnosticModel{main: main}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(ctx, profile, Options{Assembly: fixtureModuleAssembly(), MCP: fixtureMCPOptions(), Environment: &environment,
		Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return language, nil }})
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if err := node.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	}()
	var mu sync.Mutex
	var observed []logging.Fields
	logging.SetReport(func(level slog.Level, fields logging.Fields) {
		if level == slog.LevelError && fields["area"] == "agent" && (fields["operation"] == "tool_execution" || fields["operation"] == "model_request") {
			mu.Lock()
			observed = append(observed, fields)
			mu.Unlock()
		}
	})
	defer logging.SetReport(nil)

	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	var picture bytes.Buffer
	if err := png.Encode(&picture, image.NewRGBA(image.Rect(0, 0, 2, 2))); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "image.png"), picture.Bytes(), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "bad.dat"), []byte("not an image"), 0o600); err != nil {
		t.Fatal(err)
	}
	draft := protocol.DefaultProjectsDraft()
	draft.Name, draft.Path, draft.Source = "Media fixture", root, protocol.ProjectsSource{Kind: "local"}
	project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: draft}).Data.(protocol.Project)
	key := protocol.NewSecret("fixture-key")
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Name, provider.Enabled = protocol.NewProviderID(), "Private fixture provider", true
	provider.Api, provider.Authentication, provider.Endpoint, provider.DefaultModel = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, "https://fixture.invalid/v1", "fixture"
	vision := protocol.DefaultConversationModel()
	vision.ID, vision.Tools, vision.Vision, vision.CustomCapabilities, vision.Context, vision.Output = provider.DefaultModel, true, true, true, 32000, 8000
	provider.Models = []protocol.ConversationModel{vision}
	provider = commandFixture(t, node, protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &key}}).Data.(protocol.ConversationProvider)
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Credential, config.Mode, config.Permission = provider.ID, provider.DefaultModel, provider.Credential, protocol.WorkModeCode, protocol.PermissionFull
	session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: &protocol.CommandCreateSessionPayload{Config: &config, Project: &project.ID}}).Data.(protocol.Session)
	turn := commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
		Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Inspect the fixture images"},
	}}).Data.(protocol.QueuedTurn).ID
	for {
		changed := node.agent.ActivitySignal()
		var run protocol.ConversationRun
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			run, err = store.ReadAgentRun(ctx, db, turn)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if run.FinishedMs != nil {
			if run.Status != protocol.ConversationStatusCompleted {
				t.Fatalf("turn did not recover from tool failures: %+v", run)
			}
			break
		}
		select {
		case <-changed:
		case <-ctx.Done():
			t.Fatal("turn did not settle")
		}
	}
	if requests := language.vision.Load(); requests != 6 {
		t.Fatalf("vision requests = %d, want 6 without retries or invalid file requests", requests)
	}
	main.mu.Lock()
	defer main.mu.Unlock()
	for id, want := range map[string]string{"stream_error": "Model stream ended before a completion event", "truncated": "vision model returned no text", "empty": "vision model returned no text", "bad_file": "image could not be decoded"} {
		if !strings.Contains(main.results[id], want) {
			t.Fatalf("tool response %s = %q, want %q", id, main.results[id], want)
		}
	}
	mu.Lock()
	defer mu.Unlock()
	counts := map[string]int{}
	for _, fields := range observed {
		cause, _ := fields["cause_code"].(string)
		want := logging.Fields{"area": "agent", "operation": "tool_execution", "phase": logging.Failed, "code": "unavailable", "cause_code": cause}
		if !maps.Equal(fields, want) {
			t.Fatalf("unexpected diagnostics: %v", fields)
		}
		counts[cause]++
	}
	if want := map[string]int{"model_unavailable": 1, "model_invalid_response": 2}; !maps.Equal(counts, want) {
		t.Fatalf("vision diagnostics = %v, want %v", counts, want)
	}
}

type inspectMediaDiagnosticModel struct {
	fantasy.LanguageModel
	main   *mediaScript
	vision atomic.Int32
}

func (m *inspectMediaDiagnosticModel) Stream(ctx context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	for _, message := range call.Prompt {
		var prompt string
		var image bool
		for _, part := range message.Content {
			switch part := part.(type) {
			case fantasy.TextPart:
				prompt = part.Text
			case fantasy.FilePart:
				image = true
			}
		}
		if !image {
			continue
		}
		m.vision.Add(1)
		switch prompt {
		case "upstream":
			return nil, &fantasy.ProviderError{StatusCode: http.StatusBadGateway, Message: "private provider error"}
		case "stream_error":
			return func(yield func(fantasy.StreamPart) bool) {
				yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: io.ErrUnexpectedEOF})
			}, nil
		case "truncated":
			return func(func(fantasy.StreamPart) bool) {}, nil
		case "empty":
			return func(yield func(fantasy.StreamPart) bool) {
				yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
			}, nil
		case "rejected":
			return nil, &fantasy.ProviderError{StatusCode: http.StatusBadRequest, Message: "private invalid input"}
		case "cancelled":
			return nil, context.Canceled
		default:
			return nil, fmt.Errorf("unexpected vision request %q", prompt)
		}
	}
	return m.main.Stream(ctx, call)
}
