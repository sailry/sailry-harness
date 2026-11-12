package live

import (
	"context"
	"errors"
	"slices"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) uploadAttachment(x *conversationActions, a *composer.Attachment) {
	if r.attachments == nil {
		r.initAttachments()
	}
	if existing := r.attachments.items[a]; existing != nil {
		if existing.running {
			return
		}
		if existing.pending != nil {
			r.checkAttachmentPending(x, a, existing)
			return
		}
		if existing.ready != nil {
			a.Status = composer.Ready
			return
		}
		if existing.cancel != nil {
			existing.cancel()
		}
	}
	record := &attachmentRecord{composer: x.comp}
	r.attachments.items[a] = record
	a.Status, a.Error, a.Progress = composer.Preparing, "", 0
	x.ensureAttachmentSession(func(c *ui.Context, session protocol.Session, err error) {
		if r.attachments.items[a] != record {
			return
		}
		if err != nil {
			a.Status, a.Error = composer.Failed, "files_upload_failed"
			r.attachmentOutcome(c, err)
			return
		}
		record.session, record.worktree = session.ID, session.Worktree
		ctx, cancel := context.WithCancel(r.attachments.ctx)
		record.cancel, record.running = cancel, true
		path, data, name, media := a.Path, slices.Clone(a.Data), a.Name, a.Mime
		r.async(func() {
			if name == "" {
				r.finishAttachment(a, record, protocol.Output{}, sourceAttachmentFault("files_upload_name", protocol.ErrorCodeInvalidRequest, "attachment name is unavailable"), nil, protocol.AttachmentSpec{})
				return
			}
			select {
			case r.attachments.slots <- struct{}{}:
				defer func() { <-r.attachments.slots }()
			case <-ctx.Done():
				r.finishAttachment(a, record, protocol.Output{}, ctx.Err(), nil, protocol.AttachmentSpec{})
				return
			}
			source, err := localSource(ctx, path, data, media)
			if err != nil {
				r.finishAttachment(a, record, protocol.Output{}, err, nil, protocol.AttachmentSpec{})
				return
			}
			defer source.reader.Close()
			spec := protocol.AttachmentSpec{Worktree: record.worktree, Name: name, MediaType: source.mime, Size: source.size, Revision: source.revision}
			output, err := r.controller.Do(ctx, protocol.Command{Kind: "upload_attachment", Data: &spec})
			if err != nil {
				r.finishAttachment(a, record, protocol.Output{}, err, nil, spec)
				return
			}
			upload, ok := detail[protocol.AttachmentUpload](output.Data)
			if output.Kind != "attachment_upload" || !ok || upload.Spec != spec {
				if ok {
					r.cancelAttachmentStream(upload.Stream)
				}
				r.finishAttachment(a, record, protocol.Output{}, fileActionFault(protocol.ErrorCodeInvalidRequest, "attachment upload descriptor changed"), nil, spec)
				return
			}
			last := time.Time{}
			err = r.controller.Upload(ctx, upload.Stream, spec.Size, spec.Revision, source.reader, func(copied uint64) {
				now := time.Now()
				if copied != 0 && copied != spec.Size && !last.IsZero() && now.Sub(last) < 75*time.Millisecond {
					return
				}
				last = now
				r.enqueue(func(*ui.Context) {
					if r.attachments.items[a] == record && !record.removing {
						a.Status, a.Size, a.Progress = composer.Sending, int64(spec.Size), 1
						if spec.Size > 0 {
							a.Progress = float32(copied) / float32(spec.Size)
						}
					}
				})
			})
			_ = source.reader.Close()
			if err != nil {
				r.finishAttachment(a, record, protocol.Output{}, err, nil, spec)
				return
			}
			if err = ctx.Err(); err != nil {
				r.cancelAttachmentStream(upload.Stream)
				r.finishAttachment(a, record, protocol.Output{}, err, nil, spec)
				return
			}
			pending := r.controller.Prepare(protocol.Command{Kind: "finish_attachment_upload", Data: &protocol.CommandFinishAttachmentUploadPayload{Worktree: record.worktree, Stream: upload.Stream}})
			r.enqueue(func(*ui.Context) {
				if r.attachments.items[a] == record {
					record.pending, record.expected = &pending, spec
					if !record.removing {
						a.Status = composer.Publishing
					}
				}
			})
			output, err = r.controller.Execute(r.attachments.ctx, pending)
			r.finishAttachment(a, record, output, err, &pending, spec)
		})
	})
}
func (r *Runtime) cancelAttachmentStream(stream protocol.StreamID) {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	_, _ = r.controller.Do(ctx, protocol.Command{Kind: "cancel_file_transfer", Data: &protocol.CommandCancelFileTransferPayload{Stream: stream}})
}
func (r *Runtime) finishAttachment(a *composer.Attachment, record *attachmentRecord, output protocol.Output, err error, pending *protocol.Request, expected protocol.AttachmentSpec) {
	r.enqueue(func(c *ui.Context) {
		if r.attachments.items[a] != record {
			return
		}
		record.running = false
		if pending != nil {
			record.pending, record.expected = pending, expected
		}
		// unexpected is an answer the Node gave that is not this
		// attachment: asking about the request again reads the same answer,
		// so the request ends with an uncertain toast and is not kept.
		unexpected := false
		if err == nil {
			attachment, ok := detail[protocol.Attachment](output.Data)
			if output.Kind != "attachment" || !ok || attachment.Spec != expected {
				err, unexpected = fileActionFault(protocol.ErrorCodeOutcomeUnknown, "attachment publication result is unavailable"), true
			} else {
				record.ready, record.pending = &attachment, nil
				a.ID, a.Name, a.Mime, a.Size, a.Status, a.Progress, a.Error = attachment.ID.String(), attachment.Spec.Name, attachment.Spec.MediaType, int64(attachment.Spec.Size), composer.Ready, 1, ""
				r.registerConversationAttachment(record.session.String(), attachment)
				if record.removing {
					r.startAttachmentDiscard(a, record)
				}
				return
			}
		}
		if record.removing && pending == nil {
			r.removeDraftAttachment(a, record)
			return
		}
		a.Status, a.Error = composer.Failed, "files_upload_failed"
		var sourceError *attachmentSourceError
		if errors.As(err, &sourceError) {
			a.Error = sourceError.key
		}
		if errors.Is(err, context.Canceled) || faultCode(err) == protocol.ErrorCodeCancelled {
			a.Error = "files_upload_cancelled"
		}
		if faultCode(err) == protocol.ErrorCodeRevisionConflict {
			a.Error = "files_upload_changed"
		}
		if unexpected {
			record.pending = nil
			if record.removing {
				r.removeDraftAttachment(a, record)
				return
			}
			kit.Toast(c, kit.Uncertain, i18n.T(c, "chat_attachment_unknown"))
			return
		}
		if pending != nil && attachmentRequestUncertain(err) {
			a.Status, a.Error = composer.Uncertain, "chat_attachment_unknown"
		} else if pending != nil {
			record.pending = nil
			if record.removing {
				r.removeDraftAttachment(a, record)
				return
			}
		}
		if a.Status == composer.Uncertain {
			kit.Toast(c, kit.Uncertain, i18n.T(c, "chat_attachment_unknown"))
			return
		}
		r.attachmentOutcome(c, err)
	})
}
func (r *Runtime) checkAttachmentPending(x *conversationActions, a *composer.Attachment, record *attachmentRecord) {
	if record.running || record.pending == nil {
		return
	}
	pending, expected := *record.pending, record.expected
	record.running = true
	a.Status = composer.Publishing
	if record.removing || pending.Command.Kind == "discard_attachment" {
		a.Status = composer.Removing
	}
	r.async(func() {
		output, err := r.retryAttachmentRequest(r.attachments.ctx, pending)
		if pending.Command.Kind == "discard_attachment" {
			r.finishAttachmentDiscard(a, record, output, err, pending)
		} else {
			r.finishAttachment(a, record, output, err, &pending, expected)
		}
	})
}
func (r *Runtime) discardAttachment(x *conversationActions, a *composer.Attachment) {
	if r.attachments == nil {
		return
	}
	record := r.attachments.items[a]
	if record == nil {
		record = &attachmentRecord{composer: x.comp}
		if session, ok := r.session(x.cv.ID); ok {
			record.session, record.worktree = session.ID, session.Worktree
		}
		if id, err := protocol.ParseAttachmentID(a.ID); err == nil {
			record.ready = &protocol.Attachment{ID: id, Spec: protocol.AttachmentSpec{Worktree: record.worktree, Name: a.Name, MediaType: a.Mime, Size: uint64(max(a.Size, 0))}}
		}
		r.attachments.items[a] = record
	}
	record.removing = true
	if record.cancel != nil {
		record.cancel()
	}
	if record.running {
		a.Status = composer.Removing
		return
	}
	if record.pending != nil {
		r.checkAttachmentPending(x, a, record)
		return
	}
	if record.ready == nil {
		r.removeDraftAttachment(a, record)
		return
	}
	r.startAttachmentDiscard(a, record)
}
func (r *Runtime) startAttachmentDiscard(a *composer.Attachment, record *attachmentRecord) {
	pending := r.controller.Prepare(protocol.Command{Kind: "discard_attachment", Data: &protocol.CommandDiscardAttachmentPayload{Worktree: record.ready.Spec.Worktree, Attachment: record.ready.ID}})
	record.pending, record.running = &pending, true
	a.Status = composer.Removing
	r.async(func() {
		output, err := r.controller.Execute(r.attachments.ctx, pending)
		r.finishAttachmentDiscard(a, record, output, err, pending)
	})
}
func (r *Runtime) finishAttachmentDiscard(a *composer.Attachment, record *attachmentRecord, output protocol.Output, err error, pending protocol.Request) {
	r.enqueue(func(c *ui.Context) {
		if r.attachments.items[a] != record {
			return
		}
		record.running = false
		result, ok := detail[protocol.OutputAttachmentDiscardedPayload](output.Data)
		expected := pending.Command.Data.(*protocol.CommandDiscardAttachmentPayload)
		if err == nil && (output.Kind != "attachment_discarded" || !ok || result.Attachment != expected.Attachment) {
			err = fileActionFault(protocol.ErrorCodeOutcomeUnknown, "attachment discard result is unavailable")
		}
		if err == nil || faultCode(err) == protocol.ErrorCodeNotFound || faultCode(err) == protocol.ErrorCodeConflict {
			r.removeDraftAttachment(a, record)
			return
		}
		a.Status, a.Error = composer.Failed, "chat_attachment_remove_failed"
		if attachmentRequestUncertain(err) {
			record.pending = &pending
			a.Status, a.Error = composer.Uncertain, "chat_attachment_unknown"
			kit.Toast(c, kit.Uncertain, i18n.T(c, "chat_attachment_unknown"))
			return
		}
		// The request ended: the next removal is a new one.
		record.pending = nil
		r.attachmentOutcome(c, err)
	})
}

// attachmentRequestUncertain reports whether a publication's request is kept
// to be asked about again. A released request is not: the Node holds nothing
// more for it.
func attachmentRequestUncertain(err error) bool {
	if client.Released(err) {
		return false
	}
	code := faultCode(err)
	return code == protocol.ErrorCodeOutcomeUnknown || code == protocol.ErrorCodeUnavailable || code == protocol.ErrorCodeInternal
}
