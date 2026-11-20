package live

import (
	"bytes"
	"context"
	"image"
	"io"
	"math"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
	_ "golang.org/x/image/tiff"
)

// A thumbnail also fills the viewer's fitted area while the original loads.
// Kit fits images within 960 by 640 DIPs.
const attachmentThumbnailEdge = 1024

type attachmentThumbnail struct {
	bitmap           *ui.Bitmap
	originalRevision string
}

func (r *Runtime) attachmentDescriptor(session, id string) (attachmentDescriptor, bool) {
	if r.attachments == nil {
		return attachmentDescriptor{}, false
	}
	value, ok := r.attachments.files[attachmentKey(session, id)]
	return value, ok
}
func (r *Runtime) registerConversationAttachment(session string, attachment protocol.Attachment) {
	id, err := protocol.ParseSessionID(session)
	if r.attachments == nil || err != nil && (!strings.HasPrefix(session, "draft/") || attachment.Spec.Worktree != (protocol.WorktreeID{})) {
		return
	}
	if current := r.attachments.files[attachmentKey(session, attachment.ID.String())]; current.image != nil && current.image.Attachment == attachment {
		return
	}
	value := attachment
	r.attachments.files[attachmentKey(session, attachment.ID.String())] = attachmentDescriptor{session: id, worktree: attachment.Spec.Worktree, name: attachment.Spec.Name, mime: attachment.Spec.MediaType, attachment: &value}
}
func (r *Runtime) registerWorkspaceFile(session, id string, worktree protocol.WorktreeID, path string, revision *string) {
	owner, err := protocol.ParseSessionID(session)
	if err != nil || r.attachments == nil {
		return
	}
	r.attachments.files[attachmentKey(session, id)] = attachmentDescriptor{session: owner, worktree: worktree, path: path, name: path, revision: cloneAttachmentRevision(revision)}
}
func cloneAttachmentRevision(value *string) *string {
	if value == nil {
		return nil
	}
	cloned := *value
	return &cloned
}

type attachmentPart struct{ entry, part int }

func attachmentParts(page protocol.ConversationPage) []attachmentPart {
	var parts []attachmentPart
	for i, entry := range page.Entries {
		for j, part := range entry.Parts {
			switch part.Kind {
			case "attachment", "image":
			case "tool_result":
				v, ok := detail[protocol.ConversationPartToolResultPayload](part.Data)
				if !ok || len(v.Images) == 0 && (v.Summary == nil || !v.Summary.Published || !v.Summary.HasDetails) {
					continue
				}
			default:
				continue
			}
			parts = append(parts, attachmentPart{i, j})
		}
	}
	return parts
}

