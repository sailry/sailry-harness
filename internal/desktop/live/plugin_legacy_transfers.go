package live

import (
	"context"
	"encoding/json"
	"path"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const maxLegacyTransfers = 128

type legacyClipboardPath struct {
	Path string             `json:"path"`
	Kind protocol.EntryKind `json:"kind"`
}

type legacyClipboard struct {
	ID          string                `json:"id"`
	Cut         bool                  `json:"cut"`
	SourceLabel string                `json:"source_label"`
	Paths       []legacyClipboardPath `json:"paths"`
	CrossNode   bool                  `json:"cross_node"`
	Available   bool                  `json:"available"`
}

type legacyTransferProgress struct {
	Copied uint64 `json:"copied"`
	Size   uint64 `json:"size"`
}

type legacyTransferState struct {
	ID          string                  `json:"id"`
	Kind        string                  `json:"kind"`
	Stage       string                  `json:"stage"`
	Path        string                  `json:"path"`
	SourceLabel string                  `json:"source_label"`
	SourcePath  string                  `json:"source_path"`
	Error       *protocol.Fault         `json:"error"`
	Progress    *legacyTransferProgress `json:"progress"`
	Published   bool                    `json:"published"`
	CanCancel   bool                    `json:"can_cancel"`
	CanStart    bool                    `json:"can_start"`
	CanReplace  bool                    `json:"can_replace"`
	CanCheck    bool                    `json:"can_check"`
}

type legacyTransfer struct {
	value     legacyTransferState
	local     string
	clipboard string
	expected  *string
	request   *protocol.Request
	cancel    context.CancelFunc
	hidden    bool
}

func (host *legacyFileHost) transferOperations() {
	ops := host.state.native.Operations
	ops["sailry/file-transfers.readClipboard"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		if err := host.check(false); err != nil {
			return nil, err
		}
		clipboard := host.workspace.clipboard
		if clipboard == nil || host.check(true) == nil {
			return pluginNativeJSON(clipboard)
		}
		value := *clipboard
		value.Available = false
		return pluginNativeJSON(value)
	}
	ops["sailry/file-transfers.readTransfers"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		if err := host.check(false); err != nil {
			return nil, err
		}
		host.publishTransfers()
		return host.transfers.read(), nil
	}
	ops["sailry/file-transfers.nextTransferChange"] = host.transfers.next
	ops["sailry/file-transfers.captureClipboard"] = host.captureClipboard
	ops["sailry/file-transfers.prepareMove"] = host.prepareEntryTransfer(false)
	ops["sailry/file-transfers.preparePaste"] = host.prepareEntryTransfer(true)
	ops["sailry/file-transfers.transferAction"] = host.transferAction
	ops["sailry/file-transfers.selectUpload"] = host.selectUpload
	ops["sailry/file-transfers.startDownload"] = host.startDownload
	ops["sailry/file-transfers.openSystem"] = host.openSystemFile
}

func (host *legacyFileHost) publishTransfers() {
	workspace := host.workspace
	if host.transferRevision == workspace.transferRevision && host.transfers.changed != nil {
		return
	}
	host.transferRevision = workspace.transferRevision
	values := make([]legacyTransferState, 0, len(workspace.transfers))
	for _, transfer := range workspace.transfers {
		if !transfer.hidden {
			values = append(values, transfer.value)
		}
	}
	host.transfers.publish(func(cursor string) any { return map[string]any{"cursor": cursor, "transfers": values} })
}

func (host *legacyFileHost) captureClipboard(ctx context.Context, args []json.RawMessage) (json.RawMessage, error) {
	names, err := pluginNativeArgument[[]string](args, 0)
	if err != nil || len(names) == 0 || len(names) > maxLegacyTransfers {
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid file selection")
	}
	cut, err := pluginNativeArgument[bool](args, 1)
	if err != nil {
		return nil, err
	}
	for _, name := range names {
		if !legacyFilePath(name) {
			return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid clipboard path")
		}
	}
	if _, err := host.state.update(ctx, func(*ui.Context) (json.RawMessage, error) { return nil, host.check(cut) }); err != nil {
		return nil, err
	}
	selection := map[string][]string{}
	for _, name := range names {
		directory := path.Dir(name)
		selection[directory] = append(selection[directory], name)
	}
	found := map[string]protocol.EntryKind{}
	for directory, wanted := range selection {
		var after *protocol.DirectoryCursor
		remaining := len(wanted)
		for remaining > 0 {
			out, err := host.state.execute(ctx, protocol.Command{Kind: "list_directory", Data: protocol.CommandListDirectoryPayload{Worktree: host.worktree, Path: directory, After: after}})
			if err != nil {
				return nil, err
			}
			page, valid := detail[protocol.Directory](out.Data)
			if !valid || out.Kind != "directory" {
				return nil, pluginNativeFault(protocol.ErrorCodeInternal, "unexpected file listing response")
			}
			for _, entry := range page.Entries {
				if name := path.Join(directory, entry.Name); slices.Contains(wanted, name) {
					found[name] = entry.Kind
					remaining--
				}
			}
			if page.Next == nil {
				break
			}
			after = page.Next
		}
	}
	entries := make([]legacyClipboardPath, 0, len(names))
	for _, name := range names {
		if kind, present := found[name]; present {
			entries = append(entries, legacyClipboardPath{Path: name, Kind: kind})
		} else {
			return nil, pluginNativeFault(protocol.ErrorCodeNotFound, "clipboard entry is no longer available")
		}
	}
	return host.state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
		if err := host.check(cut); err != nil {
			return nil, err
		}
		clipboard := &legacyClipboard{ID: protocol.NewRequestID().String(), Cut: cut, Paths: entries, SourceLabel: host.transferSourceLabel(), Available: host.check(true) == nil}
		host.workspace.clipboard = clipboard
		return pluginNativeJSON(clipboard)
	})
}

