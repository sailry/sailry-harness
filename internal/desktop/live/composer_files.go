package live

import (
	"context"
	"path"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) projectComposerFiles(cv *viewmodel.Conversation, comp *composer.State, ctx *composer.Context) {
	comp.SetReferenceChildren(nil)
	session, ok := r.session(cv.ID)
	if !ok {
		session, ok = r.draftWorkspaceSession(cv.ID)
	}
	if ok {
		if project := r.workspace.projects[session.Worktree]; project != nil {
			ctx.References = append([]composer.Reference{{ID: "files", Source: "files", Kind: composer.Page, Label: "reference_files", Description: "reference_files_description", Key: true, Alias: "files", Group: "reference_group_context", Icon: icons.Folder}}, ctx.References...)
			comp.AddReferenceChildren(func(page composer.Reference) ([]composer.Reference, bool) {
				if page.ID != "files" && (page.Kind != composer.Folder || page.Source != "files") {
					return nil, false
				}
				directory := page.ID
				if page.Kind == composer.Page {
					directory = ""
				}
				return project.pickFiles(directory), true
			})
		}
	}
	if !ctx.Readonly && r.attachments != nil {
		ctx.References = append(ctx.References, composer.Reference{ID: "attach", Kind: composer.AttachFiles, Label: "composer_attachment", Key: true, Group: "reference_group_attach", Icon: icons.Plus})
	}
}

func composerFileReferences(directory string, entries []protocol.FileEntry) []composer.Reference {
	rows := make([]composer.Reference, 0, len(entries))
	for _, entry := range entries {
		relative := path.Join(directory, entry.Name)
		switch entry.Kind {
		case protocol.EntryKindDirectory:
			rows = append(rows, composer.Reference{ID: relative, Source: "files", Kind: composer.Folder, Label: relative, Icon: icons.Folder})
		case protocol.EntryKindFile:
			rows = append(rows, composer.Reference{ID: relative, Kind: composer.File, Label: relative, Icon: icons.File})
		}
	}
	return rows
}

// The composer picker reads only directories the user enters.
type filePicker struct {
	cancel  context.CancelFunc
	ctx     context.Context
	dir     string
	current *filePickerPage
}

type filePickerPage struct {
	rows                    []composer.Reference
	next                    *protocol.DirectoryCursor
	loading, loaded, failed bool
	revision                uint64
}

func (picker *filePicker) close() {
	if picker.cancel != nil {
		picker.cancel()
	}
	*picker = filePicker{}
}

func (p *workspaceProject) pickFiles(directory string) []composer.Reference {
	picker := &p.filePicker
	if picker.current == nil || picker.dir != directory || picker.current.revision != p.fileDirectoryRevision {
		picker.close()
		picker.ctx, picker.cancel = context.WithCancel(p.runtime.ctx)
		picker.dir = directory
		picker.current = &filePickerPage{revision: p.fileDirectoryRevision}
	}
	state := picker.current
	if !state.failed && !state.loading && (!state.loaded || state.next != nil) {
		state.loading = true
		p.readFileDirectory(picker.ctx, directory, state.next, func(_ *ui.Context, page protocol.Directory, err error) {
			state.loading = false
			if err != nil {
				state.failed = true
				return
			}
			state.rows = append(state.rows, composerFileReferences(directory, page.Entries)...)
			state.next, state.loaded = page.Next, true
		})
	}
	return state.rows
}
