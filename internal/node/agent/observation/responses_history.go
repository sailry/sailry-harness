package observation

import (
	"context"
	"crypto/rand"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const maxResponsesSessions = 16

// ResponsesHistory carries one receipt per recently completed session in memory.
// The engine serializes Start and Complete with turn claims and durable ends.
type ResponsesHistory struct {
	recent []responsesHandoff
}

type responsesHandoff struct {
	session protocol.SessionID
	key     [32]byte
	receipt responsesReceipt
}

func (history *ResponsesHistory) Start(ctx context.Context, session protocol.SessionID, turn protocol.TurnID) (context.Context, *ResponsesTrace) {
	trace := &ResponsesTrace{session: session, turn: turn}
	for i, prior := range history.recent {
		if prior.session == session {
			history.recent = slices.Delete(history.recent, i, i+1)
			if prior.receipt.turn != turn {
				trace.key, trace.previous = prior.key, prior.receipt
				return context.WithValue(ctx, responsesTraceKey{}, trace), trace
			}
			break
		}
	}
	rand.Read(trace.key[:])
	return context.WithValue(ctx, responsesTraceKey{}, trace), trace
}

// Complete hands off only a receipt observed by this execution. A turn with no
// measurable last attempt cannot forward the previous turn's receipt.
func (history *ResponsesHistory) Complete(ctx context.Context) {
	trace := ResponsesTraceFrom(ctx)
	if trace == nil {
		return
	}
	trace.mu.Lock()
	defer trace.mu.Unlock()
	receipt := trace.previous
	if receipt.shape == nil || receipt.turn != trace.turn {
		return
	}
	trace.previous = responsesReceipt{}
	if len(history.recent) == maxResponsesSessions {
		history.recent = slices.Delete(history.recent, 0, 1)
	}
	history.recent = append(history.recent, responsesHandoff{session: trace.session, key: trace.key, receipt: receipt})
}