func (host *legacyFileHost) prepareEntryTransfer(paste bool) func(context.Context, []json.RawMessage) (json.RawMessage, error) {
	return func(ctx context.Context, args []json.RawMessage) (json.RawMessage, error) {
		var clipboardID string
		if paste {
			var err error
			clipboardID, err = pluginNativeArgument[string](args, 0)
			if err != nil {
				return nil, err
			}
			args = args[1:]
		}
		source, err := pluginNativeArgument[string](args, 0)
		if err != nil || !legacyFilePath(source) {
			return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid transfer source")
		}
		destination, err := pluginNativeArgument[string](args, 1)
		if err != nil || !legacyFilePath(destination) || legacyPathWithin(destination, source) {
			return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid transfer destination")
		}
		return host.state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
			if err := host.check(true); err != nil {
				return nil, err
			}
			kind := "move"
			if paste {
				clipboard := host.workspace.clipboard
				if clipboard == nil || clipboard.ID != clipboardID || !slices.ContainsFunc(clipboard.Paths, func(entry legacyClipboardPath) bool { return entry.Path == source }) {
					return nil, pluginNativeFault(protocol.ErrorCodeNotFound, "clipboard entry is outside the captured workspace")
				}
				if !clipboard.Cut {
					kind = "copy"
				}
			}
			for _, existing := range host.workspace.transfers {
				value := existing.value
				if value.Kind == kind && value.SourcePath == source && value.Path == destination && (value.Stage == "ready" || value.Stage == "publishing" || value.Stage == "uncertain") {
					existing.hidden = false
					host.transferChanged()
					return pluginNativeJSON(value)
				}
			}
			transfer, err := host.newTransfer(kind, source, destination, "")
			if err != nil {
				return nil, err
			}
			transfer.clipboard = clipboardID
			return pluginNativeJSON(transfer.value)
		})
	}
}

