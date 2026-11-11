package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// changedFile is what a file holds on disk while its draft began from other
// content; missing is a file that is gone.
type changedFile struct {
	content protocol.FileContent
	missing bool
}

// draftSource records what a file holds on disk while its draft is open. The
// draft keeps the source it began from, so a save still tells the Node which
// content the user edited; the editor shows that the file changed.
func (p *workspaceProject) draftSource(relative string, disk changedFile) {
	source := p.files[relative]
	if !disk.missing && source.Revision != nil && disk.content.Revision != nil && *source.Revision == *disk.content.Revision {
		p.clearChanged(relative)
		return
	}
	if p.changedFiles == nil {
		p.changedFiles = map[string]changedFile{}
	}
	p.changedFiles[relative] = disk
	p.panel.SetFileChanged(relative, true)
}

func (p *workspaceProject) clearChanged(relative string) {
	delete(p.changedFiles, relative)
	p.panel.SetFileChanged(relative, false)
}

// settleChanged shows what a changed file holds on disk once its draft is
// gone: the user reloaded, undid the edits or discarded them, so nothing is
// left to keep.
func (p *workspaceProject) settleChanged(relative string) {
	disk, changed := p.changedFiles[relative]
	if !changed {
		return
	}
	if _, dirty := p.panel.FileDraft(relative); dirty {
		return
	}
	if disk.missing {
		p.clearChanged(relative)
		p.fileUnavailable(relative)
		return
	}
	p.fileLoaded(relative, disk.content)
}

// checkDraft reads the file under a draft again, for a refresh or for a save
// the Node refused, and records what it holds. A read that fails for another
// reason than the file being gone leaves the record as it is; done, when
// given, receives the failure.
func (p *workspaceProject) checkDraft(reason, relative string, done func(*ui.Context, error)) {
	command := protocol.Command{Kind: "read_file", Data: protocol.CommandReadFilePayload{Worktree: p.owner.ID, Path: relative}}
	p.runtime.executeScoped("workspace-read/"+p.owner.ID.String()+"/"+reason+"/"+relative, command, func(c *ui.Context, out protocol.Output, err error) {
		_, loaded := p.files[relative]
		if _, dirty := p.panel.FileDraft(relative); loaded && dirty {
			if value, ok := detail[protocol.FileContent](out.Data); err == nil && ok {
				p.draftSource(relative, changedFile{content: value})
			} else if faultCode(err) == protocol.ErrorCodeNotFound {
				err = nil
				p.draftSource(relative, changedFile{missing: true})
			}
		}
		if done != nil {
			done(c, err)
		}
	})
}

// writeFile saves text over the content expected names: the source a draft
// began from, or what the user chose to overwrite (nil for a file that is
// gone). A file that holds something else is not written; the user is asked.
func (p *workspaceProject) writeFile(relative, text string, expected *string) {
	source := p.files[relative]
	p.panel.SetFileStatus(relative, true, source.Truncated)
	command := protocol.Command{Kind: "write_file", Data: protocol.CommandWriteFilePayload{Worktree: p.owner.ID, Path: relative, Text: text, ExpectedRevision: expected}}
	p.mutateNamed(fileSave, command, func(c *ui.Context, out protocol.Output) {
		p.panel.SetFileStatus(relative, false, source.Truncated)
		written, ok := detail[protocol.FileWritten](out.Data)
		if !ok {
			return
		}
		source.Text, source.Revision, source.Size = text, &written.Revision, written.Size
		p.files[relative] = source
		p.clearChanged(relative)
		p.panel.SavedFile(relative, text)
		kit.Toast(c, kit.Done, i18n.T(c, "workspace_saved"))
	}, func(c *ui.Context, err error) {
		if fileSaveConflict(err) {
			p.saveConflict(relative)
		}
	})
}

// fileSaveConflict reports a save the Node refused because the file no longer
// holds the content the draft began from, or is gone.
func fileSaveConflict(err error) bool {
	code := faultCode(err)
	return code == protocol.ErrorCodeRevisionConflict || code == protocol.ErrorCodeNotFound
}

// saveConflict asks what to do with a draft the Node refused to save. It
// reads the file first, so the choice is about what is on disk now: overwrite
// it with the draft, reload it and drop the draft, or keep editing.
func (p *workspaceProject) saveConflict(relative string) {
	p.checkDraft("conflict", relative, func(c *ui.Context, err error) {
		disk, changed := p.changedFiles[relative]
		if err != nil {
			p.runtime.reportKeys(c, err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
			return
		}
		if !changed {
			p.workspaceError(c, protocol.ErrorCodeRevisionConflict, "file changed before saving")
			return
		}
		var overwrite func(*ui.Context)
		if disk.missing || disk.content.Revision != nil && !disk.content.Truncated {
			overwrite = func(*ui.Context) {
				text, _ := p.panel.FileDraft(relative)
				p.writeFile(relative, text, disk.content.Revision)
			}
		}
		p.panel.FileConflict(relative, overwrite, func(*ui.Context) {
			p.panel.DropFileDraft(relative)
			p.settleChanged(relative)
		})
	})
}
