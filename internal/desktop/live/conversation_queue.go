package live

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func pendingQueueItem(queue protocol.ConversationQueue, id string) (protocol.ConversationPending, bool) {
	for _, item := range queue.Items {
		if item.Turn.String() == id {
			return item, true
		}
	}
	return protocol.ConversationPending{}, false
}

type queuedSend struct {
	request       protocol.Request
	busy          bool
	queueRevision uint64
}

// An accepted send can still be visible at the head while the stopped turn
// exits. A later pause, move or withdrawal can make it actionable again.
func (r *Runtime) reconcileQueuedSends(session protocol.SessionID, queue protocol.ConversationQueue) {
	sends := r.queuedSends[session]
	for turn, send := range sends {
		if send.queueRevision == 0 || queue.Revision < send.queueRevision {
			continue
		}
		if queue.Paused || len(queue.Items) == 0 || queue.Items[0].Turn != turn || !queue.Items[0].Ready {
			delete(sends, turn)
		}
	}
	if len(sends) == 0 {
		delete(r.queuedSends, session)
	}
}

func (r *Runtime) sendQueuedTurn(session protocol.SessionID, queueRevision uint64, item protocol.ConversationPending) {
	sends := r.queuedSends[session]
	attempt := sends[item.Turn]
	checking := attempt != nil
	if checking {
		if attempt.busy || attempt.queueRevision != 0 {
			return
		}
	} else {
		if r.queuedSends == nil {
			r.queuedSends = map[protocol.SessionID]map[protocol.TurnID]*queuedSend{}
		}
		if sends == nil {
			sends = map[protocol.TurnID]*queuedSend{}
			r.queuedSends[session] = sends
		}
		request := r.controller.Prepare(protocol.Command{Kind: "send_queued_turn", Data: protocol.CommandSendQueuedTurnPayload{Turn: item.Turn, ExpectedRevision: item.Revision}})
		attempt = &queuedSend{request: request}
		sends[item.Turn] = attempt
	}
	attempt.busy = true
	r.async(func() {
		var output protocol.Output
		var err, lookupErr error
		if checking {
			var resolution client.Resolution
			resolution, lookupErr = r.controller.Resolve(r.ctx, attempt.request)
			if lookupErr == nil {
				output, err = resolution.Result()
			}
		} else {
			output, err = r.controller.Settle(r.ctx, attempt.request)
		}
		r.enqueue(func(c *ui.Context) {
			attempt.busy = false
			if lookupErr != nil {
				kit.Toast(c, kit.Uncertain, i18n.T(c, "chat_action_unknown"))
				return
			}
			unknown := "chat_action_unknown"
			switch {
			case err == nil:
				attempt.queueRevision = queueRevision + 1
				if update, ok := detail[protocol.ConversationQueueUpdate](output.Data); ok && update.Session == session && update.Queue.Revision != 0 {
					attempt.queueRevision = update.Queue.Revision
				}
			case client.Released(err) && faultCode(err) == protocol.ErrorCodeOutcomeUnknown:
				attempt.queueRevision = queueRevision + 1
				unknown = "tool_interrupted"
			case !client.Retained(err):
				delete(sends, item.Turn)
			}
			r.reportKeys(c, err, "chat_action_failed", "session_changed", unknown)
		})
	})
}

// enrichQueue gives the rows a user can edit their reader and writer. A
// notice, a compaction and a continuation a package queued (a marker row)
// are automatic rows the strip draws without an editor, and get neither.
func (r *Runtime) enrichQueue(cv *viewmodel.Conversation, queue protocol.ConversationQueue) {
	for i := range cv.Queue {
		// The strip leaves out the turns that started, so a row is found by
		// its turn, not its place.
		index := slices.IndexFunc(queue.Items, func(item protocol.ConversationPending) bool { return item.Turn.String() == cv.Queue[i].ID })
		if index < 0 || cv.Queue[i].Notice || cv.Queue[i].Compaction {
			continue
		}
		item := queue.Items[index]
		cv.Queue[i].Load = func(done func(string, bool)) {
			r.readQueued(item, func(value protocol.ConversationQueuedMessage, ok bool) { done(value.Message.Text, ok) })
		}
		cv.Queue[i].Save = func(text string, done func(bool)) { r.saveQueued(item, text, done) }
	}
}

// saveQueued replaces the text of a queued message and keeps the rest of it.
func (r *Runtime) saveQueued(item protocol.ConversationPending, text string, done func(bool)) {
	r.readQueued(item, func(value protocol.ConversationQueuedMessage, ok bool) {
		if ok {
			value.Message.Text = text
			r.command(protocol.Command{Kind: "edit_queued_turn", Data: protocol.CommandEditQueuedTurnPayload{Turn: item.Turn, ExpectedRevision: item.Revision, Message: value.Message}}, "session_changed", func(_ protocol.Output, ok bool) {
				if done != nil {
					done(ok)
				}
			})
		} else if done != nil {
			done(false)
		}
	})
}

func (r *Runtime) readQueued(item protocol.ConversationPending, done func(protocol.ConversationQueuedMessage, bool)) {
	// Not execute, which answers one of two identical commands in flight: an
	// editor reopened before its first read answered waits for its own.
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_queued_turn", Data: protocol.CommandReadQueuedTurnPayload{Turn: item.Turn}})
		r.enqueue(func(c *ui.Context) {
			value, ok := detail[protocol.ConversationQueuedMessage](out.Data)
			if err == nil && (!ok || value.Turn.ID != item.Turn || value.Revision != item.Revision) {
				err = &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "Queued message changed before editing"}}
			}
			if err != nil {
				r.report(c, err, "session_changed")
			}
			done(value, err == nil)
		})
	})
}
