package live

import (
	"encoding/json"
	"path"
	"sort"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/transfers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type workspaceTransfer struct {
	project  *workspaceProject
	key      string
	commands []protocol.Command
	index    int
	active   protocol.Command
	view     transfers.EntryTransfer
	stop     bool
	finish   func()
}

func (r *Runtime) workspaceTransferOpen() bool {
	return r.workspace != nil && r.workspace.transfer != nil && r.workspace.transfer.view.Open
}
func (r *Runtime) workspaceTransferDialog(c *ui.Context) {
	if !r.workspaceTransferOpen() {
		return
	}
	transfer := r.workspace.transfer
	transfers.EntryTransferDialog(c, &transfer.view, transfers.EntryTransferActions{Start: func(destination string) { transfer.start(destination, false) }, Check: func() { transfer.start(transfer.view.Destination, true) }, Cancel: func() { transfer.cancel(c) }, Close: func() { transfer.stop = true; transfer.view.Open = false }})
}
func (p *workspaceProject) startWorkspaceTransfer(c *ui.Context, commands []protocol.Command, finish func()) {
	if len(commands) == 0 {
		return
	}
	if p.runtime.transferModalBusy(true) {
		p.workspaceError(c, protocol.ErrorCodeBusy, "another file transfer dialog is open")
		return
	}
	encoded, err := json.Marshal(commands)
	if err != nil {
		p.workspaceError(c, protocol.ErrorCodeInvalidRequest, "file transfer command is invalid")
		return
	}
	key := string(encoded)
	state := p.runtime.workspace
	if existing := state.transfers[key]; existing != nil {
		existing.view.Open = true
		state.transfer = existing
		return
	}
	if state.transfer != nil && state.transfer.view.Status == transfers.EntryRunning {
		p.workspaceError(c, protocol.ErrorCodeBusy, "another file transfer is active")
		return
	}
	if state.transfers == nil {
		state.transfers = map[string]*workspaceTransfer{}
	}
	transfer := &workspaceTransfer{project: p, key: key, commands: append([]protocol.Command(nil), commands...), finish: finish}
	state.transfers[key] = transfer
	state.transfer = transfer
	transfer.prepare()
}
func workspaceTransferEntry(command protocol.Command) (source protocol.WorktreeID, from, to string, move bool) {
	switch command.Kind {
	case "copy_entry_to":
		v, _ := detail[protocol.CommandCopyEntryToPayload](command.Data)
		return v.Source, v.From, v.To, false
	case "move_entry_to":
		v, _ := detail[protocol.CommandMoveEntryToPayload](command.Data)
		return v.Source, v.From, v.To, true
	}
	return
}
func workspaceTransferDestination(command protocol.Command, destination string) protocol.Command {
	switch command.Kind {
	case "copy_entry_to":
		v, _ := detail[protocol.CommandCopyEntryToPayload](command.Data)
		v.To = destination
		command.Data = v
	case "move_entry_to":
		v, _ := detail[protocol.CommandMoveEntryToPayload](command.Data)
		v.To = destination
		command.Data = v
	}
	return command
}
func (t *workspaceTransfer) prepare() {
	source, from, to, move := workspaceTransferEntry(t.commands[t.index])
	label := source.String()
	for _, tree := range t.project.runtime.snapshot().Worktrees {
		if tree.ID == source {
			label = path.Base(tree.Path)
			break
		}
	}
	remaining := []string{}
	for _, command := range t.commands[t.index+1:] {
		_, relative, _, _ := workspaceTransferEntry(command)
		remaining = append(remaining, relative)
	}
	t.view = transfers.EntryTransfer{Open: true, Move: move, SourceLabel: label, SourcePath: from, Destination: to, Remaining: remaining, Status: transfers.EntryReady}
	t.stop = false
}
func (t *workspaceTransfer) start(destination string, check bool) {
	if t.view.Status == transfers.EntryRunning || t.index >= len(t.commands) {
		return
	}
	if t.view.Status == transfers.EntryUncertain && !check {
		return
	}
	if !check {
		t.active = workspaceTransferDestination(t.commands[t.index], destination)
		t.view.Destination = destination
	}
	t.view.Status = transfers.EntryRunning
	t.view.Canceling = false
	if !check {
		t.stop = false
	}
	t.project.runtime.executeScoped("workspace-action/"+t.project.owner.ID.String(), t.active, func(c *ui.Context, out protocol.Output, err error) {
		if err != nil {
			t.view.Status = transfers.EntryFailed
			if client.Retained(err) {
				t.view.Status = transfers.EntryUncertain
			}
			t.project.runtime.reportKeys(c, err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
			return
		}
		t.project.pendingFiles, t.project.pendingGit = true, true
		t.index++
		if t.index == len(t.commands) {
			t.view.Status = transfers.EntryDone
			t.view.Remaining = nil
			if t.finish != nil {
				t.finish()
			}
			delete(t.project.runtime.workspace.transfers, t.key)
			kit.Toast(c, kit.Done, i18n.T(c, "workspace_completed"))
			return
		}
		if t.stop {
			open := t.view.Open
			t.prepare()
			t.view.Open = open
			t.view.Status = transfers.EntryCancelled
			t.view.Canceling = false
			kit.Toast(c, kit.Done, i18n.T(c, "files_paste_cancelled"))
			return
		}
		t.prepare()
	})
}
func (t *workspaceTransfer) cancel(c *ui.Context) {
	t.stop = true
	t.view.Open = false
	if t.view.Status == transfers.EntryRunning {
		t.view.Canceling = true
		return
	}
	t.view.Status = transfers.EntryCancelled
	kit.Toast(c, kit.Done, i18n.T(c, "files_paste_cancelled"))
}

func workspaceOperationPath(command protocol.Command) string {
	switch command.Kind {
	case "write_file":
		v, _ := detail[protocol.CommandWriteFilePayload](command.Data)
		return v.Path
	case "create_directory":
		v, _ := detail[protocol.CommandCreateDirectoryPayload](command.Data)
		return v.Path
	case "rename_entry":
		v, _ := detail[protocol.CommandRenameEntryPayload](command.Data)
		return v.From
	case "trash_entry":
		v, _ := detail[protocol.CommandTrashEntryPayload](command.Data)
		return v.Path
	}
	return ""
}
func (p *workspaceProject) pendingWorkspaceOperations() []panel.PendingOperation {
	result := []panel.PendingOperation{}
	result = append(result, p.pendingWorkspaceUploads()...)
	for _, transfer := range p.runtime.workspace.transfers {
		if transfer.project != p || transfer.view.Status != transfers.EntryUncertain {
			continue
		}
		result = append(result, panel.PendingOperation{Label: transfer.view.Destination, Run: func(c *ui.Context) {
			if p.runtime.transferModalBusy(true) {
				p.workspaceError(c, protocol.ErrorCodeBusy, "another file transfer dialog is open")
				return
			}
			p.runtime.workspace.transfer = transfer
			transfer.view.Open = true
		}})
	}
	for key, command := range p.uncertainOperations {
		settled := p.uncertainCallbacks[key]
		relative := workspaceOperationPath(command)
		if relative == "" {
			continue
		}
		result = append(result, panel.PendingOperation{Label: relative, Run: func(c *ui.Context) {
			p.panel.Confirm("files_check_result", relative, func(done func(bool)) {
				p.mutateNamed(workspaceAction, command, func(c *ui.Context, out protocol.Output) {
					if settled != nil {
						settled(c, out)
					}
					done(out.Kind != "")
				})
			})
		}})
	}
	sort.Slice(result, func(i, j int) bool { return result[i].Label < result[j].Label })
	return result
}
