package engine

import (
	"context"
	"slices"
	"strings"
	"sync/atomic"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestTitleTool(t *testing.T) {
	t.Parallel()
	for _, row := range []struct {
		name, title string
		assigned    bool
	}{{"title commits with response", "Recognizable fixture", true}, {"controls are rejected", "Invalid\nfixture", false}} {
		t.Run(row.name, func(t *testing.T) {
			t.Parallel()
			arguments := `{"title":"Recognizable fixture"}`
			if !row.assigned {
				arguments = `{"title":"Invalid\nfixture"}`
			}
			model := &scriptedModel{scripts: []script{response(call("title", "set_session_title", arguments), finish()), response(text("Final answer"), finish())}}
			fixture := newFixture(t, model)
			fixture.start()
			turn := fixture.submit("Name the task")
			fixture.waitRun(turn, protocol.ConversationStatusCompleted)
			state := fixture.state()
			if row.assigned {
				if state.Context.Title == nil || *state.Context.Title != row.title {
					t.Fatalf("title was not committed: %+v", state.Context.Title)
				}
			} else if state.Context.Title != nil {
				t.Fatal("invalid title changed state")
			}
			if model.count() != 2 {
				t.Fatal("title caused extra model work")
			}
		})
	}
	// Rust parity: the catalog dispatches every registered tool, and a saved
	// request loads its declaration, so a deferred tool called by name runs
	// under its own approval and is declared from the next request on.
	t.Run("deferred tool called by name runs and stays loaded", func(t *testing.T) {
		t.Parallel()
		var ran atomic.Int32
		deferred := objectTool("files_write", false, false, func(context.Context, canonical.ToolRequest) ToolResult {
			ran.Add(1)
			return ToolResult{Value: map[string]any{}}
		})
		deferred.Deferred = true
		model := &scriptedModel{scripts: []script{
			response(call("early", "files_write", `{}`), call("absent", "files_delete", `{}`), finish()),
			response(text("Final answer"), finish()),
		}}
		fixture := newFixture(t, model, deferred)
		fixture.start()
		fixture.waitRun(fixture.submit("Write the file"), protocol.ConversationStatusCompleted)
		if ran.Load() != 1 {
			t.Fatalf("deferred tool ran %d times; want 1", ran.Load())
		}
		offered := func(request int) bool {
			return slices.ContainsFunc(model.calls[request].Tools, func(tool fantasy.Tool) bool { return tool.GetName() == "files_write" })
		}
		if offered(0) || !offered(1) {
			t.Fatal("deferred tool was not declared exactly from the request after its call")
		}
		for _, record := range fixture.state().Records {
			message := record.Message()
			if message == nil {
				continue
			}
			for _, block := range message.Content {
				if value, ok := block.Data.(*canonical.ToolResponse); ok && value.ID == "absent" {
					if value.Status != canonical.ToolErrorStatus || !strings.Contains(string(value.Result.Value), "This tool call was not handled.") {
						t.Fatalf("unregistered call answered %s: %s", value.Status, value.Result.Value)
					}
					return
				}
			}
		}
		t.Fatal("unregistered call has no response")
	})
	t.Run("discovery matches frozen catalog", func(t *testing.T) {
		tools := []Tool{jsonTool("files_read", "Read workspace files", `{"type":"object"}`, true, true, nil), jsonTool("files_search", "Search files", `{"type":"object"}`, true, true, nil), jsonTool("hidden_native", "Read files", `{"type":"object"}`, true, true, nil)}
		tools[0].Deferred = true
		tools[1].Deferred = true
		for _, row := range []struct{ query, want string }{{"please use the tools", ""}, {"select:files_search,files_read,files_search", "files_search,files_read"}, {"files_read", "files_read,files_search"}} {
			matched := builtinMatches(tools, row.query, 8)
			names := []string{}
			for _, entry := range matched {
				names = append(names, entry.Definition.Name)
			}
			if strings.Join(names, ",") != row.want {
				t.Fatalf("%s matched %v; want %s", row.query, names, row.want)
			}
		}
	})
}
