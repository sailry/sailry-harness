package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type workspaceCompletion struct {
	external bool
	commit   func(context.Context, *sql.Conn, protocol.Output) (protocol.Output, []protocol.Event, error)
	release  func()
	after    func()
}

func (effect *workspaceCompletion) close() {
	if effect.after != nil {
		effect.after()
	}
	if effect.release != nil {
		effect.release()
	}
}

func workspaceGitRead(kind string) bool {
	switch kind {
	case "inspect_git", "read_git_head", "read_git_diff", "read_git_log", "read_git_commit", "read_git_show", "read_git_blame", "list_git_branches", "resolve_git_revision", "read_git_stash", "read_git_output", "list_git_remote_tags", "list_git_changes", "read_git_change", "read_git_context", "list_worktrees":
		return true
	}
	return false
}
func workspaceGitWrite(kind string) bool {
	switch kind {
	case "apply_git_hunk", "update_git_index", "create_git_commit", "create_git_branch", "rename_git_branch", "delete_git_branch", "switch_git_branch", "merge_git_branch", "run_git_action", "create_worktree", "create_managed_worktree", "remove_worktree":
		return true
	}
	return false
}
func workspaceCommand(kind string) bool {
	if terminalCommand(kind) {
		return true
	}
	if networkCommand(kind) || codeIndexCommand(kind) {
		return true
	}
	if files.SupportsRead(kind) || files.SupportsMutation(kind) || store.SupportsFilesCommand(kind) || workspaceGitRead(kind) || workspaceGitWrite(kind) {
		return true
	}
	switch kind {
	case "read_project_catalog", "read_worktree_catalog":
		return true
	case "fetch_web", "cancel_web_fetch", "search_web", "cancel_web_search", "test_search_driver", "read_search_settings", "save_search_settings", "write_artifact", "upload_file", "upload_attachment", "download_file", "download_attachment", "download_image", "finish_file_upload", "cancel_file_transfer", "restore_file_checkpoint", "apply_turn_hunk", "read_hooks", "save_hooks", "read_sandbox_settings", "save_sandbox_settings", "run_command", "list_commands", "read_command", "read_command_page", "write_command", "stop_command", "stop_commands":
		return true
	}
	return false
}
func fileTextMutation(kind string) bool {
	switch kind {
	case "write_file", "edit_file", "edit_notebook", "delete_file", "move_file", "apply_changes":
		return true
	}
	return false
}
func (node *Node) workspaceRoot(ctx context.Context, id protocol.WorktreeID, internal bool) (string, error) {
	var root string
	owned := false
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		root, err = store.ReadWorkspaceRoot(ctx, db, id, internal)
		if err != nil {
			return err
		}
		profile := node.profile.Ownership.Path
		if session, err := protocol.ParseSessionID(filepath.Base(root)); err == nil && root == core.SessionWorkspace(profile, session) {
			return db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND worktree=?2 AND project IS NULL)", session.String(), id.String()).Scan(&owned)
		}
		if owner := filepath.Dir(filepath.Dir(root)); (owner == filepath.Join(profile, "assistants") ||
			filepath.Dir(owner) == filepath.Join(profile, "connections")) && core.ManagedWorkspace(root, profile) {
			// Every conversation of one plugin assistant or saved connection shares this scratch root.
			return db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM sessions WHERE worktree=?1 AND project IS NULL)", id.String()).Scan(&owned)
		}
		return nil
	})
	if err != nil || !owned {
		return root, err
	}
	profile, err := core.OpenFiles(node.profile.Ownership.Path, core.ManualFiles)
	if err != nil {
		return "", err
	}
	defer profile.Close()
	relative, err := filepath.Rel(node.profile.Ownership.Path, root)
	if err != nil {
		return "", err
	}
	if err = profile.CreatePrivateDirectories(filepath.ToSlash(relative)); err != nil {
		return "", err
	}
	return root, nil
}
func (node *Node) workspaceRoots(ctx context.Context, command protocol.Command, internal bool) (source, root string, err error) {
	if id, ok := commandWorktree(command); ok {
		root, err = node.workspaceRoot(ctx, id, internal)
		if err != nil {
			return
		}
		source = root
	}
	var id protocol.WorktreeID
	switch input := command.Data.(type) {
	case *protocol.CommandCopyEntryToPayload:
		id = input.Source
	case *protocol.CommandMoveEntryToPayload:
		id = input.Source
	case *protocol.ConversationCheckpointHunkSelection:
		id = input.Worktree
	}
	if id != (protocol.WorktreeID{}) {
		source, err = node.workspaceRoot(ctx, id, internal)
		if root == "" {
			root = source
		}
	}
	return
}

