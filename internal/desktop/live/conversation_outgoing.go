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

// This is the same in-flight input the composer retains for failure recovery,
// not another history. It never enters Client or survives a settled rejection.
type conversationOutgoing struct {
	composer.Queued
	input      conversation.Turn
	request    protocol.RequestID
	turn       protocol.TurnID
	transcript bool
	restore    func(*ui.Context, error) bool
}

func (x *conversationActions) stageOutgoing(message composer.Message) {
	if x.r.composerOutgoing == nil {
		x.r.composerOutgoing = map[string]*conversationOutgoing{}
	}
	if pending := x.r.composerOutgoing[x.cv.ID]; pending != nil && pending.transcript {
		return
	}
	input := conversation.Turn{ID: "outgoing/" + x.cv.ID, User: message.Text, Sent: time.Now(), Reply: conversation.Reply{Notice: true}}
	for _, attachment := range message.Attachments {
		file := conversation.File{ID: attachment.ID, Path: attachment.Name, Mime: attachment.Mime, Size: attachment.Size, Kind: attachmentFileKind(attachment.Name, attachment.Mime)}
		if file.Kind == conversation.ImageFile {
			image := conversation.Image{ID: attachment.ID, Name: attachment.Name, Source: attachment.Image}
			if image.Source != nil {
				width, height := ui.ImageSize(image.Source)
				image.Width, image.Height = int(width), int(height)
			}
			input.Images = append(input.Images, image)
		} else {
			input.Files = append(input.Files, file)
		}
	}
	x.r.composerOutgoing[x.cv.ID] = &conversationOutgoing{input: input, transcript: true, restore: func(c *ui.Context, err error) bool {
		if x.restore == nil || x.restoreDraft == composerFingerprint(x.comp) {
			return false
		}
		x.offerRestore(c, err)
		return true
	}}
	x.r.projectOutgoing(x.cv, protocol.ConversationPage{})
	x.r.App.Messages.List(x.cv.ID).ScrollToEnd()
}

// discardUnsubmitted removes input whose prerequisite failed before a turn
// request was sent; its captured draft remains in the composer.
func (r *Runtime) discardUnsubmitted(id string) {
	if pending := r.composerOutgoing[id]; pending != nil && pending.transcript && pending.request == (protocol.RequestID{}) {
		r.discardOutgoing(id)
	}
}

// A failed prerequisite never submitted this message. Keep newer input in the
// composer and offer the same captured message back from its failure toast.
func (r *Runtime) failUnsubmitted(c *ui.Context, id string, err error) bool {
	pending := r.composerOutgoing[id]
	if pending == nil || !pending.transcript || pending.request != (protocol.RequestID{}) {
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

func (r *Runtime) projectOutgoing(cv *viewmodel.Conversation, page protocol.ConversationPage) {
	pending := r.composerOutgoing[cv.ID]
	if pending == nil || !pending.transcript {
		return
	}
	// A Node event can beat its command response. Match the immutable request,
	// never the draft's text, so equal messages from other turns stay distinct.
	if pending.turn == (protocol.TurnID{}) && pending.request != (protocol.RequestID{}) {
		for _, item := range page.Queue.Items {
			if item.Request != nil && *item.Request == pending.request {
				pending.turn = item.Turn
				break
			}
		}
		for _, turn := range r.snapshot().Turns {
			if turn.Session.String() == cv.ID && turn.Request == pending.request {
				pending.turn = turn.ID
				break
			}
		}
	}
	cv.Turns = slices.DeleteFunc(cv.Turns, func(turn conversation.Turn) bool { return turn.ID == pending.input.ID })
	if pending.turn != (protocol.TurnID{}) {
		for _, item := range page.Queue.Items {
			if item.Turn == pending.turn && !slices.ContainsFunc(page.Runs, func(run protocol.ConversationRun) bool {
				return run.Turn == pending.turn && (run.Status == "running" || run.Status == "stopping")
			}) {
				delete(r.composerOutgoing, cv.ID)
				return
			}
		}
		for _, entry := range page.Entries {
			if entry.Turn == pending.turn && entry.Author == "user" {
				delete(r.composerOutgoing, cv.ID)
				return
			}
		}
		for index := range cv.Turns {
			turn := &cv.Turns[index]
			if turn.ID != pending.turn.String() {
				continue
			}
			if turn.User == "" {
				turn.User = pending.input.User
			}
			if turn.Sent.IsZero() {
				turn.Sent = pending.input.Sent
			}
			if len(turn.Images) == 0 && len(turn.Files) == 0 {
				turn.Images, turn.Files = slices.Clone(pending.input.Images), slices.Clone(pending.input.Files)
			}
			return
		}
	}
	// Until admission is observed only the user's own input is known. In
	// particular, do not invent a running assistant or a successful outcome.
	cv.Turns = append(cv.Turns, pending.input)
	cv.Session.Loading = false
}
