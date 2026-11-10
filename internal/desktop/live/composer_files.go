package live

import (
	"path"

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
				project.readDirectory(directory, false)
				if project.directoryPages[directory].Next != nil {
					project.nextDirectory(directory)
				}
				return composerFileReferences(directory, project.directoryEntries[directory]), true
			})
		}
	}
	if !ctx.Readonly && r.attachments != nil && r.attachments.native.PickFiles != nil {
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