func legacyPathWithin(name, directory string) bool {
	return name == directory || strings.HasPrefix(name, directory+"/")
}

func (host *legacyFileHost) transferSourceLabel() string {
	return path.Base(strings.ReplaceAll(host.project.owner.Path, "\\", "/"))
}

func (host *legacyFileHost) newTransfer(kind, source, destination, local string) (*legacyTransfer, error) {
	workspace := host.workspace
	if len(workspace.transfers) >= maxLegacyTransfers {
		index := slices.IndexFunc(workspace.transfers, func(job *legacyTransfer) bool {
			return job.request == nil && (job.value.Stage == "done" || job.value.Stage == "cancelled" || job.value.Stage == "failed" || job.value.Stage == "uncertain")
		})
		if index < 0 {
			return nil, pluginNativeFault(protocol.ErrorCodeBusy, "too many active file transfers")
		}
		workspace.transfers = slices.Delete(workspace.transfers, index, index+1)
	}
	transfer := &legacyTransfer{local: local, value: legacyTransferState{ID: protocol.NewRequestID().String(), Kind: kind, Path: destination, SourcePath: source, SourceLabel: host.transferSourceLabel()}}
	transfer.stage("ready", nil)
	workspace.transfers = append(workspace.transfers, transfer)
	host.transferChanged()
	return transfer, nil
}

func (transfer *legacyTransfer) stage(stage string, err error) {
	v := &transfer.value
	v.Stage, v.Error = stage, legacyFileFault(err)
	v.CanStart = stage == "ready"
	v.CanReplace = stage == "exists" && transfer.expected != nil
	v.CanCheck = stage == "uncertain" && transfer.request != nil
	v.CanCancel = stage == "ready" || stage == "exists" || transfer.cancel != nil && (stage == "preparing" || stage == "transferring")
	if stage == "done" {
		v.Published = true
	}
}

func (host *legacyFileHost) transferChanged() {
	host.workspace.transferRevision++
	host.state.runtime.localRuntime().wake()
}

func (host *legacyFileHost) transferAction(ctx context.Context, args []json.RawMessage) (json.RawMessage, error) {
	id, err := pluginNativeArgument[string](args, 0)
	if err != nil {
		return nil, err
	}
	action, err := pluginNativeArgument[struct {
		Kind string
		Path *string
	}](args, 1)
	if err != nil {
		return nil, err
	}
	return host.state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
		if err := host.check(false); err != nil {
			return nil, err
		}
		workspace := host.workspace
		index := slices.IndexFunc(workspace.transfers, func(job *legacyTransfer) bool { return job.value.ID == id && !job.hidden })
		if index < 0 {
			return nil, pluginNativeFault(protocol.ErrorCodeNotFound, "transfer is outside the captured workspace")
		}
		job := workspace.transfers[index]
		v := &job.value
		switch action.Kind {
		case "dismiss":
			switch v.Stage {
			case "ready", "exists":
				return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "cancel the prepared transfer before closing it")
			case "failed", "done", "cancelled":
				workspace.transfers = slices.Delete(workspace.transfers, index, index+1)
			default:
				job.hidden = true
			}
			host.transferChanged()
			return json.RawMessage("null"), nil
		case "cancel":
			if !v.CanCancel {
				return nil, pluginNativeFault(protocol.ErrorCodeBusy, "the transfer cannot be cancelled at this stage")
			}
			if job.cancel != nil {
				job.cancel()
				v.CanCancel = false
			} else {
				job.local = ""
				job.stage("cancelled", nil)
			}
		case "start", "replace", "check":
			if err := host.check(v.Kind != "download"); err != nil {
				return nil, err
			}
			if action.Kind == "start" && !v.CanStart || action.Kind == "replace" && !v.CanReplace || action.Kind == "check" && !v.CanCheck {
				return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "transfer action is unavailable at this stage")
			}
			if action.Path != nil {
				if action.Kind != "start" || v.Kind != "copy" && v.Kind != "move" || !legacyFilePath(*action.Path) || legacyPathWithin(*action.Path, v.SourcePath) {
					return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid transfer destination")
				}
				v.Path = *action.Path
			}
			if err := host.beginTransfer(job); err != nil {
				return nil, err
			}
		default:
			return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "unknown file transfer action")
		}
		host.transferChanged()
		return pluginNativeJSON(*v)
	})
}

