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
	download, err := r.prepareAttachmentTransfer(ctx, descriptor, 0)
	return download.stream, download.size, download.digest, err
}

type attachmentDownloadStream struct {
	stream           protocol.StreamID
	size             uint64
	digest           string
	originalRevision string
	derived          bool
}

func (r *Runtime) prepareAttachmentTransfer(ctx context.Context, descriptor attachmentDescriptor, thumbnailEdge uint32) (attachmentDownloadStream, error) {
	var command protocol.Command
	if descriptor.image != nil {
		command = protocol.Command{Kind: "download_image", Data: &protocol.CommandDownloadImagePayload{Session: descriptor.session, Image: *descriptor.image, ThumbnailEdge: thumbnailEdge}}
	} else if descriptor.attachment != nil {
		command = protocol.Command{Kind: "download_attachment", Data: &protocol.CommandDownloadAttachmentPayload{Worktree: descriptor.attachment.Spec.Worktree, Attachment: descriptor.attachment.ID, ThumbnailEdge: thumbnailEdge}}
	} else {
		command = protocol.Command{Kind: "download_file", Data: &protocol.CommandDownloadFilePayload{Worktree: descriptor.worktree, Path: descriptor.path, ThumbnailEdge: thumbnailEdge}}
	}
	output, err := r.controller.Do(ctx, command)
	if err != nil {
		return attachmentDownloadStream{}, err
	}
	invalid := func(stream protocol.StreamID, code protocol.ErrorCode, message string) (attachmentDownloadStream, error) {
		if stream != (protocol.StreamID{}) {
			r.cancelAttachmentStream(stream)
		}
		return attachmentDownloadStream{}, fileActionFault(code, message)
	}
	var download attachmentDownloadStream
	var thumbnail *protocol.ThumbnailTransfer
	if descriptor.attachment != nil {
		value, ok := detail[protocol.AttachmentDownload](output.Data)
		if output.Kind != "attachment_download" || !ok {
			return invalid(value.Stream, protocol.ErrorCodeInvalidRequest, "download descriptor is invalid")
		}
		if value.Attachment != *descriptor.attachment || value.Attachment.Spec.Size > math.MaxInt64 {
			return invalid(value.Stream, protocol.ErrorCodeRevisionConflict, "download source changed")
		}
		download.stream, download.size, download.originalRevision = value.Stream, value.Attachment.Spec.Size, value.Attachment.Spec.Revision
		thumbnail = value.Thumbnail
	} else {
		value, ok := detail[protocol.FileDownload](output.Data)
		if output.Kind != "file_download" || !ok {
			return invalid(value.Stream, protocol.ErrorCodeInvalidRequest, "download descriptor is invalid")
		}
		// A card records either the content revision or the source stamp.
		if value.Worktree != descriptor.worktree || value.Path != descriptor.path || value.Size > math.MaxInt64 ||
			descriptor.revision != nil && value.Revision != *descriptor.revision && value.Stamp != *descriptor.revision {
			return invalid(value.Stream, protocol.ErrorCodeRevisionConflict, "download source changed")
		}
		download.stream, download.size, download.originalRevision = value.Stream, value.Size, value.Revision
		thumbnail = value.Thumbnail
	}
	download.digest = download.originalRevision
	if thumbnail != nil {
		if thumbnailEdge == 0 || thumbnail.Size == 0 || thumbnail.Size > protocol.MaxThumbnailBytes || download.originalRevision == "" {
			return invalid(download.stream, protocol.ErrorCodeInvalidRequest, "download thumbnail descriptor is invalid")
		}
		download.size, download.digest, download.derived = thumbnail.Size, thumbnail.Revision, true
	}
	return download, nil
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
	download, err := r.prepareAttachmentTransfer(ctx, descriptor, 0)
	if err != nil {
		return nil, "", err
	}
	data, err := r.receiveAttachmentImage(ctx, download)
	return data, download.originalRevision, err
}

