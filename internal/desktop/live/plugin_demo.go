package live

import (
	"encoding/json"
	"math"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Demo mode shows the statistics package over a demo conversation as over a
// session: its figures in the composer's context bar and its context panel
// in the header. The package's view is mounted for each demo conversation
// shown, without a session, under the presentation "demo/<id>", and reads
// the composer state the host gives it (readComposer), which is built from
// the conversation's own usage and transcript. No other package is mounted there: theirs
// would ask the Node. Nothing of the demo is sent to a Node; the package's
// bundle is the one another of its mounts read, and is read from the Node
// only where none has it yet.
const (
	pluginStatisticsPackage = "statistics"
	pluginDemoPresentation  = "demo/"
)

// demoPluginRegistry is the contributions shown over a demo conversation.
func (r *Runtime) demoPluginRegistry(cv *viewmodel.Conversation) *plugins.Registry {
	scope := protocol.DefaultPluginContext()
	scope.Surface.Kind = "composer"
	return r.localRuntime().pluginRegistry(scope, pluginDemoPresentation+cv.ID)
}

// demoConversation makes a demo conversation's figures the statistics
// package's (shell.Hooks.DemoConversation).
func (r *Runtime) demoConversation(_ *ui.Context, cv *viewmodel.Conversation, comp *composer.State) {
	root := r.localRuntime()
	if registry := root.demoPluginRegistry(cv); registry != nil {
		root.projectPluginMetrics(comp, registry)
	}
}

// refreshDemoComposer publishes the composer state a demo conversation's
// mount reads, and reports whether the mount is one. The demo has no model
// context: its provider is no provider of the Node.
func (state *pluginNativeState) refreshDemoComposer() bool {
	id, demo := strings.CutPrefix(state.presentation, pluginDemoPresentation)
	if !demo || state.scope.Session != nil {
		return false
	}
	var projection map[string]any
	var failure error
	root := state.runtime.localRuntime()
	if view := root.App.DemoConversation(id); view == nil {
		failure = pluginNativeFault(protocol.ErrorCodeUnavailable, "composer source is unavailable")
	} else {
		projection, failure = demoComposer(view, root.App.NavShown())
	}
	state.composer.mu.Lock()
	publishNativeProjection(&state.composer.view, projection, failure)
	publishNativeProjection(&state.composer.model, nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "model context is unavailable"))
	state.composer.mu.Unlock()
	return true
}

// demoComposer is the composer state of a demo conversation in the shape
// projectComposer gives a session's: connected, idle, with the statistics
// of the conversation's own usage and the tool calls its transcript shows.
func demoComposer(view *viewmodel.Conversation, sidebar bool) (map[string]any, error) {
	recorded := demoStatistics(view.Session.Usage)
	if recorded != nil {
		recorded.Observations.Tools = demoTools(view.Turns)
	}
	statistics, err := plugins.ComposerStatistics(recorded)
	if err != nil {
		return nil, err
	}
	var limit *uint32
	if recorded != nil {
		limit = recorded.ContextLimit
	}
	return map[string]any{
		"command": nil, "mode": protocol.WorkModeCode, "mode_selection": nil, "modes": []any{},
		"session": nil, "connected": true, "connection": map[string]any{"route": "local", "rtt_ms": nil},
		"busy": false, "configuring": false, "readonly": false,
		"model_tools": true, "model": nil, "context_limit": limit, "statistics": statistics,
		"compacting": false, "has_messages": len(view.Turns) > 0, "sidebar": sidebar,
		"options": map[string]json.RawMessage{}, "operations": []any{},
	}, nil
}

// demoStatistics are a sample usage as a session's statistics: the tokens
// split into input and output at demoOutputShare, the cached part of the
// input, the cost with every response priced, the speed as the output over
// its time, and the context used of the window. Nothing else is recorded:
// the demo has no requests to observe. A conversation without usage has
// none.
func demoStatistics(usage *conversation.Usage) *protocol.ConversationStatistics {
	if usage == nil {
		return nil
	}
	statistics := protocol.DefaultConversationStatistics()
	count := func(value float64) uint64 { return uint64(math.Max(0, math.Round(value))) }
	turns := count(float64(usage.Turns))
	statistics.Turns, statistics.Responses = turns, turns
	tokens := count(float64(usage.Tokens))
	output := tokens / demoOutputShare
	input := tokens - output
	cached := min(count(float64(usage.Cached)), input)
	statistics.Usage = &protocol.ConversationUsage{Input: input, Output: output, CachedInput: cached}
	statistics.Observations.CacheRead = &cached
	statistics.Cost = &protocol.UsageCost{UsdMicros: count(usage.Cost * 1_000_000), Responses: turns}
	if usage.Speed > 0 && output > 0 {
		statistics.Generation = &protocol.UsageGeneration{
			ElapsedUs: count(float64(output) * 1_000_000 / usage.Speed), OutputTokens: output, Responses: turns,
		}
	}
	if usage.Window > 0 {
		window := uint32(usage.Window)
		used := count(math.Max(0, math.Min(1, usage.Context)) * float64(usage.Window))
		statistics.ContextTokens, statistics.ContextLimit = &used, &window
	}
	return &statistics
}

// demoTools counts a transcript's tool calls by name, as a session's
// observations do. A question or a plan review is an interaction, not a call.
func demoTools(turns []conversation.Turn) map[string]uint64 {
	tools := map[string]uint64{}
	for _, turn := range turns {
		for _, block := range turn.Reply.Blocks {
			for _, call := range block.Calls {
				if call.Name != "" && !call.Terminal {
					tools[call.Name]++
				}
			}
		}
	}
	return tools
}

// demoOutputShare is the part of a sample usage's tokens that is output: one
// in eight, the rest input.
const demoOutputShare = 8
