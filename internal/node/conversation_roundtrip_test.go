package node

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// This checks the production admission, SDK stream, feed and controller route
// together. Detailed request shaping and reduction cases live in their owners.
// Over 1 s: each Node start installs and verifies the bundled packages.
func TestAgentRoundTrip(t *testing.T) {
	t.Parallel()
	var requests atomic.Int32
	release := make(chan struct{})
	var once sync.Once
	finish := func() { once.Do(func() { close(release) }) }
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprint(w, "data: {\"id\":\"fixture\",\"object\":\"chat.completion.chunk\",\"model\":\"fixture\",\"created\":1,\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n")
		w.(http.Flusher).Flush()
		select {
		case <-release:
		case <-r.Context().Done():
			return
		}
		fmt.Fprint(w, "data: {\"id\":\"fixture\",\"object\":\"chat.completion.chunk\",\"model\":\"fixture\",\"created\":1,\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":3,\"total_tokens\":15}}\n\ndata: [DONE]\n\n")
	}))
	t.Cleanup(func() { finish(); server.Close() })
	node := nodeFixture(t)
	controller := client.New(link.NewLocal(node.ID(), node.ID(), node))
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { finish(); _ = controller.Close() })
	do := func(command protocol.Command) protocol.Output {
		t.Helper()
		out, err := controller.Do(t.Context(), command)
		if err != nil {
			t.Fatalf("%s: %v", command.Kind, err)
		}
		return out
	}
	provider := protocol.DefaultConversationProvider()
	provider.ID = protocol.NewProviderID()
	provider.Name, provider.Api = "Fixture", protocol.ConversationModelApiChatCompletions
	provider.Authentication, provider.Endpoint = protocol.AuthenticationApiKey, server.URL+"/v1"
	provider.Enabled, provider.DefaultModel = true, "fixture"
	selected := protocol.DefaultConversationModel()
	selected.ID, selected.Context, selected.Output = "fixture", 200000, 8192
	provider.Models = []protocol.ConversationModel{selected}
	secret := protocol.NewSecret("fixture-key")
	do(protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = provider.ID, selected.ID
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	session := do(protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Config: &config}}).Data.(protocol.Session)
	changed := make(chan struct{}, 1)
	releaseWatch, err := controller.WatchConversation(t.Context(), session.ID, func() {
		select {
		case changed <- struct{}{}:
		default:
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	defer releaseWatch()
	input := protocol.DefaultConversationInput()
	input.Text = "Say hello"
	do(protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: input}})
	wait := func(ready func(protocol.ConversationSnapshot) bool) protocol.ConversationSnapshot {
		t.Helper()
		for {
			if state, ok := controller.Conversation(session.ID); ok {
				if state.Err != nil {
					t.Fatal(state.Err)
				}
				for _, run := range state.Snapshot.Page.Runs {
					if run.Error != nil {
						t.Fatal(run.Error)
					}
				}
				if ready(state.Snapshot) {
					return state.Snapshot
				}
			}
			select {
			case <-changed:
			case <-t.Context().Done():
				t.Fatal(context.Cause(t.Context()))
			}
		}
	}
	streaming := wait(func(state protocol.ConversationSnapshot) bool { return len(state.Drafts) > 0 })
	if len(streaming.Page.Queue.Items) != 0 {
		t.Fatal("committed input remained in the pending queue during streaming")
	}
	finish()
	state := wait(func(state protocol.ConversationSnapshot) bool {
		if state.Statistics == nil || state.Statistics.Responses != 1 {
			return false
		}
		for _, run := range state.Page.Runs {
			if run.Status == protocol.ConversationStatusCompleted {
				return true
			}
		}
		return false
	})
	text := ""
	for _, entry := range state.Page.Entries {
		if entry.Author == "assistant" {
			for _, part := range entry.Parts {
				if part.Kind == "text" {
					switch value := part.Data.(type) {
					case string:
						text += value
					case *string:
						text += *value
					}
				}
			}
		}
	}
	if text != "Hello world" || requests.Load() != 1 || len(state.Drafts) != 0 {
		t.Fatalf("production route: text=%q requests=%d drafts=%d", text, requests.Load(), len(state.Drafts))
	}
	t.Run("native task commands use the existing loop", func(t *testing.T) {
		draft := protocol.DefaultConversationStart()
		draft.Config, draft.Title = &config, "Host task"
		draft.Execution = protocol.ConversationExecutionModel
		draft.Message.Text = "Say hello from the first task"
		request := controller.Prepare(protocol.Command{Kind: "start_session", Data: draft})
		output, err := controller.Execute(t.Context(), request)
		if err != nil {
			t.Fatal(err)
		}
		first := output.Data.(protocol.QueuedTurn)
		replayed, err := controller.Execute(t.Context(), request)
		if err != nil {
			t.Fatal(err)
		}
		var recovered protocol.QueuedTurn
		switch value := replayed.Data.(type) {
		case protocol.QueuedTurn:
			recovered = value
		case *protocol.QueuedTurn:
			recovered = *value
		default:
			t.Fatalf("unexpected recovered task admission: %T", replayed.Data)
		}
		if first.Request != request.ID || recovered.ID != first.ID || recovered.Session != first.Session {
			t.Fatal("original task request was not recovered")
		}
		taskChanges := make(chan struct{}, 1)
		releaseTask, err := controller.WatchConversation(t.Context(), first.Session, func() {
			select {
			case taskChanges <- struct{}{}:
			default:
			}
		})
		if err != nil {
			t.Fatal(err)
		}
		defer releaseTask()
		completed := func(turn protocol.TurnID) {
			t.Helper()
			for {
				if current, ok := controller.Conversation(first.Session); ok {
					if current.Err != nil {
						t.Fatal(current.Err)
					}
					for _, run := range current.Snapshot.Page.Runs {
						if run.Turn != turn {
							continue
						}
						if run.Error != nil {
							t.Fatal(run.Error)
						}
						if run.Status == protocol.ConversationStatusCompleted {
							return
						}
					}
				}
				select {
				case <-taskChanges:
				case <-t.Context().Done():
					t.Fatal(context.Cause(t.Context()))
				}
			}
		}
		completed(first.ID)
		message := protocol.DefaultConversationInput()
		message.Text = "Say hello from the next task"
		second := do(protocol.Command{Kind: "submit_task", Data: protocol.CommandSubmitTaskPayload{Session: first.Session, ExpectedRevision: &first.Revision, Message: message}}).Data.(protocol.QueuedTurn)
		completed(second.ID)
		if requests.Load() != 3 {
			t.Fatalf("task commands made %d model requests, expected three including the original round trip", requests.Load())
		}
	})
}