// dispatchWorkspace records durable admission before starting any resource IO.
// Its worker owns Node lifetime; losing the controller never repeats an effect.
func (node *Node) dispatchWorkspace(ctx context.Context, caller protocol.NodeID, request protocol.Request, body []byte) (link.Admission, error) {
	node.mu.Lock()
	defer node.mu.Unlock()
	if node.stopped {
		return link.Admission{}, unavailable()
	}
	if err := ctx.Err(); err != nil {
		return link.Admission{}, nodeFault(err)
	}
	completion := make(chan link.Response, 1)
	result := link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}
	var previous *link.Response
	if result.Receipt.Durable {
		if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			var err error
			previous, err = admission.Admit(ctx, db, caller, request, body)
			return err
		}); err != nil {
			return link.Admission{}, nodeFault(err)
		}
	}
	if previous != nil {
		completion <- *previous
		close(completion)
		return result, nil
	}
	node.work.Add(1)
	go func() {
		defer node.work.Done()
		defer close(completion)
		operationContext := ctx
		if result.Receipt.Durable {
			operationContext = node.ctx
		}
		effect := &workspaceCompletion{}
		output, effectErr := node.workspaceIO(operationContext, caller, request, nil, effect)
		if result.Receipt.Durable {
			unlock := node.agent.LockPublication()
			recordContext := context.WithoutCancel(node.ctx)
			var err error
			output, effectErr, err = node.recordWorkspaceEffect(recordContext, caller, request, output, effectErr, effect)
			if err != nil {
				effectErr = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "operation result could not be recorded; inspect the target before retrying"}
			} else {
				node.publish()
			}
			unlock()
		}
		// A completed command's effect is visible to its caller's next command.
		effect.close()
		completion <- response(output, effectErr)
	}()
	return result, nil
}

