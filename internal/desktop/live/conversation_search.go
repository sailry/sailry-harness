package live

import (
	"context"
	"errors"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type conversationReveal struct {
	request uint64
	finish  func(*viewmodel.Conversation, client.ConversationState)
}

func (r *Runtime) initConversationSearch() {
	r.App.Messages.Search = func(session, query string, before *uint64, done func(conversation.SearchPage)) func() {
		ctx, cancel := context.WithCancel(r.ctx)
		cv := r.App.Data.Find(session)
		if cv == nil {
			done(conversation.SearchPage{})
			return cancel
		}
		owner := r.conversationRuntime(cv, nil)
		id, err := protocol.ParseSessionID(session)
		if err != nil || owner == nil {
			done(conversation.SearchPage{})
			return cancel
		}
		r.async(func() {
			if before == nil {
				timer := time.NewTimer(180 * time.Millisecond)
				defer timer.Stop()
				select {
				case <-ctx.Done():
					return
				case <-timer.C:
				}
			}
			page, err := owner.controller.SearchConversation(ctx, id, protocol.ConversationSearchQuery{Text: query, Before: before, Limit: 20})
			r.enqueue(func(c *ui.Context) {
				if ctx.Err() != nil {
					return
				}
				if err != nil {
					r.reportKeys(c, err, "chat_search_failed", "chat_search_failed", "chat_search_failed")
					done(conversation.SearchPage{})
					return
				}
				result := conversation.SearchPage{NextBefore: page.NextBefore}
				for _, found := range page.Matches {
					result.Matches = append(result.Matches, conversation.Match{Turn: -1, User: found.Author == "user", Target: found.Turn.String(), Entry: found.Entry, Sequence: found.Sequence, TurnSequence: found.TurnSequence, Revision: page.Revision})
				}
				done(result)
			})
		})
		return cancel
	}
	r.App.Messages.Reveal = func(session string, target conversation.Match, done func(int, bool)) func() {
		ctx, cancel := context.WithCancel(r.ctx)
		cv := r.App.Data.Find(session)
		if cv == nil {
			done(0, false)
			return cancel
		}
		owner := r.conversationRuntime(cv, nil)
		id, err := protocol.ParseSessionID(session)
		turn, e := protocol.ParseTurnID(target.Target)
		if err != nil || e != nil || owner == nil {
			done(0, false)
			return cancel
		}
		feed := owner.conversationFeeds[id]
		r.async(func() {
			err := owner.controller.RevealConversation(ctx, id, target.Revision, protocol.ConversationSearchMatch{Turn: turn, TurnSequence: target.TurnSequence, Entry: target.Entry, Sequence: target.Sequence})
			r.enqueue(func(c *ui.Context) {
				if ctx.Err() != nil || errors.Is(err, context.Canceled) {
					return
				}
				if err != nil {
					r.reportKeys(c, err, "chat_history_failed", "chat_history_failed", "chat_history_failed")
					done(0, false)
					return
				}
				if feed == nil || owner.conversationFeeds[id] != feed {
					done(0, false)
					return
				}
				// The reveal can finish before its restored page has been projected.
				// Resolve the row in a publication read after that successful reveal.
				feed.reveal = &conversationReveal{request: feed.request.Add(1), finish: func(cv *viewmodel.Conversation, state client.ConversationState) {
					if ctx.Err() != nil {
						return
					}
					if cv != nil && state.Snapshot.Page.Revision == target.Revision {
						ownerTurn := turn
						if joined, listed := turnOwners(state.Snapshot.Page.Runs)[turn]; listed {
							ownerTurn = joined
						}
						for i, row := range cv.Turns {
							if row.ID == ownerTurn.String() {
								done(i, true)
								return
							}
						}
					}
					done(0, false)
				}}
				feed.refresh()
			})
		})
		return cancel
	}
}
