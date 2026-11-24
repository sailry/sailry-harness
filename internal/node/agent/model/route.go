package model

import (
	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Route describes how a constructed model reaches its model, beyond the
// window its binding declares. The Node's model factory chooses the transport
// from the binding (provider SDK, package translation or agent program); the
// transport reports these facts. They do not move the compaction threshold,
// which follows the model's window and policy on every route: they shape how
// a compaction runs and how the compacted history is sized.
type Route struct {
	// PromptBytes bounds the text of one prompt. Zero means no bound beside
	// the model's window.
	PromptBytes int
	// Session reports that the transport keeps a conversation per Sailry
	// session and sends it only the messages it has not seen. A request whose
	// earlier history differs replays the whole history in one prompt.
	Session bool
	// Resident reports that such a conversation holds this session's history
	// now, in a running program or one it can resume, so the next request
	// sends only its new messages.
	Resident bool
	// Compact reports that the resident conversation advertises the agent's
	// own `/compact` command (CompactSession).
	Compact bool
}

// promptBytesPerToken converts a prompt byte bound into tokens. o200k text
// averages about four bytes per token for English and code; denser text, such
// as CJK or JSON, has fewer bytes per token and reaches the token budget before
// the byte bound. Overflow recovery covers text that is sparser still.
const promptBytesPerToken = 4

// ReplayWindow is the window of a history sent whole in one prompt: the
// model's window within the token equivalent of PromptBytes. A compacted
// history begins with its summary, which no agent session has seen, so it is
// always sent whole; its summary and retained tail are sized to fit.
func (route Route) ReplayWindow(context uint32) uint32 {
	if route.PromptBytes > 0 {
		return min(context, uint32(route.PromptBytes/promptBytesPerToken))
	}
	return context
}

type routed interface {
	route(session protocol.SessionID) Route
}

// RouteOf reports the route of a model the factory constructed. A transport
// without a session or prompt bound has the zero Route.
func RouteOf(languageModel fantasy.LanguageModel, session protocol.SessionID) Route {
	if model, ok := languageModel.(routed); ok {
		return model.route(session)
	}
	return Route{}
}
