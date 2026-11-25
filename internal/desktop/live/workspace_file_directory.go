package live

import (
	"context"
	"path"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (p *workspaceProject) fileDirectoryHost() panel.FileDirectoryHost {
	r := p.runtime
	ctx, cancel := context.WithCancel(r.ctx)
	p.fileDirectoryCancel = cancel
	return panel.FileDirectoryHost{
		Name: path.Base(p.owner.Path), Context: ctx, Read: p.readFileDirectory,
		Revision: func() uint64 { return p.fileDirectoryRevision }, Fault: faultCode,
		CancelOpen: func() {
			if p.openingFile != nil && p.openingFile.managed {
				p.cancelFileOpen()
			}
		},
		Observe: func(ctx context.Context) func() {
			release, err := r.controller.ObserveWorkspace(ctx, p.owner.ID, false, func(protocol.Update) {
				r.enqueue(func(*ui.Context) {
					if ctx.Err() == nil {
						p.pendingFiles = true
					}
				})
			})
			if err != nil {
				return func() {}
			}
			return release
		},
	}
}

func (p *workspaceProject) closeFileDirectory() {
	p.filePicker.close()
	if p.fileDirectoryCancel != nil {
		p.fileDirectoryCancel()
		p.fileDirectoryCancel = nil
	}
	p.cancelFileSearch()
}

func (p *workspaceProject) readFileDirectory(ctx context.Context, name string, after *protocol.DirectoryCursor, done func(*ui.Context, protocol.Directory, error)) {
	if ctx.Err() != nil {
		return
	}
	r := p.runtime
	if name != "" && !validWorkspaceRelative(name) {
		r.enqueue(func(c *ui.Context) {
			done(c, protocol.Directory{}, fileActionFault(protocol.ErrorCodeInvalidRequest, "invalid workspace directory"))
		})
		return
	}
	if len(p.trashing) != 0 {
		r.enqueue(func(c *ui.Context) { done(c, protocol.Directory{}, context.Canceled) })
		return
	}
	fence := p.fileDirectoryFence
	request := r.controller.Prepare(protocol.Command{Kind: "list_directory", Data: protocol.CommandListDirectoryPayload{Worktree: p.owner.ID, Path: name, After: after}})
	r.async(func() {
		out, err := r.controller.Settle(ctx, request)
		r.enqueue(func(c *ui.Context) {
			if ctx.Err() != nil {
				return
			}
			if fence != p.fileDirectoryFence {
				done(c, protocol.Directory{}, context.Canceled)
				return
			}
			page, ok := detail[protocol.Directory](out.Data)
			if err == nil && (out.Kind != "directory" || !ok || page.Path != name) {
				err = fileActionFault(protocol.ErrorCodeInternal, "invalid workspace directory response")
			}
			if err == nil {
				p.entryFound(name)
				for _, entry := range page.Entries {
					if !strings.ContainsAny(entry.Name, "/\\") && validWorkspaceRelative(entry.Name) {
						p.entryFound(path.Join(name, entry.Name))
					}
				}
			} else if faultCode(err) == protocol.ErrorCodeNotFound {
				p.entryMissing(name)
			}
			done(c, page, err)
		})
	})
}
