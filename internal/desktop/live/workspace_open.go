package live

import (
	"context"
	"errors"
	"mime"
	"path"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type workspaceFileOpen struct {
	path    string
	line    int
	managed bool
	cancel  context.CancelFunc
	show    func(string, int)
}

func (p *workspaceProject) cancelFileOpen() {
	if opening := p.openingFile; opening != nil {
		p.openingFile = nil
		opening.cancel()
	}
}

func (p *workspaceProject) openWorkspaceFile(relative string, line int) {
	p.openFile(relative, line, false, p.showFileEditor)
}

func (p *workspaceProject) openManagedFile(relative string, line int) {
	p.openFile(relative, line, true, p.showFileEditor)
}

func (p *workspaceProject) openFile(relative string, line int, managed bool, show func(string, int)) {
	if opening := p.openingFile; opening != nil && opening.path == relative {
		if line > 0 {
			opening.line = line
		}
		opening.managed, opening.show = managed, show
		return
	}
	p.cancelFileOpen()
	key := "file/" + relative
	if !validWorkspaceRelative(relative) || p.trashBlocksRead(key) || p.entryIsMissing(relative) {
		return
	}
	// Capture edits delivered since the last editor frame before replacing its tab.
	p.panel.FileDraft(p.panel.Files.File())
	if managed && line == 0 && isDocumentPreview(relative) {
		if p.panel.HasFilePreview(relative) {
			p.showItem(panel.Files, relative)
		} else if _, dirty := p.panel.FileDraft(relative); dirty {
			show(relative, line)
		} else {
			p.openDocumentPreview(relative)
		}
		return
	}
	media := mime.TypeByExtension(strings.ToLower(path.Ext(relative)))
	if p.runtime.attachments != nil && attachmentFileKind(relative, media) == conversation.ImageFile {
		descriptor := attachmentDescriptor{session: p.session, worktree: p.owner.ID, path: relative, name: path.Base(relative), mime: media}
		p.runtime.openCapturedFilePreview(p.owner.ID.String(), conversation.File{ID: relative, Path: relative, Kind: conversation.ImageFile}, descriptor)
		return
	}
	if value, loaded := p.files[relative]; loaded {
		if managed && value.Truncated && !notebookSourceFile(relative) {
			p.openManagedExternal(relative)
		} else {
			show(relative, line)
		}
		return
	}
	ctx, cancel := context.WithCancel(p.runtime.ctx)
	opening := &workspaceFileOpen{path: relative, line: line, managed: managed, cancel: cancel, show: show}
	p.openingFile = opening
	command := protocol.Command{Kind: "read_file", Data: protocol.CommandReadFilePayload{Worktree: p.owner.ID, Path: relative}}
	if !p.runtime.async(func() {
		out, err := p.runtime.controller.Do(ctx, command)
		p.runtime.enqueue(func(c *ui.Context) {
			if p.openingFile != opening {
				return
			}
			p.cancelFileOpen()
			if p.trashBlocksRead(key) || p.entryIsMissing(relative) {
				return
			}
			if err != nil {
				if opening.managed && uneditableFile(err) {
					p.openManagedExternal(relative)
					return
				}
				switch faultCode(err) {
				case protocol.ErrorCodeInvalidRequest, protocol.ErrorCodeCancelled:
				default:
					if !errors.Is(err, context.Canceled) {
						p.runtime.reportKeys(c, err, "files_open_failed", "workspace_changed", "workspace_action_unknown")
					}
				}
				return
			}
			value, ok := detail[protocol.FileContent](out.Data)
			if !ok || out.Kind != "file_content" || value.Path != relative {
				p.workspaceError(c, protocol.ErrorCodeInternal, "invalid file content response")
				return
			}
			if opening.managed && value.Truncated && !notebookSourceFile(relative) {
				p.openManagedExternal(relative)
				return
			}
			if _, loaded := p.files[relative]; !loaded {
				delete(p.failedReads, key)
				p.fileLoaded(relative, value)
			}
			opening.show(relative, opening.line)
		})
	}) {
		p.cancelFileOpen()
	}
}

func (p *workspaceProject) showFileEditor(relative string, line int) {
	p.prepareFileEditor(relative, line)
	p.showItem(panel.Files, relative)
}

func (p *workspaceProject) prepareFileEditor(relative string, line int) {
	p.panel.ClearFilePreview(relative)
	if line > 0 {
		p.panel.FileOpenAt(relative, line)
	} else {
		p.panel.FileOpen(relative)
	}
}

func uneditableFile(err error) bool {
	if faultCode(err) != protocol.ErrorCodeInvalidRequest {
		return false
	}
	switch faultMessage(err) {
	case "file is not UTF-8 text", "binary file cannot be displayed as text":
		return true
	}
	return false
}

func notebookSourceFile(name string) bool { return strings.EqualFold(path.Ext(name), ".ipynb") }

func (p *workspaceProject) openManagedExternal(relative string) {
	p.runtime.OpenWorkspaceExternal(p.owner.ID, relative, nil)
}