func (r *Runtime) bindAttachmentSources(session string, page protocol.ConversationPage, parts []attachmentPart) {
	if r.attachments == nil {
		return
	}
	owner, err := protocol.ParseSessionID(session)
	if err != nil {
		return
	}
	registerImage := func(value protocol.ConversationImage) {
		attachment := value.Attachment
		r.attachments.files[attachmentKey(session, attachment.ID.String())] = attachmentDescriptor{session: owner, worktree: attachment.Spec.Worktree, name: attachment.Spec.Name, mime: attachment.Spec.MediaType, attachment: &attachment, image: &value}
	}
	for _, source := range parts {
		part := page.Entries[source.entry].Parts[source.part]
		switch part.Kind {
		case "attachment":
			if value, ok := detail[protocol.Attachment](part.Data); ok {
				r.registerConversationAttachment(session, value)
			}
		case "image":
			if value, ok := detail[protocol.ConversationImage](part.Data); ok {
				registerImage(value)
			}
		case "tool_result":
			if value, ok := detail[protocol.ConversationPartToolResultPayload](part.Data); ok {
				for _, image := range value.Images {
					registerImage(image)
				}
			}
		}
	}
	for _, run := range page.Runs {
		if run.Input != nil {
			for _, attachment := range run.Input.Attachments {
				r.registerConversationAttachment(session, attachment)
			}
		}
	}
	for _, queued := range page.Queue.Items {
		for _, attachment := range queued.Attachments {
			r.registerConversationAttachment(session, attachment)
		}
	}
}
func (r *Runtime) openAttachmentFile(x *conversationActions, file conversation.File) {
	descriptor, ok := r.attachmentDescriptor(x.cv.ID, file.ID)
	if !ok {
		r.report(x.c, fileActionFault(protocol.ErrorCodeNotFound, "file source is unavailable"), "workspace_changed")
		return
	}
	if descriptor.path != "" {
		file.Editable = file.Editable && descriptor.artifact == nil
	}
	r.openCapturedFilePreview(x.cv.ID, file, descriptor)
}
func (r *Runtime) prepareAttachmentDownload(ctx context.Context, descriptor attachmentDescriptor) (protocol.StreamID, uint64, string, error) {
	var command protocol.Command
	if descriptor.image != nil {
		command = protocol.Command{Kind: "download_image", Data: &protocol.CommandDownloadImagePayload{Session: descriptor.session, Image: *descriptor.image}}
	} else if descriptor.attachment != nil {
		command = protocol.Command{Kind: "download_attachment", Data: &protocol.CommandDownloadAttachmentPayload{Worktree: descriptor.attachment.Spec.Worktree, Attachment: descriptor.attachment.ID}}
	} else {
		command = protocol.Command{Kind: "download_file", Data: &protocol.CommandDownloadFilePayload{Worktree: descriptor.worktree, Path: descriptor.path}}
	}
	output, err := r.controller.Do(ctx, command)
	if err != nil {
		return protocol.StreamID{}, 0, "", err
	}
	invalid := func(stream protocol.StreamID) (protocol.StreamID, uint64, string, error) {
		r.cancelAttachmentStream(stream)
		return protocol.StreamID{}, 0, "", fileActionFault(protocol.ErrorCodeRevisionConflict, "download source changed")
	}
	if descriptor.attachment != nil {
		value, ok := detail[protocol.AttachmentDownload](output.Data)
		if output.Kind != "attachment_download" || !ok {
			return protocol.StreamID{}, 0, "", fileActionFault(protocol.ErrorCodeInvalidRequest, "download descriptor is invalid")
		}
		if value.Attachment != *descriptor.attachment || value.Attachment.Spec.Size > math.MaxInt64 {
			return invalid(value.Stream)
		}
		return value.Stream, value.Attachment.Spec.Size, value.Attachment.Spec.Revision, nil
	}
	value, ok := detail[protocol.FileDownload](output.Data)
	if output.Kind != "file_download" || !ok {
		return protocol.StreamID{}, 0, "", fileActionFault(protocol.ErrorCodeInvalidRequest, "download descriptor is invalid")
	}
	// A card records the content revision or, as present_files does, the
	// source stamp; either identifies the version it presented.
	if value.Worktree != descriptor.worktree || value.Path != descriptor.path || value.Size > math.MaxInt64 ||
		descriptor.revision != nil && value.Revision != *descriptor.revision && value.Stamp != *descriptor.revision {
		return invalid(value.Stream)
	}
	return value.Stream, value.Size, value.Revision, nil
}

type imageDownloadSink struct {
	data  []byte
	limit uint64
}

func (s *imageDownloadSink) Write(data []byte) (int, error) {
	if uint64(len(s.data))+uint64(len(data)) > s.limit {
		return 0, fileActionFault(protocol.ErrorCodeInvalidRequest, "image exceeds its transfer limit")
	}
	s.data = append(s.data, data...)
	return len(data), nil
}
func (*imageDownloadSink) Close() error { return nil }
func (r *Runtime) readAttachmentImage(ctx context.Context, descriptor attachmentDescriptor) ([]byte, string, error) {
	stream, size, digest, err := r.prepareAttachmentDownload(ctx, descriptor)
	if err != nil {
		return nil, "", err
	}
	if size > attachmentMaximum {
		r.cancelAttachmentStream(stream)
		return nil, "", fileActionFault(protocol.ErrorCodeInvalidRequest, "image exceeds its transfer limit")
	}
	sink := &imageDownloadSink{limit: size}
	if err = r.controller.Download(ctx, stream, size, digest, sink, nil); err != nil {
		return nil, "", err
	}
	return sink.data, digest, nil
}
func (r *Runtime) attachmentImageAction(x *conversationActions, action kit.ImageAction, image conversation.Image) {
	descriptor, ok := r.attachmentDescriptor(x.cv.ID, image.ID)
	if !ok {
		r.report(x.c, fileActionFault(protocol.ErrorCodeNotFound, "image source is unavailable"), "workspace_changed")
		return
	}
	r.attachmentImageDescriptorAction(x.c, action, descriptor)
}
func (r *Runtime) attachmentImageDescriptorAction(c *ui.Context, action kit.ImageAction, descriptor attachmentDescriptor) {
	switch action {
	case kit.ImageSave:
		r.chooseAttachmentDestination(descriptor)
	case kit.ImageOpen:
		r.openExternalAttachment(descriptor, false)
	case kit.ImageCopy:
		native := r.attachments.native
		if native.CopyImage == nil {
			r.report(c, fileActionFault(protocol.ErrorCodeUnavailable, "image clipboard is unavailable"), "workspace_changed")
			return
		}
		r.async(func() {
			data, _, err := r.readAttachmentImage(r.attachments.ctx, descriptor)
			r.enqueue(func(c *ui.Context) {
				if err != nil {
					r.downloadOutcome(c, err)
					return
				}
				native.CopyImage(data, descriptor.mime)
			})
		})
	}
}
func (r *Runtime) draftImageAction(x *conversationActions, action kit.ImageAction, a *composer.Attachment) {
	if record := r.attachments.items[a]; record != nil && record.ready != nil {
		r.attachmentImageAction(x, action, conversation.Image{ID: record.ready.ID.String(), Name: a.Name})
		return
	}
	if action == kit.ImageCopy && len(a.Data) > 0 && r.attachments.native.CopyImage != nil {
		r.attachments.native.CopyImage(a.Data, a.Mime)
		return
	}
	if a.Path != "" && action == kit.ImageOpen && r.attachments.native.OpenLocal != nil {
		r.attachments.native.OpenLocal(a.Path)
		return
	}
	if len(a.Data) > 0 {
		r.localImageAction(a, action)
		return
	}
	r.report(x.c, fileActionFault(protocol.ErrorCodeBusy, "image upload is still preparing"), "workspace_changed")
}

