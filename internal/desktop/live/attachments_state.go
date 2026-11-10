package live

import (
	"context"
	"errors"
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/transfers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type NativeFileActions struct {
	LocalNode      protocol.NodeID
	PickFiles      func(func([]string))
	SaveFile       func(string, func(string))
	OpenLocal      func(string)
	CopyImage      func([]byte, string)
	RevealLocal    func(string)
	RenderDocument func(context.Context, protocol.ArtifactSummary, string, func([]byte, error))
	DocumentView   func(*ui.Context, protocol.ArtifactSummary, string) bool
}
type attachmentRecord struct {
	cancel            context.CancelFunc
	running, removing bool
	session           protocol.SessionID
	worktree          protocol.WorktreeID
	ready             *protocol.Attachment
	pending           *protocol.Request
	expected          protocol.AttachmentSpec
	composer          *composer.State
}
type attachmentDescriptor struct {
	session          protocol.SessionID
	worktree         protocol.WorktreeID
	path, name, mime string
	revision         *string
	attachment       *protocol.Attachment
	image            *protocol.ConversationImage
	artifact         *protocol.ArtifactSummary
}
type attachmentImageState struct {
	ctx     context.Context
	cancel  context.CancelFunc
	images  map[string]*ui.Bitmap
	loading map[string]bool
}
type attachmentState struct {
	ctx                context.Context
	cancel             context.CancelFunc
	native             NativeFileActions
	items              map[*composer.Attachment]*attachmentRecord
	files              map[string]attachmentDescriptor
	publications       map[string]*attachmentPublication
	previews           map[string]attachmentDescriptor
	previewGeneration  map[string]uint64
	previewHTML        map[string]string
	documents          map[string]*attachmentDocumentImage
	slots              chan struct{}
	download           *transfers.Download
	downloadCancel     context.CancelFunc
	downloadGeneration uint64
	imageSessions      map[string]*attachmentImageState
	imageConnected     bool
	imageSlots         chan struct{}
	mu                 sync.Mutex
	closed             bool
	temporary          map[string]func()
}

func (r *Runtime) initAttachments() {
	ctx, cancel := context.WithCancel(r.ctx)
	r.attachments = &attachmentState{ctx: ctx, cancel: cancel, items: map[*composer.Attachment]*attachmentRecord{}, files: map[string]attachmentDescriptor{}, publications: map[string]*attachmentPublication{}, previews: map[string]attachmentDescriptor{}, previewGeneration: map[string]uint64{}, previewHTML: map[string]string{}, documents: map[string]*attachmentDocumentImage{}, slots: make(chan struct{}, 2), imageSessions: map[string]*attachmentImageState{}, imageSlots: make(chan struct{}, 2), temporary: map[string]func(){}}
	if r.App != nil && r.App.Panel != nil {
		r.App.Panel.SetArtifactCallbacks(panel.ArtifactCallbacks{OpenExternal: r.OpenArtifactExternal, Image: r.ArtifactImageAction, Render: r.renderArtifactPreview})
	}
}
func (r *Runtime) closeAttachments() {
	state := r.attachments
	if state == nil {
		return
	}
	state.cancel()
	state.mu.Lock()
	state.closed = true
	release := state.temporary
	state.temporary = map[string]func(){}
	state.mu.Unlock()
	for _, close := range release {
		close()
	}
}
func (r *Runtime) SetNativeFileActions(actions NativeFileActions) {
	if r.attachments == nil {
		r.initAttachments()
	}
	r.attachments.native = actions
}
func attachmentKey(session, id string) string { return session + "\x00" + id }
func (r *Runtime) bindComposerAttachments(x *conversationActions) {
	if r.attachments == nil {
		return
	}
	r.projectDraftAttachmentImages(x)
	callbacks := &composer.AttachmentCallbacks{Remove: func(a *composer.Attachment) { r.discardAttachment(x, a) }, Image: func(action kit.ImageAction, a *composer.Attachment) { r.draftImageAction(x, action, a) }}
	if picker := r.attachments.native.PickFiles; picker != nil {
		callbacks.Pick = func() {
			picker(func(paths []string) { r.enqueue(func(c *ui.Context) { x.c = c; x.comp.AttachPaths(c, paths, x) }) })
		}
	}
	x.comp.SetAttachmentCallbacks(callbacks)
}
func (r *Runtime) attachmentsCommitted(attachments []*composer.Attachment) {
	if r.attachments == nil {
		return
	}
	for _, attachment := range attachments {
		if record := r.attachments.items[attachment]; record != nil {
			if record.cancel != nil {
				record.cancel()
			}
			delete(r.attachments.items, attachment)
		}
	}
}
func (r *Runtime) attachmentDownloadDialog(c *ui.Context) {
	if r.attachments == nil || r.attachments.download == nil {
		return
	}
	transfers.DownloadDialog(c, r.attachments.download, func() {
		if r.attachments.downloadCancel != nil {
			r.attachments.downloadCancel()
		}
	})
}
func (r *Runtime) attachmentOutcome(c *ui.Context, err error) {
	key := "files_upload_failed"
	var sourceError *attachmentSourceError
	if errors.As(err, &sourceError) {
		key = sourceError.key
	}
	if errors.Is(err, context.Canceled) || faultCode(err) == protocol.ErrorCodeCancelled {
		kit.Notify(c, "", i18n.T(c, "files_upload_cancelled"))
		return
	}
	r.reportKeys(c, err, key, "files_upload_changed", "chat_attachment_unknown")
}
func (r *Runtime) removeDraftAttachment(attachment *composer.Attachment, record *attachmentRecord) {
	items := record.composer.Attachments
	for index, item := range items {
		if item == attachment {
			record.composer.Attachments = append(items[:index:index], items[index+1:]...)
			break
		}
	}
	delete(r.attachments.items, attachment)
}
func (r *Runtime) attachmentFileAction(x *conversationActions, file conversation.File, action conversation.FileAction) {
	switch action {
	case conversation.RequestEdit:
		descriptor, ok := r.attachmentDescriptor(x.cv.ID, file.ID)
		if !ok || descriptor.path == "" || !file.Editable {
			r.report(x.c, fileActionFault(protocol.ErrorCodeUnavailable, "file edit source is unavailable"), "workspace_changed")
			return
		}
		session, ok := r.session(x.cv.ID)
		if !ok || session.Worktree != descriptor.worktree {
			r.report(x.c, fileActionFault(protocol.ErrorCodeRevisionConflict, "file edit source belongs to another workspace"), "workspace_changed")
			return
		}
		x.comp.InsertFiles([]string{descriptor.path}, i18n.T(x.c, "artifact_edit_prompt"))
	case conversation.Download:
		r.downloadFileCard(x, file, false)
	case conversation.OpenExternal:
		r.downloadFileCard(x, file, true)
	}
}