func (node *Node) workspaceIO(ctx context.Context, caller protocol.NodeID, request protocol.Request, prepared *files.Prepared, effect *workspaceCompletion) (protocol.Output, error) {
	command := request.Command
	if terminalCommand(command.Kind) {
		return node.terminalIO(ctx, caller, request, effect)
	}
	if networkCommand(command.Kind) {
		return node.networkIO(ctx, command)
	}
	if command.Kind == "read_project_catalog" || command.Kind == "read_worktree_catalog" {
		return node.readWorkspaceCatalog(ctx, command)
	}
	access := core.FileAccessForRequest(request, request.Plugin != nil)
	registrations := node.locks.registrations.Load()
	source, root, err := node.workspaceRoots(ctx, command, request.Plugin != nil)
	if err != nil {
		return protocol.Output{}, err
	}
	if request.Command.Durable() {
		switch command.Kind {
		case "run_git_action", "create_git_commit":
			// The Git service locks the root itself, around its local work
			// only, so that a transfer with a remote does not hold it.
		case "run_command", "stop_command", "stop_commands", "write_command":
		case "create_worktree", "create_managed_worktree", "remove_worktree":
			effect.release = node.locks.registration()
		default:
			effect.release = node.locks.effect(source, root)
		}
		if effect.release != nil && node.locks.registrations.Load() != registrations {
			// A project or worktree was registered, moved or removed while
			// this effect waited; it must not run in a root that changed.
			current, target, err := node.workspaceRoots(ctx, command, request.Plugin != nil)
			if err != nil {
				return protocol.Output{}, err
			}
			if current != source || target != root {
				return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "worktree changed while the operation waited; retry"}
			}
		}
	}
	if files.SupportsRead(command.Kind) {
		return node.inspectWorkspaceFiles(ctx, request, root, access)
	}
	if workspaceGitRead(command.Kind) {
		if command.Kind == "list_worktrees" {
			value, err := node.git.ListWorktrees(ctx, root)
			return protocol.Output{Kind: "git_worktrees", Data: value}, err
		}
		return node.git.Read(ctx, root, command)
	}
	if workspaceGitWrite(command.Kind) {
		return node.workspaceGitEffect(ctx, request, root, effect)
	}
	if files.SupportsMutation(command.Kind) {
		if err := node.protectWorkspaceMutation(source, root, command, prepared); err != nil {
			return protocol.Output{}, err
		}
		node.watches.changed(root, nil)
		defer node.watches.changed(root, nil)
		if source != root {
			node.watches.changed(source, nil)
			defer node.watches.changed(source, nil)
		}
		if fileTextMutation(command.Kind) {
			if prepared == nil {
				prepared, err = files.Prepare(ctx, root, command, access)
				if err != nil {
					return protocol.Output{}, err
				}
			}
			return files.Apply(ctx, root, prepared)
		}
		return files.MutateEntries(ctx, source, root, command, access)
	}
	switch command.Kind {
	case "fetch_web", "cancel_web_fetch", "search_web", "cancel_web_search", "test_search_driver", "read_search_settings", "save_search_settings":
		return node.workspaceWebIO(ctx, caller, request, effect)
	case "search_code", "outline_code", "graph_code", "cancel_code_index_query", "read_code_index_settings", "save_code_index_settings":
		return node.codeIndexIO(ctx, caller, request, root, effect)
	case "upload_attachment":
		value, err := node.transfers.UploadAttachment(ctx, caller, root, *command.Data.(*protocol.AttachmentSpec))
		if err == nil {
			err = node.recheckWorkspaceStream(ctx, value.Spec.Worktree, root, caller, value.Stream)
		}
		return protocol.Output{Kind: "attachment_upload", Data: value}, err
	case "upload_file":
		input := command.Data.(*protocol.FileUploadSpec)
		if err = node.protectEntry(root, input.Path); err != nil {
			return protocol.Output{}, err
		}
		value, err := node.transfers.UploadFile(ctx, caller, root, *input, access)
		if err == nil {
			err = node.recheckWorkspaceStream(ctx, value.Spec.Worktree, root, caller, value.Stream)
		}
		return protocol.Output{Kind: "file_upload", Data: value}, err
	case "download_file":
		input := command.Data.(*protocol.CommandDownloadFilePayload)
		value, err := node.transfers.DownloadFile(ctx, caller, root, input.Worktree, input.Path, access)
		if err == nil {
			err = node.recheckWorkspaceStream(ctx, input.Worktree, root, caller, value.Stream)
		}
		return protocol.Output{Kind: "file_download", Data: value}, err
	case "download_attachment":
		input := command.Data.(*protocol.CommandDownloadAttachmentPayload)
		var attachment protocol.Attachment
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			attachment, err = store.ReadAttachment(ctx, db, input.Worktree, input.Attachment)
			return err
		})
		if err != nil {
			return protocol.Output{}, err
		}
		value, err := node.transfers.DownloadAttachment(ctx, caller, root, attachment)
		if err == nil {
			err = node.recheckWorkspaceStream(ctx, input.Worktree, root, caller, value.Stream)
		}
		return protocol.Output{Kind: "attachment_download", Data: value}, err
	case "download_image":
		input := command.Data.(*protocol.CommandDownloadImagePayload)
		var data []byte
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			data, err = store.ReadCanonicalImage(ctx, db, input.Session, input.Image)
			return err
		})
		if err != nil {
			return protocol.Output{}, err
		}
		root, err = node.workspaceRoot(ctx, input.Image.Attachment.Spec.Worktree, false)
		if err != nil {
			return protocol.Output{}, err
		}
		value, err := node.transfers.DownloadBytes(ctx, caller, root, input.Image.Attachment, data)
		if err == nil {
			err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				_, err := store.ReadCanonicalImage(ctx, db, input.Session, input.Image)
				return err
			})
			if err != nil {
				_ = node.transfers.Cancel(caller, value.Stream)
			}
		}
		return protocol.Output{Kind: "attachment_download", Data: value}, err
	case "finish_file_upload", "finish_attachment_upload":
		if command.Kind == "finish_file_upload" {
			if err = node.protectEntry(root, command.Data.(*protocol.CommandFinishFileUploadPayload).Path); err != nil {
				return protocol.Output{}, err
			}
			defer node.watches.changed(root, nil)
		}
		return node.transfers.Finish(ctx, caller, root, command)
	case "cancel_file_transfer":
		id := command.Data.(*protocol.CommandCancelFileTransferPayload).Stream
		return protocol.Output{Kind: "file_transfer_cancelled", Data: protocol.OutputFileTransferCancelledPayload{Stream: id}}, node.transfers.Cancel(caller, id)
	case "read_artifact":
		return node.readWorkspaceArtifact(ctx, request)
	case "write_artifact":
		return node.writeWorkspaceArtifact(ctx, request, effect)
	case "discard_attachment":
		input := command.Data.(*protocol.CommandDiscardAttachmentPayload)
		effect.commit = func(ctx context.Context, db *sql.Conn, out protocol.Output) (protocol.Output, []protocol.Event, error) {
			return out, nil, store.DiscardAttachment(ctx, db, caller, input.Worktree, input.Attachment)
		}
		return protocol.Output{Kind: "attachment_discarded", Data: protocol.OutputAttachmentDiscardedPayload{Attachment: input.Attachment}}, nil
	case "restore_file_checkpoint":
		return node.restoreWorkspaceCheckpoint(ctx, request, root)
	case "apply_turn_hunk":
		return node.applyWorkspaceHunk(ctx, request, root)
	case "read_hooks", "save_hooks", "read_sandbox_settings", "save_sandbox_settings":
		return node.workspaceSettings(ctx, command, effect)
	case "run_command", "list_commands", "read_command", "read_command_page", "write_command", "stop_command", "stop_commands":
		return node.workspaceProcess(ctx, caller, request, effect)
	default:
		var out protocol.Output
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			out, err = store.ReadFilesCommand(ctx, db, caller, request)
			return err
		})
		return out, err
	}
}
func (node *Node) recheckWorkspaceStream(ctx context.Context, id protocol.WorktreeID, root string, caller protocol.NodeID, stream protocol.StreamID) error {
	current, err := node.workspaceRoot(ctx, id, false)
	if err != nil || current != root {
		_ = node.transfers.Cancel(caller, stream)
		return unavailable()
	}
	return nil
}
func withinWorkspace(path, root string) bool {
	relative, err := filepath.Rel(root, path)
	return err == nil && (relative == "." || relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator)))
}
func (node *Node) protectEntry(root, path string) error {
	absolute := filepath.Clean(filepath.Join(root, filepath.FromSlash(path)))
	if info, err := os.Lstat(absolute); err == nil && info.Mode()&os.ModeSymlink == 0 {
		if actual, e := filepath.EvalSymlinks(absolute); e == nil {
			absolute = actual
		}
	} else if parent, e := filepath.EvalSymlinks(filepath.Dir(absolute)); e == nil {
		absolute = filepath.Join(parent, filepath.Base(absolute))
	}
	var trees []protocol.Worktree
	var sessions []protocol.Session
	err := node.profile.Store.Node.Read(node.ctx, func(db *sql.Conn) error {
		var err error
		trees, err = store.ReadWorktrees(node.ctx, db)
		if err != nil {
			return err
		}
		snapshot, err := store.Snapshot(node.ctx, db, node.ID())
		sessions = snapshot.Sessions
		return err
	})
	if err != nil {
		return err
	}
	managed := false
	for _, tree := range trees {
		if withinWorkspace(tree.Path, absolute) {
			return &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "entry contains registered resources or Node storage"}
		}
		if tree.Path == root && !tree.Main && core.ManagedWorkspace(root, node.profile.Ownership.Path) {
			for _, session := range sessions {
				if session.Worktree == tree.ID && session.Project == nil {
					expected := core.SessionWorkspace(node.profile.Ownership.Path, session.ID)
					if session.Config.Resource != nil {
						expected, _ = core.ConnectionWorkspace(node.profile.Ownership.Path, *session.Config.Resource)
					} else if session.Config.Assistant != nil {
						expected, _ = core.AssistantWorkspace(node.profile.Ownership.Path, *session.Config.Assistant)
					}
					managed = managed || expected == root
				}
			}
		}
	}
	profile := node.profile.Ownership.Path
	if withinWorkspace(profile, absolute) || withinWorkspace(absolute, profile) && !managed {
		return &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "entry contains registered resources or Node storage"}
	}
	return nil
}
func (node *Node) protectWorkspaceMutation(source, root string, command protocol.Command, prepared *files.Prepared) error {
	var targets []struct{ root, path string }
	add := func(root, path string) { targets = append(targets, struct{ root, path string }{root, path}) }
	if prepared != nil {
		for _, value := range prepared.Changes {
			add(root, value.Path)
			if value.Destination != nil {
				add(root, *value.Destination)
			}
		}
	}
	switch v := command.Data.(type) {
	case *protocol.CommandWriteFilePayload:
		add(root, v.Path)
	case *protocol.CommandEditFilePayload:
		add(root, v.Path)
	case *protocol.CommandDeleteFilePayload:
		add(root, v.Path)
	case *protocol.CommandMoveFilePayload:
		add(root, v.From)
		add(root, v.To)
	case *protocol.CommandEditNotebookPayload:
		add(root, v.Edit.Path)
	case *protocol.CommandCreateDirectoryPayload:
		add(root, v.Path)
	case *protocol.CommandRenameEntryPayload:
		add(root, v.From)
		add(root, v.To)
	case *protocol.CommandCopyEntryPayload:
		add(root, v.From)
		add(root, v.To)
	case *protocol.CommandCopyEntryToPayload:
		add(source, v.From)
		add(root, v.To)
	case *protocol.CommandMoveEntryToPayload:
		add(source, v.From)
		add(root, v.To)
	case *protocol.CommandTrashEntryPayload:
		add(root, v.Path)
	case *protocol.CommandTrashFilePayload:
		add(root, v.Path)
	case *protocol.FileBrowserAction:
		values, err := files.BrowserTargets(*v)
		if err != nil {
			return err
		}
		for _, value := range values {
			add(value.Root, value.Name)
		}
	case *protocol.CommandApplyChangesPayload:
		for _, change := range v.Changes {
			raw, _ := json.Marshal(change.Data)
			var value struct {
				Path        string  `json:"path"`
				Destination *string `json:"destination"`
			}
			if err := json.Unmarshal(raw, &value); err != nil {
				return err
			}
			add(root, value.Path)
			if value.Destination != nil {
				add(root, *value.Destination)
			}
		}
	}
	for _, target := range targets {
		if err := node.protectEntry(target.root, target.path); err != nil {
			return err
		}
	}
	return nil
}