// The transcript resolves only image rows it actually builds. History keeps
// attachment references, while the session cache owns visible decoded bitmaps.
func (r *Runtime) resolveConversationImage(session string, image conversation.Image) conversation.Image {
	if image.Source != nil || image.Missing || image.ID == "" {
		return image
	}
	if r.attachments == nil || !r.attachmentConversationVisible(session) {
		return image
	}
	cache := r.attachmentImages(session)
	key := attachmentKey(session, image.ID)
	if descriptor := r.attachments.files[key]; descriptor.artifact != nil {
		if thumbnail := r.projectDocumentThumbnail(session, image.ID, descriptor); thumbnail != nil {
			image.Source, image.Width, image.Height = thumbnail.Source, thumbnail.Width, thumbnail.Height
		}
		return image
	}
	if thumbnail := r.projectAttachmentImage(cache, key); thumbnail != nil {
		image.Source, image.OriginalRevision = thumbnail.bitmap, thumbnail.originalRevision
		image.Width, image.Height = thumbnail.bitmap.Size()
	}
	image.Missing = cache.missing[key]
	return image
}

func (r *Runtime) attachmentImages(session string) *attachmentImageState {
	state := r.attachments
	connected := r.controller.Connected()
	if connected && !state.imageConnected {
		for _, cache := range state.imageSessions {
			cache.cancel()
			cache.ctx, cache.cancel = context.WithCancel(state.ctx)
			cache.loading = map[string]context.CancelFunc{}
			clear(cache.failed)
		}
		for _, publication := range state.publications {
			if !publication.ready {
				publication.loading, publication.failed = false, false
			}
		}
		for _, document := range state.documents {
			document.loading, document.failed = false, false
		}
	}
	state.imageConnected = connected
	cache := state.imageSessions[session]
	if cache == nil {
		ctx, cancel := context.WithCancel(state.ctx)
		cache = &attachmentImageState{ctx: ctx, cancel: cancel, images: map[string]*attachmentThumbnail{}, loading: map[string]context.CancelFunc{}, failed: map[string]bool{}, missing: map[string]bool{}}
		state.imageSessions[session] = cache
	}
	return cache
}
func (r *Runtime) projectAttachmentImage(cache *attachmentImageState, key string) *attachmentThumbnail {
	if cache.visible == nil {
		cache.visible = map[string]bool{}
	}
	cache.visible[key] = true
	if source := cache.images[key]; source != nil {
		return source
	}
	state := r.attachments
	descriptor, ok := state.files[key]
	if !ok || cache.loading[key] != nil || cache.failed[key] || cache.missing[key] || !state.imageConnected {
		return nil
	}
	ctx, cancel := context.WithCancel(cache.ctx)
	cache.loading[key] = cancel
	r.async(func() {
		if !state.takeImageSlot(ctx) {
			return
		}
		defer func() { <-state.imageSlots }()
		data, revision, err := r.readAttachmentImage(ctx, descriptor)
		if ctx.Err() != nil {
			return
		}
		var thumbnail *attachmentThumbnail
		undecodable := false
		if err == nil {
			thumbnail, err = decodeAttachmentThumbnail(data, revision)
			undecodable = err != nil
		}
		r.enqueue(func(*ui.Context) {
			if ctx.Err() != nil {
				return
			}
			delete(cache.loading, key)
			cancel()
			if err == nil {
				cache.images[key] = thumbnail
				return
			}
			// A picture that is gone or past the transfer and decoding limits
			// never shows. Other failed reads wait for an explicit refresh or
			// a reconnected Node instead of retrying on every frame.
			if code := faultCode(err); undecodable || code == protocol.ErrorCodeNotFound || code == protocol.ErrorCodeInvalidRequest {
				cache.missing[key] = true
			} else {
				cache.failed[key] = true
			}
		})
	})
	return nil
}

