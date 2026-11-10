package live

import (
	"context"
	"errors"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/protocol"
)

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
				r.projectConversations()
				cv := r.App.Data.Find(session)
				if cv != nil {
					ownerTurn := turn
					if state, ok := owner.controller.Conversation(id); ok {
						for _, run := range state.Snapshot.Page.Runs {
							if run.Turn == turn && run.Joined != nil {
								ownerTurn = *run.Joined
							}
						}
					}
					for i, row := range cv.Turns {
						if row.ID == ownerTurn.String() {
							done(i, true)
							return
						}
					}
				}
				done(0, false)
			})
		})
		return cancel
	}
}
