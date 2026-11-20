package engine

import (
	"context"
	"database/sql"
	"strings"
	"sync/atomic"
	"testing"

	"charm.land/fantasy"
	"charm.land/fantasy/providers/anthropic"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestClaudeToolContinuationScopesBoundThinking(t *testing.T) {
	for _, discovery := range []bool{false, true} {
		name := "ordinary tool"
		if discovery {
			name = "discovery keeps the bound request prefix"
		}
		t.Run(name, func(t *testing.T) {
			var executed atomic.Int32
			plain := objectTool("plain", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
				executed.Add(1)
				return ToolResult{Value: "Fixed result"}
			})
			alpha := plain
			alpha.Definition.Name, alpha.Deferred = "alpha", true
			reasoning := func(id string) fantasy.StreamPart {
				return fantasy.StreamPart{Type: fantasy.StreamPartTypeReasoningEnd, ID: id, ProviderMetadata: fantasy.ProviderMetadata{anthropic.Name: &anthropic.ReasoningOptionMetadata{Signature: id}}}
			}
			firstCall, secondCall := call("one", "plain", `{}`), call("two", "plain", `{}`)
			if discovery {
				firstCall, secondCall = call("one", "tool_search", `{"query":"select:alpha"}`), call("two", callToolName, `{"name":"alpha","arguments":{}}`)
			}
			language := &scriptedModel{scripts: []script{
				response(reasoning("first-signature"), text("Working"), firstCall, finish()),
				response(reasoning("second-signature"), secondCall, finish()),
				response(text("Complete"), finish()),
			}}
			f := newFixture(t, language, plain, alpha)
			err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
				_, err := db.ExecContext(f.ctx, `UPDATE providers SET body=json_set(body,
 '$.provider.api','anthropic','$.provider.default_model','claude-opus-5-5',
 '$.provider.models[0].id','claude-opus-5-5','$.provider.models[0].reasoning',json('true'))`)
				if err != nil {
					return err
				}
				_, err = db.ExecContext(f.ctx, `UPDATE session_revisions SET config=json_set(config,'$.model','claude-opus-5-5') WHERE session=?1`, f.session.ID.String())
				return err
			})
			if err != nil {
				t.Fatal(err)
			}
			const instruction = "Use alpha only for this captured workspace."
			f.r.toolsForTurn = func(_ context.Context, invocation *store.AgentInvocation) ([]Tool, error) {
				invocation.Instructions = []store.AgentInstruction{{Text: instruction, AnyTools: []string{"alpha"}}}
				return nil, nil
			}
			f.start()
			f.waitRun(f.submit("Complete the fixed tool workflow"), protocol.ConversationStatusCompleted)
			if language.count() != 3 {
				t.Fatalf("model steps = %d, want 3", language.count())
			}
			signatures := func(request fantasy.Call) []string {
				var values []string
				for _, message := range request.Prompt {
					for _, part := range message.Content {
						if value, ok := part.Options()[anthropic.Name].(*anthropic.ReasoningOptionMetadata); ok && value.Signature != "" {
							values = append(values, value.Signature)
						}
					}
				}
				return values
			}
			second, third := strings.Join(signatures(language.calls[1]), ","), strings.Join(signatures(language.calls[2]), ",")
			// Loading a tool changes neither declarations nor policy, so the
			// reasoning bound to the earlier prefix is still replayed.
			if discovery {
				if second != "first-signature" || third != "first-signature,second-signature" || executed.Load() != 1 {
					t.Fatalf("discovery replay = %q, %q; executions=%d", second, third, executed.Load())
				}
			} else if second != "first-signature" || third != "first-signature,second-signature" || executed.Load() != 2 {
				t.Fatalf("ordinary replay = %q, %q; executions=%d", second, third, executed.Load())
			}
			stored := 0
			for _, record := range f.state().Records {
				if message := record.Message(); message != nil {
					for _, block := range message.Content {
						if thinking, ok := block.Data.(*canonical.Thinking); ok && thinking.Signature != nil {
							stored++
							if record.Metadata.Runtime.Inference.ReplayPrefix == "" {
								t.Fatal("native reasoning lost its captured request prefix")
							}
						}
					}
				}
			}
			if stored != 2 {
				t.Fatalf("canonical thinking records = %d, want 2", stored)
			}
		})
	}
}