func (host *legacyFileHost) beginTransfer(job *legacyTransfer) error {
	v := job.value
	if job.request != nil {
		return host.publishTransfer(job, *job.request)
	}
	for _, unsaved := range host.project.panel.Unsaved() {
		if (v.Kind == "copy" || v.Kind == "move" || v.Kind == "upload") && legacyPathWithin(unsaved, v.Path) || v.Kind == "move" && legacyPathWithin(unsaved, v.SourcePath) {
			return pluginNativeFault(protocol.ErrorCodeBusy, "the transfer would affect an unsaved document")
		}
	}
	switch v.Kind {
	case "copy":
		return host.publishTransfer(job, host.request(protocol.Command{Kind: "copy_entry_to", Data: protocol.CommandCopyEntryToPayload{Source: host.worktree, Worktree: host.worktree, From: v.SourcePath, To: v.Path}}))
	case "move":
		return host.publishTransfer(job, host.request(protocol.Command{Kind: "move_entry_to", Data: protocol.CommandMoveEntryToPayload{Source: host.worktree, Worktree: host.worktree, From: v.SourcePath, To: v.Path}}))
	case "upload":
		return host.runUpload(job)
	case "download":
		return host.runDownload(job)
	}
	return pluginNativeFault(protocol.ErrorCodeInvalidRequest, "unknown file transfer kind")
}

func (host *legacyFileHost) publishTransfer(job *legacyTransfer, request protocol.Request) error {
	r := host.state.runtime
	checking := job.request != nil
	job.request = &request
	job.stage("publishing", nil)
	host.transferChanged()
	if !r.async(func() {
		var out protocol.Output
		var err error
		if checking {
			out, err = r.retryAttachmentRequest(r.ctx, request)
		} else {
			out, err = r.controller.Settle(r.ctx, request)
		}
		unexpected := err == nil && (request.Command.Kind == "copy_entry_to" && out.Kind != "entry_copied" || request.Command.Kind == "move_entry_to" && out.Kind != "entry_moved" || request.Command.Kind == "finish_file_upload" && out.Kind != "file_written")
		if unexpected {
			err = pluginNativeFault(protocol.ErrorCodeOutcomeUnknown, "unexpected file transfer result")
		}
		r.enqueue(func(*ui.Context) {
			stage := "done"
			if client.Retained(err) || faultCode(err) == protocol.ErrorCodeOutcomeUnknown || checking && err != nil && !client.Released(err) && faultCode(err) == protocol.ErrorCodeUnavailable {
				stage = "uncertain"
				if unexpected || client.Released(err) {
					job.request, job.local = nil, ""
				}
			} else {
				job.request, job.local = nil, ""
				if err != nil {
					stage = "failed"
				}
			}
			if stage == "done" {
				host.project.pendingFiles, host.project.pendingGit = true, true
				if clipboard := host.workspace.clipboard; clipboard != nil && clipboard.ID == job.clipboard && clipboard.Cut {
					clipboard.Paths = slices.DeleteFunc(clipboard.Paths, func(entry legacyClipboardPath) bool { return entry.Path == job.value.SourcePath })
					if len(clipboard.Paths) == 0 {
						host.workspace.clipboard = nil
					}
				}
			}
			job.stage(stage, err)
			host.transferChanged()
		})
	}) {
		job.stage("uncertain", resourceOfflineFault())
		host.transferChanged()
		return resourceOfflineFault()
	}
	return nil
}
