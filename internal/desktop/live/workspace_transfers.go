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
	request  protocol.Request
	view     transfers.EntryTransfer
	stop     bool
	applied  func(protocol.Command)
}

func (r *Runtime) workspaceTransferOpen() bool {
	return r.workspace != nil && r.workspace.transfer != nil && r.workspace.transfer.view.Open
}
func (r *Runtime) workspaceTransferDialog(c *ui.Context) {
	if !r.workspaceTransferOpen() {
		return
	}
	transfer := r.workspace.transfer
	transfers.EntryTransferDialog(c, &transfer.view, transfers.EntryTransferActions{Start: func(destination string) { transfer.start(c, destination) }, Check: transfer.check, Cancel: func() { transfer.cancel(c) }, Close: transfer.dismiss})
}
func (p *workspaceProject) startWorkspaceTransfer(c *ui.Context, commands []protocol.Command, applied func(protocol.Command)) {
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
	existing := state.transfers[key]
	if state.busyTransfer(existing) {
		p.workspaceError(c, protocol.ErrorCodeBusy, "another file transfer is active")
		return
	}
	if existing != nil {
		existing.view.Open = true
		state.transfer = existing
		return
	}
	if state.transfers == nil {
		state.transfers = map[string]*workspaceTransfer{}
	}
	transfer := &workspaceTransfer{project: p, key: key, commands: append([]protocol.Command(nil), commands...), applied: applied}
	state.transfers[key] = transfer
	state.transfer = transfer
	transfer.prepare()
}

func (state *workspaceState) busyTransfer(next *workspaceTransfer) bool {
	active := state.transfer
	return active != nil && active != next && (active.view.Open || active.view.Status == transfers.EntryRunning)
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
func (t *workspaceTransfer) start(c *ui.Context, destination string) {
	if t.view.Status != transfers.EntryReady && t.view.Status != transfers.EntryFailed || t.index >= len(t.commands) {
		return
	}
	p, command := t.project, t.commands[t.index]
	t.view.Destination = destination
	if !validWorkspaceRelative(destination) {
		p.workspaceError(c, protocol.ErrorCodeInvalidRequest, "invalid workspace transfer destination")
		return
	}
	source, from, _, move := workspaceTransferEntry(command)
	if transferOverlapsDraft(p, destination) || move && transferOverlapsDraft(p.runtime.workspace.projects[source], from) {
		p.workspaceError(c, protocol.ErrorCodeConflict, "the transfer overlaps an unsaved file")
		return
	}
	t.active = workspaceTransferDestination(command, destination)
	t.request = p.runtime.controller.Prepare(t.active)
	t.stop = false
	t.run(false)
}

func transferOverlapsDraft(project *workspaceProject, path string) bool {
	if project == nil {
		return false
	}
	for _, draft := range project.panel.Unsaved() {
		if workspaceUploadPathsOverlap(path, draft) {
			return true
		}
	}
	return false
}

func (t *workspaceTransfer) check() {
	if t.view.Status == transfers.EntryUncertain {
		t.run(true)
	}
}

func (t *workspaceTransfer) run(check bool) {
	r := t.project.runtime
	t.view.Status, t.view.Canceling = transfers.EntryRunning, false
	request := t.request
	if !r.async(func() {
		var out protocol.Output
		var err error
		inspectionFailed := false
		if check {
			var state client.Resolution
			state, err = r.controller.Resolve(r.ctx, request)
			if err == nil {
				out, err = state.Result()
			} else {
				inspectionFailed = true
			}
		} else {
			out, err = r.controller.Settle(r.ctx, request)
		}
		r.enqueue(func(c *ui.Context) { t.complete(c, out, err, inspectionFailed) })
	}) {
		t.view.Status = transfers.EntryFailed
	}
}

func (t *workspaceTransfer) complete(c *ui.Context, out protocol.Output, err error, inspectionFailed bool) {
	source, from, to, move := workspaceTransferEntry(t.active)
	if err == nil {
		var valid bool
		if move {
			output, decoded := detail[protocol.OutputEntryMovedPayload](out.Data)
			valid = out.Kind == "entry_moved" && decoded && output.From == from && output.To == to
		} else {
			output, decoded := detail[protocol.OutputEntryCopiedPayload](out.Data)
			valid = out.Kind == "entry_copied" && decoded && output.From == from && output.To == to
		}
		if !valid {
			err = fileActionFault(protocol.ErrorCodeOutcomeUnknown, "the transfer result could not be confirmed")
		}
	}
	t.view.Canceling = false
	if err != nil {
		t.view.Status = transfers.EntryFailed
		if inspectionFailed || faultCode(err) == protocol.ErrorCodeOutcomeUnknown {
			t.view.Status = transfers.EntryUncertain
		} else if t.stop {
			t.retire()
		}
		t.project.runtime.reportKeys(c, err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
		return
	}
	t.project.pendingFiles, t.project.pendingGit = true, true
	if move {
		if project := t.project.runtime.workspace.projects[source]; project != nil {
			project.pendingFiles, project.pendingGit = true, true
		}
	}
	if t.applied != nil {
		t.applied(t.active)
	}
	t.index++
	if t.index == len(t.commands) {
		t.view.Status = transfers.EntryDone
		t.view.Remaining = nil
		delete(t.project.runtime.workspace.transfers, t.key)
		kit.Toast(c, kit.Done, i18n.T(c, "workspace_completed"))
		return
	}
	if t.stop {
		t.view.Status = transfers.EntryCancelled
		t.retire()
		kit.Toast(c, kit.Done, i18n.T(c, "files_paste_cancelled"))
		return
	}
	t.prepare()
}
func (t *workspaceTransfer) cancel(c *ui.Context) {
	t.dismiss()
	if t.view.Status != transfers.EntryRunning {
		t.view.Status = transfers.EntryCancelled
		kit.Toast(c, kit.Done, i18n.T(c, "files_paste_cancelled"))
	}
}

func (t *workspaceTransfer) dismiss() {
	t.view.Open = false
	switch t.view.Status {
	case transfers.EntryRunning:
		t.stop = true
		t.view.Canceling = true
	case transfers.EntryReady, transfers.EntryCancelled, transfers.EntryDone, transfers.EntryFailed:
		t.retire()
	}
}

func (t *workspaceTransfer) retire() {
	state := t.project.runtime.workspace
	delete(state.transfers, t.key)
	if state.transfer == t {
		state.transfer = nil
	}
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
			state := p.runtime.workspace
			if p.runtime.transferModalBusy(true) || state.busyTransfer(transfer) {
				p.workspaceError(c, protocol.ErrorCodeBusy, "another file transfer dialog is open")
				return
			}
			state.transfer = transfer
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