func (r *Runtime) readAttachmentThumbnail(ctx context.Context, descriptor attachmentDescriptor) (*attachmentThumbnail, error) {
	download, err := r.prepareAttachmentTransfer(ctx, descriptor, attachmentThumbnailEdge)
	if err != nil {
		return nil, err
	}
	data, err := r.receiveAttachmentImage(ctx, download)
	if err != nil {
		return nil, err
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return decodeAttachmentThumbnail(data, download.originalRevision, download.derived)
}

func (r *Runtime) receiveAttachmentImage(ctx context.Context, download attachmentDownloadStream) ([]byte, error) {
	// Old Nodes ignore thumbnail_edge and describe an original stream.
	// That compatibility path is bounded by the existing original limits.
	if download.size > attachmentMaximum {
		r.cancelAttachmentStream(download.stream)
		return nil, fileActionFault(protocol.ErrorCodeInvalidRequest, "image exceeds its transfer limit")
	}
	sink := &imageDownloadSink{limit: download.size}
	if err := r.controller.Download(ctx, download.stream, download.size, download.digest, sink, nil); err != nil {
		return nil, err
	}
	return sink.data, nil
}
func (r *Runtime) attachmentImageAction(x *conversationActions, action kit.ImageAction, image conversation.Image) {
	descriptor, ok := r.attachmentDescriptor(x.cv.ID, image.ID)
	if !ok {
		r.report(x.c, fileActionFault(protocol.ErrorCodeNotFound, "image source is unavailable"), "workspace_changed")
		return
	}
	if image.OriginalRevision != "" {
		if descriptor.attachment != nil && descriptor.attachment.Spec.Revision != image.OriginalRevision {
			r.downloadOutcome(x.c, fileActionFault(protocol.ErrorCodeRevisionConflict, "image source changed"))
			return
		}
		if descriptor.path != "" && descriptor.revision == nil {
			descriptor.revision = &image.OriginalRevision
		}
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

// Transcript layout reads facts; an actual tile in view requests pixels.
func (r *Runtime) resolveConversationImage(session string, image conversation.Image) conversation.Image {
	if image.Source != nil || image.Missing || image.ID == "" {
		return image
	}
	if r.attachments == nil || !r.attachmentConversationVisible(session) {
		return image
	}
	cache := r.attachmentImages(session)
	key := attachmentKey(session, image.ID)
	descriptor := r.attachments.files[key]
	if descriptor.artifact != nil {
		if thumbnail := r.projectDocumentThumbnail(session, image.ID, descriptor); thumbnail != nil {
			image.Source, image.Width, image.Height = thumbnail.Source, thumbnail.Width, thumbnail.Height
		}
		return image
	}
	factsOnly := image.ThumbnailFactsOnly()
	if thumbnail := r.resolveAttachmentImage(cache, key, !factsOnly); thumbnail != nil && !factsOnly {
		image.Source = thumbnail.bitmap
	}
	if known, ok := cache.facts[key]; ok {
		image.Width, image.Height, image.OriginalRevision = known.width, known.height, known.revision
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
		cache = &attachmentImageState{ctx: ctx, cancel: cancel, images: map[string]*attachmentThumbnail{}, sources: map[string]attachmentDescriptor{}, facts: map[string]attachmentImageFacts{}, loading: map[string]context.CancelFunc{}, failed: map[string]bool{}, missing: map[string]bool{}}
		state.imageSessions[session] = cache
	}
	return cache
}
func (r *Runtime) resolveAttachmentImage(cache *attachmentImageState, key string, visible bool) *attachmentThumbnail {
	if visible {
		if cache.visible == nil {
			cache.visible = map[string]bool{}
		}
		cache.visible[key] = true
	}
	state := r.attachments
	descriptor, ok := state.files[key]
	if previous, tracked := cache.sources[key]; tracked && (!ok || !previous.sameImageSource(descriptor)) {
		state.releaseThumbnail(cache.images[key])
		delete(cache.images, key)
		if cancel := cache.loading[key]; cancel != nil {
			cancel()
			delete(cache.loading, key)
		}
		delete(cache.sources, key)
		delete(cache.facts, key)
		delete(cache.failed, key)
		delete(cache.missing, key)
	}
	if source := cache.images[key]; source != nil {
		return source
	}
	if !visible || !ok || cache.loading[key] != nil || cache.failed[key] || cache.missing[key] || !state.imageConnected {
		return nil
	}
	ctx, cancel := context.WithCancel(cache.ctx)
	if r.loadAttachmentThumbnail(ctx, attachmentThumbnailEdge, func(finish attachmentThumbnailResult) {
		finish(r.readAttachmentThumbnail(ctx, descriptor))
	}, func(thumbnail *attachmentThumbnail, err error) bool {
		if ctx.Err() != nil {
			return false
		}
		delete(cache.loading, key)
		cancel()
		if current, ok := state.files[key]; !ok || !descriptor.sameImageSource(current) {
			// Leave the old source associated with its facts so the next
			// lookup invalidates them before projecting the new source.
			return false
		}
		if err == nil {
			width, height := thumbnail.bitmap.Size()
			cache.facts[key] = attachmentImageFacts{width, height, thumbnail.originalRevision}
			cache.images[key] = thumbnail
			return true
		}
		// Other failed reads wait for a refresh or reconnect, not another frame.
		if code := faultCode(err); code == protocol.ErrorCodeNotFound || code == protocol.ErrorCodeInvalidRequest {
			delete(cache.facts, key)
			cache.missing[key] = true
		} else {
			cache.failed[key] = true
		}
		return false
	}) {
		cache.sources[key] = descriptor
		cache.loading[key] = cancel
	} else {
		cancel()
	}
	return nil
}

func (d attachmentDescriptor) sameImageSource(other attachmentDescriptor) bool {
	return d.session == other.session && d.worktree == other.worktree && d.path == other.path &&
		sameAttachmentFact(d.revision, other.revision) && sameAttachmentFact(d.attachment, other.attachment) && sameAttachmentFact(d.image, other.image)
}

func sameAttachmentFact[T comparable](a, b *T) bool {
	return a == b || a != nil && b != nil && *a == *b
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
	if config.Width <= 0 || config.Height <= 0 || config.Width > (64<<20)/config.Height {
		return image.Config{}, fileActionFault(protocol.ErrorCodeInvalidRequest, "image dimensions exceed the decoding limit")
	}
	return config, nil
}

func decodeAttachmentThumbnail(data []byte, revision string, derived bool) (*attachmentThumbnail, error) {
	config, err := attachmentImageConfig(data)
	if err != nil || derived && max(config.Width, config.Height) > attachmentThumbnailEdge {
		return nil, fileActionFault(protocol.ErrorCodeInvalidRequest, "image thumbnail dimensions are invalid")
	}
	bitmap, err := ui.DecodeBitmapScaled(data, attachmentThumbnailEdge)
	if err != nil {
		return nil, fileActionFault(protocol.ErrorCodeInvalidRequest, "image could not be decoded")
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
		if attachment.Image != nil && attachment.ImageRevision == "" || attachment.Status != composer.Ready || !strings.HasPrefix(attachment.Mime, "image/") {
			continue
		}
		attachment.Image, attachment.ImageRevision = nil, ""
		if thumbnail := r.resolveAttachmentImage(cache, attachmentKey(x.cv.ID, attachment.ID), true); thumbnail != nil {
			attachment.Image, attachment.ImageRevision = thumbnail.bitmap, thumbnail.originalRevision
		}
	}
}

// pruneAttachmentImages runs after all panes have recorded their conversations.
func (r *Runtime) pruneAttachmentImages() {
	state := r.attachments
	if state == nil {
		return
	}
	before := state.thumbnailPixels
	for key, document := range state.documents {
		if document.visible {
			document.visible = false
			continue
		}
		if document.cancel != nil {
			document.cancel()
		}
		state.releaseThumbnail(document.thumbnail)
		delete(state.documents, key)
	}
	for session, cache := range state.imageSessions {
		id, err := protocol.ParseSessionID(session)
		if err == nil && r.conversationSeen[id] || r.localRuntime().draftsShown[session] {
			if viewer := cache.viewer; viewer != nil {
				if viewer.visible {
					viewer.visible = false
				} else {
					viewer.release()
					cache.viewer = nil
				}
			}
			for key, thumbnail := range cache.images {
				if !cache.visible[key] {
					state.releaseThumbnail(thumbnail)
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
			for key := range cache.sources {
				if _, known := cache.facts[key]; !known && cache.images[key] == nil && cache.loading[key] == nil && !cache.failed[key] && !cache.missing[key] {
					delete(cache.sources, key)
				}
			}
			clear(cache.visible)
			continue
		}
		state.releaseImageSession(cache)
		delete(state.imageSessions, session)
		for key, publication := range state.publications {
			if publication.session.String() == session {
				delete(state.publications, key)
			}
		}
	}
	root := r.localRuntime()
	if state.thumbnailPixels < before && root.attachments != nil && root.attachments.thumbnailBudgetBlocked {
		root.attachments.thumbnailBudgetBlocked = false
		root.wake()
	}
}

func (state *attachmentState) releaseImageSession(cache *attachmentImageState) {
	cache.cancel()
	if cache.viewer != nil {
		cache.viewer.release()
		cache.viewer = nil
	}
	for _, thumbnail := range cache.images {
		state.releaseThumbnail(thumbnail)
	}
	clear(cache.images)
}

func (r *Runtime) attachmentConversationVisible(session string) bool {
	id, err := protocol.ParseSessionID(session)
	return err == nil && r.conversationSeen[id] || r.App != nil && r.App.FocusedConversation() == session
}

var _ io.WriteCloser = (*imageDownloadSink)(nil)
