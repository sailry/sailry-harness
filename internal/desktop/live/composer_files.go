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
	var project *workspaceProject
	if ok {
		project = r.workspace.projects[session.Worktree]
	}
	hasFiles := project != nil
	if hasFiles {
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
	hasAttach := !ctx.Readonly && r.attachments != nil
	if catalog := r.composerCatalogs[cv.ID]; catalog != nil && sameReferenceSlice(ctx.References, catalog.references) {
		key := composerFilesReferencesKey{valid: true, referencesGen: catalog.referencesGen, hasFiles: hasFiles, hasAttach: hasAttach}
		if catalog.filesRefKey != key {
			catalog.filesRefKey = key
			catalog.filesRefGen++
			catalog.filesReferences = composeFileReferences(ctx.References, hasFiles, hasAttach)
		}
		ctx.References = catalog.filesReferences
		return
	}
	ctx.References = composeFileReferences(ctx.References, hasFiles, hasAttach)
}

func composeFileReferences(base []composer.Reference, hasFiles, hasAttach bool) []composer.Reference {
	if !hasFiles && !hasAttach {
		return base
	}
	extra := 0
	if hasFiles {
		extra++
	}
	if hasAttach {
		extra++
	}
	refs := make([]composer.Reference, 0, len(base)+extra)
	if hasFiles {
		refs = append(refs, composer.Reference{ID: "files", Source: "files", Kind: composer.Page, Label: "reference_files", Description: "reference_files_description", Key: true, Alias: "files", Group: "reference_group_context", Icon: icons.Folder})
	}
	refs = append(refs, base...)
	if hasAttach {
		refs = append(refs, composer.Reference{ID: "attach", Kind: composer.AttachFiles, Label: "composer_attachment", Key: true, Group: "reference_group_attach", Icon: icons.Plus})
	}
	return refs
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
