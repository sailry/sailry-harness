package engine

import (
	"context"
	"slices"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const inPlaceHandoff = "```json\n{\"user_intent\":[\"Continue the task\"],\"current_work\":\"Earlier answer recorded\"}\n```"

// cachedHistory appends a main reply whose request was served from the
// provider's prompt cache, followed by a new user request.
func cachedHistory(t *testing.T, f *engineFixture, binding model.Binding, state store.AgentState, cacheRead uint64) store.AgentState {
	t.Helper()
	turn := state.Records[len(state.Records)-1].Turn
	reply := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Earlier answer")), f.r.clock.Now())
	reply.MutableMessage().Usage = &canonical.Usage{CacheRead: &cacheRead}
	scope, purpose := model.Scope(binding), canonical.PurposeMain
	reply.Metadata.Execution = &canonical.Execution{Source: &scope, Purpose: &purpose}
	reply.Metadata.Runtime = &canonical.Runtime{UserVisible: true, AgentVisible: true}
	reply.Provenance.Author = "assistant"
	next := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Next request")), f.r.clock.Now())
	next.Metadata.Runtime = &canonical.Runtime{UserVisible: true, AgentVisible: true}
	if err := f.r.save(f.ctx, f.session.ID, reply, next); err != nil {
		t.Fatal(err)
	}
	return f.state()
}

func messageText(message fantasy.Message) string {
	var parts []string
	for _, part := range message.Content {
		if text, ok := fantasy.AsMessagePart[fantasy.TextPart](part); ok {
			parts = append(parts, text.Text)
		}
	}
	return strings.Join(parts, "\n")
}

func compacted(t *testing.T, f *engineFixture) bool {
	t.Helper()
	for _, record := range f.state().Records {
		if record.Kind == "compaction" {
			return true
		}
	}
	return false
}

func TestInPlaceCompaction(t *testing.T) {
	reasoning := func(binding *model.Binding) {
		binding.Config.Effort = protocol.Effort{Kind: "high"}
		binding.Provider.Models[0].Reasoning = true
		binding.Provider.Models[0].Efforts = []protocol.Effort{{Kind: "high"}, {Kind: "low"}, {Kind: "none"}}
	}
	t.Run("a cached conversation is summarized as its own history", func(t *testing.T) {
		m := &scriptedModel{scripts: []script{response(text(inPlaceHandoff), finish())}}
		f, x, binding, state := compactionFixture(t, m)
		reasoning(&binding)
		state = cachedHistory(t, f, binding, state, 4096)
		var efforts []string
		f.r.model = func(_ context.Context, value model.Binding) (fantasy.LanguageModel, error) {
			efforts = append(efforts, value.Config.Effort.Kind)
			return m, nil
		}
		if err := f.r.compact(x, binding, model.Route{}, state, true); err != nil {
			t.Fatal(err)
		}
		if m.count() != 1 || !compacted(t, f) {
			t.Fatalf("summary requests=%d, compacted=%v", m.count(), compacted(t, f))
		}
		request := m.calls[0].Prompt
		last := messageText(request[len(request)-1])
		if !strings.Contains(last, "replaces the conversation above") || strings.Contains(last, "Conversation History") {
			t.Fatalf("summary instructions were not appended to the history: %q", last)
		}
		history := false
		for _, message := range request {
			history = history || strings.Contains(messageText(message), "Earlier answer")
		}
		if !history || request[0].Role != fantasy.MessageRoleSystem || strings.Contains(messageText(request[0]), "Conversation History") {
			t.Fatal("the summary request did not carry the main system prompt and history")
		}
		// The request keeps the turn's effort: changing it would miss the cache.
		if efforts[len(efforts)-1] != "high" {
			t.Fatalf("in-place summary efforts=%v, want the turn's high effort", efforts)
		}
	})
	t.Run("a large output limit leaves room for the in-place request", func(t *testing.T) {
		m := &scriptedModel{scripts: []script{response(text(inPlaceHandoff), finish())}}
		f, x, binding, state := compactionFixture(t, m)
		binding.Provider.Models[0].Context, binding.Provider.Models[0].Output = 200000, 64000
		state = cachedHistory(t, f, binding, state, 4096)
		if err := f.r.compact(x, binding, model.Route{}, state, true); err != nil {
			t.Fatal(err)
		}
		want := 2 * model.OutputLimit(binding, true)
		if m.count() != 1 || m.calls[0].MaxOutputTokens == nil || *m.calls[0].MaxOutputTokens != want || strings.Contains(messageText(m.calls[0].Prompt[0]), "Conversation History") {
			t.Fatalf("in-place output limit=%v, want %d", m.calls[0].MaxOutputTokens, want)
		}
	})
	t.Run("a tool call falls back to the rendered transcript", func(t *testing.T) {
		m := &scriptedModel{scripts: []script{response(call("c1", "read_file", `{}`), finish()), response(text(inPlaceHandoff), finish())}}
		f, x, binding, state := compactionFixture(t, m)
		reasoning(&binding)
		state = cachedHistory(t, f, binding, state, 4096)
		var efforts []string
		f.r.model = func(_ context.Context, value model.Binding) (fantasy.LanguageModel, error) {
			efforts = append(efforts, value.Config.Effort.Kind)
			return m, nil
		}
		if err := f.r.compact(x, binding, model.Route{}, state, true); err != nil {
			t.Fatal(err)
		}
		if m.count() != 2 || !compacted(t, f) {
			t.Fatalf("summary requests=%d, compacted=%v", m.count(), compacted(t, f))
		}
		if !strings.Contains(messageText(m.calls[1].Prompt[0]), "Conversation History") {
			t.Fatal("the fallback did not render the transcript")
		}
		// The transcript request is built for the low summary effort; the
		// in-place attempt kept the turn's own.
		if !slices.Equal(efforts, []string{"low", "high"}) {
			t.Fatalf("model efforts=%v, want the low summary model and the high in-place model", efforts)
		}
	})
	t.Run("an uncached conversation keeps the rendered transcript", func(t *testing.T) {
		m := &scriptedModel{scripts: []script{response(text(inPlaceHandoff), finish())}}
		f, x, binding, state := compactionFixture(t, m)
		state = cachedHistory(t, f, binding, state, 0)
		if err := f.r.compact(x, binding, model.Route{}, state, true); err != nil {
			t.Fatal(err)
		}
		if m.count() != 1 || !strings.Contains(messageText(m.calls[0].Prompt[0]), "Conversation History") {
			t.Fatal("an uncached conversation did not use the rendered transcript")
		}
	})
}
