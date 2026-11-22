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
	path   string
	line   int
	cancel context.CancelFunc
}

func (p *workspaceProject) cancelFileOpen() {
	if opening := p.openingFile; opening != nil {
		p.openingFile = nil
		opening.cancel()
	}
}

func (p *workspaceProject) openWorkspaceFile(relative string, line int) {
	if opening := p.openingFile; opening != nil && opening.path == relative {
		if line > 0 {
			opening.line = line
		}
		return
	}
	p.cancelFileOpen()
	key := "file/" + relative
	if !validWorkspaceRelative(relative) || p.trashBlocksRead(key) || p.entryIsMissing(relative) {
		return
	}
	media := mime.TypeByExtension(strings.ToLower(path.Ext(relative)))
	if p.runtime.attachments != nil && attachmentFileKind(relative, media) == conversation.ImageFile {
		descriptor := attachmentDescriptor{session: p.session, worktree: p.owner.ID, path: relative, name: path.Base(relative), mime: media}
		p.runtime.openCapturedFilePreview(p.owner.ID.String(), conversation.File{ID: relative, Path: relative, Kind: conversation.ImageFile}, descriptor)
		return
	}
	if _, loaded := p.files[relative]; loaded {
		p.showFileEditor(relative, line)
		return
	}
	ctx, cancel := context.WithCancel(p.runtime.ctx)
	opening := &workspaceFileOpen{path: relative, line: line, cancel: cancel}
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
			if _, loaded := p.files[relative]; !loaded {
				delete(p.failedReads, key)
				p.fileLoaded(relative, value)
			}
			p.showFileEditor(relative, opening.line)
		})
	}) {
		p.cancelFileOpen()
	}
}

func (p *workspaceProject) showFileEditor(relative string, line int) {
	if line > 0 {
		p.panel.FileOpenAt(relative, line)
	} else {
		p.panel.FileOpen(relative)
	}
	p.showItem(panel.Files, relative)
}