func (node *Node) workspaceSettings(ctx context.Context, command protocol.Command, effect *workspaceCompletion) (protocol.Output, error) {
	var output protocol.Output
	operation := func(ctx context.Context, db *sql.Conn) error {
		switch command.Kind {
		case "read_hooks":
			value, err := store.ReadHooks(ctx, db, command.Data.(*protocol.CommandReadHooksPayload).Worktree)
			output = protocol.Output{Kind: "hook_settings", Data: protocol.HooksState{Worktree: command.Data.(*protocol.CommandReadHooksPayload).Worktree, Settings: value, Sandboxed: process.SandboxSupported()}}
			return err
		case "save_hooks":
			v := command.Data.(*protocol.CommandSaveHooksPayload)
			value, err := store.SaveHooks(ctx, db, v.Worktree, v.Settings, v.ExpectedRevision)
			output = protocol.Output{Kind: "hook_settings", Data: protocol.HooksState{Worktree: v.Worktree, Settings: value, Sandboxed: process.SandboxSupported()}}
			return err
		case "read_sandbox_settings":
			value, err := store.ReadSandboxSettings(ctx, db)
			output = protocol.Output{Kind: "sandbox_settings", Data: value}
			return err
		case "save_sandbox_settings":
			value, err := store.SaveSandboxSettings(ctx, db, *command.Data.(*protocol.SandboxSettings))
			output = protocol.Output{Kind: "sandbox_settings", Data: value}
			if err != nil {
				return err
			}
			_, err = store.RecordEvents(ctx, db, node.ID(), []protocol.Event{{Kind: "sandbox_settings_changed", Data: protocol.EventSandboxSettingsChangedPayload{Revision: value.Revision}}})
			return err
		}
		return errors.New("workspace settings command required")
	}
	if command.Durable() {
		effect.commit = func(ctx context.Context, db *sql.Conn, out protocol.Output) (protocol.Output, []protocol.Event, error) {
			err := operation(ctx, db)
			return output, nil, err
		}
		return protocol.Output{}, nil
	}
	return outputAfter(&output, node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error { return operation(ctx, db) }))
}
func outputAfter(value *protocol.Output, err error) (protocol.Output, error) { return *value, err }

