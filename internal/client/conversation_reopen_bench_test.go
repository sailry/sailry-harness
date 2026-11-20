package client

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type reopenHistoryTransport struct {
	*conversationTransport
	pages map[protocol.TurnID][]byte
	reads atomic.Uint64
	bytes atomic.Uint64
}

func (t *reopenHistoryTransport) Dispatch(_ context.Context, request protocol.Request) (link.Admission, error) {
	query, ok := request.Command.Data.(protocol.CommandReadConversationPayload)
	if !ok || request.Command.Kind != "read_conversation" || query.Before == nil || query.Limit != 20 {
		return link.Admission{}, fault(protocol.ErrorCodeInvalidRequest, "expected older conversation page")
	}
	body, ok := t.pages[*query.Before]
	if !ok {
		return link.Admission{}, fault(protocol.ErrorCodeNotFound, "fixture page is unavailable")
	}
	t.reads.Add(1)
	t.bytes.Add(uint64(len(body)))
	var out protocol.Output
	if err := protocol.DecodeAs(protocol.PeerData, body, &out); err != nil {
		return link.Admission{}, err
	}
	done := make(chan link.Response, 1)
	done <- link.Response{Output: out}
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: done}, nil
}

func BenchmarkConversationReopenHistory(b *testing.B) {
	const pageSize = 20
	for _, turns := range []int{pageSize, 1000} {
		initial := conversationFixture()
		initial.Page.Runs, initial.Page.Entries = nil, nil
		pages := map[protocol.TurnID][]byte{}
		for end := turns; end > 0; end -= pageSize {
			page := protocol.DefaultConversationPage()
			page.Session, page.Revision = initial.Page.Session, initial.Page.Revision
			for n := end - pageSize + 1; n <= end; n++ {
				turn := protocol.TurnID{byte(n), byte(n >> 8)}
				page.Runs = append(page.Runs, protocol.ConversationRun{Session: page.Session, Worktree: protocol.WorktreeID{1}, Turn: turn, Sequence: uint64(n), Revision: 1, Kind: "task", Status: "completed"})
				page.Entries = append(page.Entries, protocol.ConversationEntry{ID: fmt.Sprint(n), Turn: turn, Sequence: uint64(n), Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: strings.Repeat("Fixture response paragraph.\n", 150)}}})
			}
			if end > pageSize {
				page.NextBefore = new(page.Runs[0].Turn)
			}
			if end == turns {
				initial.Page = page
				continue
			}
			body, err := json.Marshal(protocol.Output{Kind: "conversation", Data: protocol.ConversationHistory{Sequence: initial.Sequence, Page: page, Missing: []protocol.TurnID{}}})
			if err != nil {
				b.Fatal(err)
			}
			pages[protocol.TurnID{byte(end + 1), byte((end + 1) >> 8)}] = body
		}
		for _, recent := range []bool{false, true} {
			b.Run(fmt.Sprintf("turns=%d/recent=%t", turns, recent), func(b *testing.B) {
				transport := &reopenHistoryTransport{conversationTransport: &conversationTransport{opened: make(chan *clientSubscription, 1), initial: func() protocol.ConversationSnapshot { return initial }}, pages: pages}
				c := New(transport)
				defer c.Close()
				watch, count := c.WatchConversation, turns
				if recent {
					watch, count = c.WatchRecentConversation, pageSize
				}
				b.ReportAllocs()
				for b.Loop() {
					c.conversationMu.Lock()
					c.conversationBookmarks = []conversationBookmark{{session: initial.Page.Session, extent: conversationExtent{all: true}}}
					c.conversationMu.Unlock()
					ctx, cancel := context.WithTimeout(b.Context(), 10*time.Second)
					changed := make(chan struct{}, 1)
					release, err := watch(ctx, initial.Page.Session, func() {
						select {
						case changed <- struct{}{}:
						default:
						}
					})
					if err != nil {
						cancel()
						b.Fatal(err)
					}
					<-transport.opened
					for {
						c.conversationMu.Lock()
						w := c.conversations[initial.Page.Session]
						ready := false
						if w != nil {
							snapshot := w.projection.snapshot
							ready = snapshot != nil && len(snapshot.Page.Runs) == count
							err = w.state.OlderErr
						}
						c.conversationMu.Unlock()
						if ready || err != nil {
							break
						}
						select {
						case <-changed:
							continue
						case <-ctx.Done():
							err = ctx.Err()
						}
						break
					}
					release()
					cancel()
					if err != nil {
						b.Fatal(err)
					}
				}
				b.ReportMetric(float64(transport.reads.Load())/float64(b.N), "older-pages/op")
				b.ReportMetric(float64(transport.bytes.Load())/float64(b.N), "older-json-B/op")
			})
		}
	}
}
