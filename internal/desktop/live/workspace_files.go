package live

import (
	"encoding/json"
	"path"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (p *workspaceProject) workspaceError(c *ui.Context, code protocol.ErrorCode, message string) {
	p.runtime.reportKeys(c, protocol.Fault{Code: code, Message: message}, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
}
func (p *workspaceProject) mutate(command protocol.Command, done func(*ui.Context, protocol.Output), failed ...func(*ui.Context, error)) {
	encoded, _ := json.Marshal(command)
	key := string(encoded)
	scope := "workspace-action/" + p.owner.ID.String()
	// A Git action that is sent, not one already in flight, counts as running.
	git := command.Kind == "run_git_action" && !p.runtime.busy[scope+"\x00"+key]
	if git {
		p.gitRunning++
	}
	p.runtime.executeScoped(scope, command, func(c *ui.Context, out protocol.Output, err error) {
		if git {
			p.gitRunning--
		}
		unknown := faultCode(err) == protocol.ErrorCodeOutcomeUnknown
		p.panel.SetPromptUncertain(unknown)
		if unknown {
			if p.uncertainOperations == nil {
				p.uncertainOperations = map[string]protocol.Command{}
			}
			p.uncertainOperations[key] = command
			if p.uncertainCallbacks == nil {
				p.uncertainCallbacks = map[string]func(*ui.Context, protocol.Output){}
			}
			if _, exists := p.uncertainCallbacks[key]; !exists {
				p.uncertainCallbacks[key] = done
			}
		} else {
			delete(p.uncertainOperations, key)
			delete(p.uncertainCallbacks, key)
		}
		if err != nil {
			p.runtime.reportKeys(c, err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
			for _, failure := range failed {
				failure(c, err)
			}
			if done != nil {
				done(c, protocol.Output{})
			}
			return
		}
		p.pendingFiles, p.pendingGit = true, true
		if done != nil {
			done(c, out)
		} else {
			kit.Toast(c, kit.Done, i18n.T(c, "workspace_completed"))
		}
	})
}
func (p *workspaceProject) saveFile(c *ui.Context, relative, text string) {
	source, exists := p.files[relative]
	if !exists || source.Revision == nil || source.Truncated {
		p.panel.SetFileStatus(relative, false, source.Truncated)
		p.workspaceError(c, protocol.ErrorCodeRevisionConflict, "complete file source is not loaded")
		return
	}
	p.panel.SetFileStatus(relative, true, source.Truncated)
	p.mutate(protocol.Command{Kind: "write_file", Data: protocol.CommandWriteFilePayload{Worktree: p.owner.ID, Path: relative, Text: text, ExpectedRevision: source.Revision}}, func(c *ui.Context, out protocol.Output) {
		p.panel.SetFileStatus(relative, false, source.Truncated)
		written, ok := detail[protocol.FileWritten](out.Data)
		if !ok {
			return
		}
		source.Text, source.Revision, source.Size = text, &written.Revision, written.Size
		p.files[relative] = source
		p.panel.SavedFile(relative, text)
		kit.Toast(c, kit.Done, i18n.T(c, "workspace_saved"))
	})
}
func (p *workspaceProject) fileAction(c *ui.Context, a panel.FileAction) {
	native := p.runtime.workspace.native
	switch a.Kind {
	case "files_copy", "files_cut":
		p.runtime.workspace.clipboard = workspaceClipboard{worktree: p.owner.ID, paths: append([]string(nil), a.Paths...), cut: a.Kind == "files_cut"}
	case "files_paste":
		copied := p.runtime.workspace.clipboard
		commands := make([]protocol.Command, 0, len(copied.paths))
		for _, relative := range copied.paths {
			destination := path.Join(a.Directory, path.Base(relative))
			if copied.cut {
				commands = append(commands, protocol.Command{Kind: "move_entry_to", Data: protocol.CommandMoveEntryToPayload{Source: copied.worktree, Worktree: p.owner.ID, From: relative, To: destination}})
			} else {
				commands = append(commands, protocol.Command{Kind: "copy_entry_to", Data: protocol.CommandCopyEntryToPayload{Source: copied.worktree, Worktree: p.owner.ID, From: relative, To: destination}})
			}
		}
		p.startWorkspaceTransfer(c, commands, func() {
			if copied.cut {
				current := p.runtime.workspace.clipboard
				if current.worktree == copied.worktree && strings.Join(current.paths, "\x00") == strings.Join(copied.paths, "\x00") {
					p.runtime.workspace.clipboard = workspaceClipboard{}
				}
			}
		})
	case "files_move":
		commands := []protocol.Command{}
		for _, relative := range a.Paths {
			commands = append(commands, protocol.Command{Kind: "move_entry_to", Data: protocol.CommandMoveEntryToPayload{Source: p.owner.ID, Worktree: p.owner.ID, From: relative, To: path.Join(a.Directory, path.Base(relative))}})
		}
		p.startWorkspaceTransfer(c, commands, nil)
	case "files_new_file", "files_new_directory", "files_rename":
		initial := ""
		if a.Kind == "files_rename" && len(a.Paths) == 1 {
			initial = path.Base(a.Paths[0])
		}
		p.panel.Prompt(a.Kind, []string{"settings_name"}, []string{initial}, func(values []string, finish func(bool)) {
			name := values[0]
			if name == "" || name == "." || name == ".." || strings.ContainsAny(name, "/\\\x00") {
				p.runtime.enqueue(func(c *ui.Context) {
					p.workspaceError(c, protocol.ErrorCodeInvalidRequest, "entry name must be a single nonempty name")
				})
				finish(false)
				return
			}
			var command protocol.Command
			switch a.Kind {
			case "files_new_file":
				command = protocol.Command{Kind: "write_file", Data: protocol.CommandWriteFilePayload{Worktree: p.owner.ID, Path: path.Join(a.Directory, name), Text: ""}}
			case "files_new_directory":
				command = protocol.Command{Kind: "create_directory", Data: protocol.CommandCreateDirectoryPayload{Worktree: p.owner.ID, Path: path.Join(a.Directory, name)}}
			default:
				if len(a.Paths) != 1 {
					finish(false)
					return
				}
				command = protocol.Command{Kind: "rename_entry", Data: protocol.CommandRenameEntryPayload{Worktree: p.owner.ID, From: a.Paths[0], To: path.Join(path.Dir(a.Paths[0]), name)}}
			}
			p.mutate(command, func(c *ui.Context, out protocol.Output) {
				finish(out.Kind != "")
				if out.Kind != "" {
					if a.Kind == "files_new_file" {
						if written, ok := detail[protocol.FileWritten](out.Data); ok {
							p.panel.FileOpen(written.Path)
							p.runtime.App.PanelOpen = true
							p.runtime.App.Panel.Show(panel.Tab{Kind: panel.Files, ID: written.Path})
						}
					}
					kit.Toast(c, kit.Done, i18n.T(c, "workspace_completed"))
				}
			})
		})
	case "files_trash":
		selected := append([]string(nil), a.Paths...)
		// Every selected entry exists when its confirmation opens, so each confirmation has its own place.
		batch := &fileBatch{}
		p.panel.Confirm("files_trash", strings.Join(selected, "\n"), func(finish func(bool)) {
			commands := []protocol.Command{}
			for _, relative := range selected {
				commands = append(commands, protocol.Command{Kind: "trash_entry", Data: protocol.CommandTrashEntryPayload{Worktree: p.owner.ID, Path: relative}})
			}
			p.fileCommandsFinish(batch, commands, finish, nil)
		})
	case "files_insert_conversation":
		if native.Insert == nil {
			p.workspaceError(c, protocol.ErrorCodeUnavailable, "native file action is unavailable")
			return
		}
		native.Insert(p.owner.ID, append([]string(nil), a.Paths...))
	case "files_open_system", "files_download":
		effect := native.Open
		if a.Kind == "files_download" {
			effect = native.Download
		}
		if effect == nil {
			p.workspaceError(c, protocol.ErrorCodeUnavailable, "native file action is unavailable")
			return
		}
		for _, relative := range a.Paths {
			var revision *string
			if source, ok := p.files[relative]; ok {
				revision = source.Revision
			}
			effect(p.owner.ID, relative, revision)
		}
	case "files_reveal_manager", "files_reveal_finder", "files_reveal_explorer":
		if native.Reveal == nil {
			p.workspaceError(c, protocol.ErrorCodeUnavailable, "native reveal is unavailable")
			return
		}
		for _, relative := range a.Paths {
			native.Reveal(p.owner.ID, relative)
		}
	default:
		p.workspaceError(c, protocol.ErrorCodeInvalidRequest, "unknown file action")
	}
}

// fileBatch is how far a sequential batch got. Its entries cannot be applied
// twice, so a retry continues after the confirmed ones.
type fileBatch struct{ confirmed int }

func (p *workspaceProject) fileCommandsFinish(batch *fileBatch, commands []protocol.Command, finish func(bool), done func()) {
	var next func(int, *ui.Context)
	next = func(index int, c *ui.Context) {
		if index == len(commands) {
			batch.confirmed = 0
			if finish != nil {
				finish(true)
			}
			if done != nil {
				done()
			}
			if c != nil {
				kit.Toast(c, kit.Done, i18n.T(c, "workspace_completed"))
			}
			return
		}
		p.mutate(commands[index], func(c *ui.Context, out protocol.Output) {
			if out.Kind == "" {
				if finish != nil {
					finish(false)
				}
				return
			}
			batch.confirmed = index + 1
			next(index+1, c)
		})
	}
	next(batch.confirmed, nil)
}
