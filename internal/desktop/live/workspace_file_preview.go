package live

import (
	"fmt"
	"path"
	"strings"
	"unicode/utf8"

	htmltomarkdown "github.com/JohannesKaufmann/html-to-markdown/v2"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/fileview"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/office"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func isDocumentPreview(name string) bool {
	switch strings.ToLower(path.Ext(name)) {
	case ".htm", ".html", ".pdf", ".docx", ".xlsx", ".pptx":
		return true
	}
	return false
}

func (p *workspaceProject) openDocumentPreview(relative string) {
	r := p.runtime
	if r.attachments == nil {
		return
	}
	descriptor := attachmentDescriptor{worktree: p.owner.ID, path: relative, name: path.Base(relative)}
	id := "worktree-file/" + r.controller.Target().String() + "/" + p.owner.ID.String() + "/" + relative
	var preview *worktreeDocumentPreview
	p.panel.ShowFilePreview(relative, func(c *ui.Context, view *fileview.View) {
		if preview == nil {
			preview = &worktreeDocumentPreview{
				file:   conversation.File{ID: id, Path: relative, Kind: conversation.OtherFile, State: conversation.PreviewLoading},
				images: newDocumentImages(r.attachments.ctx, r, descriptor, ""),
			}
			r.readWorktreeDocument(descriptor, preview)
		}
		current := preview
		current.draw(c, view, r, func() {
			file := descriptor
			file.revision = current.revision
			r.openExternalAttachment(file, false)
		})
	}, func() {
		if preview != nil {
			preview.images.cancel()
			preview = nil
		}
	})
	p.showItem(panel.Files, relative)
}

type worktreeDocumentPreview struct {
	file     conversation.File
	revision *string
	images   *documentImages
	html     protocol.ArtifactContent
	document fileview.Document
}

func (p *worktreeDocumentPreview) draw(c *ui.Context, view *fileview.View, r *Runtime, open func()) {
	file := p.file
	callbacks := fileview.Callbacks{
		OpenExternal: open, Markdown: p.images.options(),
		Render: func(c *ui.Context, _ conversation.File) bool {
			if htmlFile(file.Path) {
				return r.documentView(c, documentSource{content: p.html, page: file.ID})
			}
			p.document.Draw(c, file.ID)
			return true
		},
	}
	ui.Column(c.Key(file.ID)).Grow(1).MinHeight(0).Children(func() {
		ui.Row(c.Key("toolbar")).Height(36).PaddingX(8).AlignItems(ui.Center).Label(i18n.T(c, "content_preview")).Children(func() {
			fileview.Title(c, file)
			fileview.Tools(c, file, callbacks)
		})
		fileview.Content(c, view, file.ID, file, callbacks)
	})
}

func (r *Runtime) readWorktreeDocument(descriptor attachmentDescriptor, preview *worktreeDocumentPreview) {
	ctx := preview.images.ctx
	isHTML := htmlFile(descriptor.path)
	if !r.async(func() {
		stream, size, revision, err := r.prepareAttachmentDownload(ctx, descriptor)
		limit := uint64(office.Limit)
		if isHTML {
			limit = fileview.Limit
		}
		if err == nil && size > limit {
			r.cancelAttachmentStream(stream)
			r.enqueue(func(*ui.Context) {
				if ctx.Err() == nil {
					preview.file.State = conversation.PreviewLarge
				}
			})
			return
		}
		var loaded fileview.Document
		var fragment, text string
		if err == nil {
			sink := &imageDownloadSink{limit: size}
			err = r.controller.Download(ctx, stream, size, revision, sink, nil)
			if err == nil {
				if isHTML {
					if !utf8.Valid(sink.data) {
						err = fmt.Errorf("HTML preview is not UTF-8 text")
					} else {
						text, err = htmltomarkdown.ConvertString(string(sink.data))
						fragment = preview.images.inlineHTML(string(sink.data))
					}
				} else {
					loaded, err = fileview.ReadDocument(ctx, descriptor.path, sink.data)
				}
			}
		}
		r.enqueue(func(c *ui.Context) {
			if ctx.Err() != nil {
				return
			}
			if err != nil {
				preview.file.State = conversation.PreviewFailed
				r.reportKeys(c, err, "files_open_failed", "workspace_changed", "workspace_action_unknown")
				return
			}
			preview.file.State, preview.file.Size = conversation.PreviewReady, int64(size)
			preview.revision = &revision
			if isHTML {
				preview.file.Kind, preview.file.Text = conversation.MarkdownFile, text
				preview.html.Html = fragment
			} else {
				preview.document = loaded
			}
		})
	}) {
		preview.file.State = conversation.PreviewFailed
	}
}