func (state *attachmentState) takeImageSlot(ctx context.Context) bool {
	select {
	case state.imageSlots <- struct{}{}:
		return true
	case <-ctx.Done():
		return false
	}
}

func attachmentImageConfig(data []byte) (image.Config, error) {
	if len(data) > attachmentMaximum {
		return image.Config{}, fileActionFault(protocol.ErrorCodeInvalidRequest, "image exceeds its transfer limit")
	}
	config, _, err := image.DecodeConfig(bytes.NewReader(data))
	if err != nil {
		return image.Config{}, err
	}
	if config.Width <= 0 || config.Height <= 0 || uint64(config.Width)*uint64(config.Height) > 256*1024*1024/4 {
		return image.Config{}, fileActionFault(protocol.ErrorCodeInvalidRequest, "image dimensions exceed the decoding limit")
	}
	return config, nil
}

func decodeAttachmentThumbnail(data []byte, revision string) (*attachmentThumbnail, error) {
	config, err := attachmentImageConfig(data)
	if err != nil {
		return nil, err
	}
	bitmap, err := ui.DecodeBitmapScaled(data, attachmentThumbnailEdge)
	if err != nil {
		return nil, err
	}
	if max(config.Width, config.Height) <= attachmentThumbnailEdge {
		revision = ""
	}
	return &attachmentThumbnail{bitmap: bitmap, originalRevision: revision}, nil
}

func decodeAttachmentBitmap(data []byte, maximumEdge ...int) (*ui.Bitmap, error) {
	if _, err := attachmentImageConfig(data); err != nil {
		return nil, err
	}
	if len(maximumEdge) > 0 && maximumEdge[0] > 0 {
		return ui.DecodeBitmapScaled(data, maximumEdge[0])
	}
	return ui.DecodeBitmap(data)
}
func (r *Runtime) projectDraftAttachmentImages(x *conversationActions) {
	cache := r.attachmentImages(x.cv.ID)
	for _, attachment := range x.comp.Attachments {
		if attachment.Image != nil || attachment.Status != composer.Ready || !strings.HasPrefix(attachment.Mime, "image/") {
			continue
		}
		if thumbnail := r.projectAttachmentImage(cache, attachmentKey(x.cv.ID, attachment.ID)); thumbnail != nil {
			attachment.Image, attachment.ImageRevision = thumbnail.bitmap, thumbnail.originalRevision
		}
	}
}

// pruneAttachmentImages runs after all panes have recorded their conversations.
func (r *Runtime) pruneAttachmentImages() {
	if r.attachments == nil {
		return
	}
	for key, document := range r.attachments.documents {
		if document.visible {
			document.visible = false
			continue
		}
		if document.cancel != nil {
			document.cancel()
		}
		delete(r.attachments.documents, key)
	}
	for session, cache := range r.attachments.imageSessions {
		id, err := protocol.ParseSessionID(session)
		if err == nil && r.conversationSeen[id] || r.localRuntime().draftsShown[session] {
			if viewer := cache.viewer; viewer != nil {
				if viewer.visible {
					viewer.visible = false
				} else {
					viewer.cancel()
					cache.viewer = nil
				}
			}
			for key := range cache.images {
				if !cache.visible[key] {
					delete(cache.images, key)
				}
			}
			// A row passed on the way, as an opened transcript goes to its
			// end, would read and decode a picture to drop it.
			for key, cancel := range cache.loading {
				if !cache.visible[key] {
					cancel()
					delete(cache.loading, key)
				}
			}
			clear(cache.visible)
			continue
		}
		cache.cancel()
		cache.viewer = nil
		delete(r.attachments.imageSessions, session)
		for key, publication := range r.attachments.publications {
			if publication.session.String() == session {
				delete(r.attachments.publications, key)
			}
		}
	}
}

func (r *Runtime) attachmentConversationVisible(session string) bool {
	id, err := protocol.ParseSessionID(session)
	return err == nil && r.conversationSeen[id] || r.App != nil && r.App.FocusedConversation() == session
}

var _ io.WriteCloser = (*imageDownloadSink)(nil)