// recordWorkspaceEffect commits resource metadata and its receipt together.
func (node *Node) recordWorkspaceEffect(ctx context.Context, caller protocol.NodeID, request protocol.Request, output protocol.Output, effectErr error, effect *workspaceCompletion) (protocol.Output, error, error) {
	var changes []protocol.Event
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := db.ExecContext(ctx, "SAVEPOINT workspace_metadata"); err != nil {
			return err
		}
		if effectErr == nil && effect.commit != nil {
			output, changes, effectErr = effect.commit(ctx, db, output)
			if effectErr != nil && effect.external {
				effectErr = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "operation completed but its resource metadata could not be recorded; inspect the target before retrying"}
			}
		}
		if effectErr != nil {
			if _, err := db.ExecContext(ctx, "ROLLBACK TO workspace_metadata"); err != nil {
				return err
			}
			changes = nil
		}
		if _, err := db.ExecContext(ctx, "RELEASE workspace_metadata"); err != nil {
			return err
		}
		fileChanges, err := store.FinishFilesCommand(ctx, db, caller, request, output, effectErr)
		if err != nil {
			return err
		}
		changes = append(changes, fileChanges...)
		if _, err = store.RecordEvents(ctx, db, node.ID(), changes); err != nil {
			return err
		}
		return admission.Complete(ctx, db, caller, request.ID, response(output, effectErr))
	})
	if err == nil {
		node.publish()
		node.conversationEvents(changes)
	}
	return output, effectErr, err
}
