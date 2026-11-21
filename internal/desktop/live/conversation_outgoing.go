package live

import (
	"slices"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// This is in-flight input, not another history. A failed send's recoverable
// input belongs to the composer, not to this local turn.
type conversationOutgoing struct {
	composer.Queued
	input   conversation.Turn
	request protocol.RequestID
	turn    protocol.TurnID
	restore func(*ui.Context, error) bool
}

func (x *conversationActions) stageOutgoing(message composer.Message) *conversationOutgoing {
	if x.r.composerOutgoing == nil {
		x.r.composerOutgoing = map[string]*conversationOutgoing{}
	}
	if x.r.composerOutgoing[x.cv.ID] != nil {
		return nil
	}
	input := conversation.Turn{ID: "outgoing/" + x.cv.ID, User: message.Text, Sent: time.Now(), InputOnly: true}
	for _, attachment := range message.Attachments {
		file := conversation.File{ID: attachment.ID, Path: attachment.Name, Mime: attachment.Mime, Size: attachment.Size, Kind: attachmentFileKind(attachment.Name, attachment.Mime)}
		if file.Kind == conversation.ImageFile {
			image := conversation.Image{ID: attachment.ID, Name: attachment.Name, Source: attachment.Image}
			if image.Source != nil {
				width, height := ui.ImageSize(image.Source)
				image.Width, image.Height = int(width), int(height)
			}
			if attachment.ImageRevision != "" {
				// This session's thumbnail cache owns the draft's bitmap and
				// releases it once no tile shows it. Resolve it by the same key,
				// which the user entry uses too, instead of holding its pixels.
				image.Source, image.OriginalRevision = nil, attachment.ImageRevision
			} else if x.r.attachments != nil {
				if record := x.r.attachments.items[attachment]; record != nil && record.ready != nil {
					image.OriginalRevision = record.ready.Spec.Revision
				}
			}
			input.Images = append(input.Images, image)
		} else {
			input.Files = append(input.Files, file)
		}
	}
	pending := &conversationOutgoing{input: input, restore: func(c *ui.Context, err error) bool {
		if x.restore == nil || x.restoreDraft == composerFingerprint(x.comp) {
			return false
		}
		x.restoreFailedDraft(c, err)
		return true
	}}
	x.r.composerOutgoing[x.cv.ID] = pending
	x.comp.HoldOutgoingAttachments(message.Attachments, func() bool { return x.r.composerOutgoing[x.cv.ID] == pending })
	x.r.projectOutgoing(x.cv, protocol.ConversationPage{})
	x.r.App.Messages.List(x.cv.ID).ScrollToEnd()
	return pending
}

func (x *conversationActions) submitTurn(command protocol.Command, pending *conversationOutgoing, done func(protocol.Output, error)) {
	id := x.cv.ID
	request := x.turn(command, func(output protocol.Output, err error) {
		if pending != nil && (x.r.composerOutgoing[id] == pending || x.r.composerTrayOutgoing[id] == pending) {
			ack, valid := detail[protocol.QueuedTurn](output.Data)
			switch {
			case err != nil:
				x.r.discardCapturedOutgoing(id, pending)
			case pending.turn != (protocol.TurnID{}):
				// The publication already identified this request.
			case output.Kind == "queued_turn" && valid && ack.ID != (protocol.TurnID{}) && ack.Session.String() == id && ack.Request == pending.request:
				pending.turn = ack.ID
			default:
				x.r.discardCapturedOutgoing(id, pending)
			}
		}
		done(output, err)
	})
	if request == (protocol.RequestID{}) {
		// An identical command already in flight does not get another row.
		x.r.discardCapturedOutgoing(id, pending)
		return
	}
	if command.Kind == "queue_turn" || command.Kind == "guide_turn" {
		pending = x.outgoing(*x.restore, command.Kind == "guide_turn")
	}
	if pending != nil {
		pending.request = request
		pending.restore = nil
	}
}

// discardUnsubmitted removes input whose prerequisite failed before a turn
// request was sent; its captured draft remains in the composer.
func (r *Runtime) discardUnsubmitted(id string) {
	if pending := r.composerOutgoing[id]; pending != nil && pending.request == (protocol.RequestID{}) {
		r.discardOutgoing(id)
	}
}

// A failed prerequisite never submitted this message. Drop the local turn,
// but keep its input available even if the composer now has another draft.
func (r *Runtime) failUnsubmitted(c *ui.Context, id string, err error) bool {
	pending := r.composerOutgoing[id]
	if pending == nil || pending.request != (protocol.RequestID{}) {
		return false
	}
	r.discardOutgoing(id)
	return pending.restore != nil && pending.restore(c, err)
}

func (r *Runtime) discardOutgoing(id string) {
	if pending := r.composerOutgoing[id]; pending != nil {
		if cv := r.App.Data.Find(id); cv != nil {
			cv.Turns = slices.DeleteFunc(cv.Turns, func(turn conversation.Turn) bool { return turn.ID == pending.input.ID })
		}
		delete(r.composerOutgoing, id)
	}
}

func (r *Runtime) discardCapturedOutgoing(id string, pending *conversationOutgoing) {
	if pending == nil {
		return
	}
	if r.composerOutgoing[id] == pending {
		r.discardOutgoing(id)
	} else if r.composerTrayOutgoing[id] == pending {
		delete(r.composerTrayOutgoing, id)
	}
}

// An event can beat its response. Match the original request in this session,
// never message text or one of the unrelated queue/configuration revisions.
func (r *Runtime) outgoingTurn(pending *conversationOutgoing, session string, page protocol.ConversationPage) protocol.TurnID {
	if page.Session == (protocol.SessionID{}) || page.Session.String() != session {
		return protocol.TurnID{}
	}
	if pending.turn != (protocol.TurnID{}) || pending.request == (protocol.RequestID{}) {
		return pending.turn
	}
	for _, item := range page.Queue.Items {
		if item.Request != nil && *item.Request == pending.request {
			pending.turn = item.Turn
			return pending.turn
		}
	}
	for _, turn := range r.snapshot().Turns {
		if turn.Session == page.Session && turn.Request == pending.request {
			pending.turn = turn.ID
			break
		}
	}
	return pending.turn
}

func outgoingQueueShown(cv *viewmodel.Conversation, turn string) bool {
	return slices.ContainsFunc(cv.Queue, func(queued composer.Queued) bool { return queued.ID == turn })
}

func outgoingInputShown(cv *viewmodel.Conversation, turn string) bool {
	for _, row := range slices.Backward(cv.Turns) {
		if len(row.Segments) == 0 {
			if row.ID == turn && (row.User != "" || len(row.Images)+len(row.Files) > 0) {
				return true
			}
			continue
		}
		for _, segment := range row.Segments {
			if slices.ContainsFunc(segment.Inputs, func(input conversation.Input) bool {
				return input.ID == turn && (input.User != "" || len(input.Images)+len(input.Files) > 0)
			}) {
				return true
			}
		}
	}
	return false
}

func outgoingRemoved(page protocol.ConversationPage, turn protocol.TurnID) bool {
	return slices.ContainsFunc(page.Runs, func(run protocol.ConversationRun) bool {
		// A rejection before claim retains the input in the queue. Removing
		// that item drops its input while leaving the failed run in history.
		return run.Turn == turn && (run.Status == protocol.ConversationStatusCancelled ||
			run.Status == protocol.ConversationStatusFailed && run.StartedMs == nil)
	}) && !slices.ContainsFunc(page.Queue.Items, func(item protocol.ConversationPending) bool { return item.Turn == turn })
}

// projectOutgoing reconciles against the rows of the page actually presented.
// It returns the overlay only if it modified the prepared rows.
func (r *Runtime) projectOutgoing(cv *viewmodel.Conversation, page protocol.ConversationPage) *conversationOutgoing {
	if pending := r.composerTrayOutgoing[cv.ID]; pending != nil {
		if turn := r.outgoingTurn(pending, cv.ID, page); turn != (protocol.TurnID{}) &&
			(outgoingQueueShown(cv, turn.String()) || outgoingInputShown(cv, turn.String()) || outgoingRemoved(page, turn)) {
			delete(r.composerTrayOutgoing, cv.ID)
		}
	}
	pending := r.composerOutgoing[cv.ID]
	if pending == nil {
		return nil
	}
	turn := r.outgoingTurn(pending, cv.ID, page)
	cv.Turns = slices.DeleteFunc(cv.Turns, func(turn conversation.Turn) bool { return turn.ID == pending.input.ID })
	var overlay *conversationOutgoing
	if turn != (protocol.TurnID{}) {
		turnID := turn.String()
		if outgoingRemoved(page, turn) || outgoingInputShown(cv, turnID) && (slices.ContainsFunc(page.Entries, func(entry protocol.ConversationEntry) bool {
			return entry.Turn == turn && entry.Author == "user"
		}) || slices.ContainsFunc(page.Runs, func(run protocol.ConversationRun) bool {
			return run.Turn == turn && run.Input != nil
		})) {
			r.adoptOutgoingImageFacts(cv.ID, pending.input.Images)
			delete(r.composerOutgoing, cv.ID)
			return nil
		}
		// The Node admits every turn as a queued item and starts it from the
		// queue. This message was sent while the conversation looked idle, so
		// it starts next and keeps its place in the transcript, including
		// while a run the client had not yet seen end is stopping: moving it
		// for a run that ends a moment later made the message jump back and
		// forth. A paused queue, another item ahead or another run that is
		// still running makes it wait, so it moves to the tray where it stays
		// editable and withdrawable.
		for index, item := range page.Queue.Items {
			if item.Turn != turn || !outgoingQueueShown(cv, turnID) {
				continue
			}
			ahead := slices.ContainsFunc(page.Queue.Items[:index], func(other protocol.ConversationPending) bool { return other.Guidance == nil })
			if page.Queue.Paused || ahead || slices.ContainsFunc(page.Runs, func(run protocol.ConversationRun) bool {
				return run.Turn != turn && run.Status == "running"
			}) {
				delete(r.composerOutgoing, cv.ID)
				return nil
			}
			cv.Queue = slices.DeleteFunc(cv.Queue, func(queued composer.Queued) bool { return queued.ID == turnID })
			overlay = pending
			break
		}
		for index := range cv.Turns {
			turn := &cv.Turns[index]
			if turn.ID != turnID {
				continue
			}
			// Until the claimed input arrives, the queue may only supply a
			// clipped preview. Keep this sender's full text and image facts.
			turn.User = pending.input.User
			if turn.Sent.IsZero() {
				turn.Sent = pending.input.Sent
			}
			if len(pending.input.Images)+len(pending.input.Files) != 0 {
				turn.Images = append(turn.Images[:0], pending.input.Images...)
				turn.Files = append(turn.Files[:0], pending.input.Files...)
			}
			return pending
		}
	}
	// Until admission is observed only the user's own input is known. In
	// particular, do not invent a running assistant or a successful outcome.
	cv.Turns = append(cv.Turns, pending.input)
	cv.Session.Loading = false
	return overlay
}

// adoptOutgoingImageFacts gives the user entry its sent preview's layout and
// original revision so the tile neither collapses to a placeholder nor jumps.
// Only facts move, and only to this session's registered attachment of that
// revision. The preview's pixels stay with their owner; the entry's thumbnail
// loads through the session's budgeted cache.
func (r *Runtime) adoptOutgoingImageFacts(session string, images []conversation.Image) {
	if r.attachments == nil {
		return
	}
	cache := r.attachmentImages(session)
	for _, image := range images {
		if image.ID == "" || image.Width <= 0 || image.Height <= 0 || image.OriginalRevision == "" {
			continue
		}
		key := attachmentKey(session, image.ID)
		descriptor, ok := r.attachments.files[key]
		if !ok || descriptor.attachment == nil || descriptor.attachment.Spec.Revision != image.OriginalRevision {
			continue
		}
		if _, known := cache.facts[key]; known {
			continue
		}
		if previous, tracked := cache.sources[key]; tracked && !previous.sameImageSource(descriptor) {
			continue
		}
		cache.sources[key] = descriptor
		cache.facts[key] = attachmentImageFacts{image.Width, image.Height, image.OriginalRevision}
	}
}
