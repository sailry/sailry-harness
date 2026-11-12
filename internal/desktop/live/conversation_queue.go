package live

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

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
