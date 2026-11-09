package engine

import (
	"strings"
	"testing"

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
