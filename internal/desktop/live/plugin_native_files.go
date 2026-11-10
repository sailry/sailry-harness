package live

import (
	"context"
	"encoding/json"
	"mime"
	"path"
	"path/filepath"
	"runtime"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (state *pluginNativeState) fileOperations() {
	operations := state.native.Operations
	operations["sailry/ui.inspectFilePath"] = func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		name, err := pluginNativeArgument[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		media := mime.TypeByExtension(strings.ToLower(path.Ext(name)))
		if media == "" {
			media = "application/octet-stream"
		}
		if contentType, _, err := mime.ParseMediaType(media); err == nil {
			media = contentType
		}
		kind := attachmentFileKind(name, media)
		return pluginNativeJSON(map[string]any{"mime": media, "image": kind == conversation.ImageFile, "external": kind == conversation.OtherFile})
	}
	operations["sailry/ui.openFileImage"] = func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		name, err := pluginNativeArgument[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		if !validWorkspaceRelative(name) || attachmentFileKind(name, "") != conversation.ImageFile || state.scope.Worktree == nil {
			return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "image requires a captured file source")
		}
		return state.effect(func(view *ui.Context) {
			project, err := state.fileProject()
			if err != nil {
				state.runtime.packageReport(view, err)
				return
			}
			project.openFileKind(name)
		})
	}
	operations["sailry/ui.openFile"] = func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		name, err := pluginNativeArgument[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		line := 0
		if len(arguments) > 1 && string(arguments[1]) != "null" {
			line, err = pluginNativeArgument[int](arguments, 1)
			if err != nil || line < 0 {
				return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid file line")
			}
		}
		if !validWorkspaceRelative(name) {
			return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid workspace file path")
		}
		return state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
			project, err := state.fileProject()
			if err != nil {
				return nil, err
			}
			if !project.openFileKind(name) {
				project.openFileAt(name, line)
			}
			return pluginNativeJSON(true)
		})
	}
	operations["sailry/ui.canInsertFileReferences"] = func(ctx context.Context, _ []json.RawMessage) (json.RawMessage, error) {
		return state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
			return pluginNativeJSON(state.fileComposer() != nil)
		})
	}
	operations["sailry/ui.insertFileReferences"] = func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		files, err := pluginNativeArgument[[]struct {
			Path      string `json:"path"`
			Directory bool   `json:"directory"`
		}](arguments, 0)
		if err != nil {
			return nil, err
		}
		for _, file := range files {
			if !validWorkspaceRelative(file.Path) {
				return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid file reference")
			}
		}
		return state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
			target := state.fileComposer()
			if target == nil {
				return pluginNativeJSON(false)
			}
			for _, file := range files {
				target.InsertFiles([]string{file.Path}, "")
				if file.Directory {
					for index := range target.References {
						reference := &target.References[index]
						if reference.Kind == composer.File && reference.ID == file.Path {
							reference.Kind, reference.Icon = composer.Folder, icons.Folder
						}
					}
				}
			}
			return pluginNativeJSON(true)
		})
	}
	operations["sailry/file-transfers.systemFileManager"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		platform := runtime.GOOS
		if platform == "darwin" {
			platform = "macos"
		}
		available := state.scope.Worktree != nil && state.runtime.attachments != nil &&
			state.runtime.attachments.native.LocalNode == state.runtime.controller.Target() &&
			state.runtime.attachments.native.RevealLocal != nil
		return pluginNativeJSON(map[string]any{"available": available, "platform": platform})
	}
	operations["sailry/file-transfers.revealSystem"] = func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		name, err := pluginNativeArgument[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		if state.scope.Worktree == nil || !validWorkspaceRelative(name) {
			return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "file reveal requires a captured relative path")
		}
		if state.runtime.attachments == nil || state.runtime.attachments.native.LocalNode != state.runtime.controller.Target() || state.runtime.attachments.native.RevealLocal == nil {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "file manager reveal requires the execution Node on this device")
		}
		if _, err := state.execute(ctx, protocol.Command{Kind: "list_directory", Data: protocol.CommandListDirectoryPayload{Worktree: *state.scope.Worktree, Path: "."}}); err != nil {
			return nil, err
		}
		var root string
		for _, worktree := range state.runtime.controller.Snapshot().Worktrees {
			if worktree.ID == *state.scope.Worktree {
				root = worktree.Path
				break
			}
		}
		base, err := filepath.EvalSymlinks(root)
		if err != nil || root == "" {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "captured worktree is unavailable")
		}
		target, err := filepath.EvalSymlinks(filepath.Join(base, filepath.FromSlash(name)))
		if err != nil {
			return nil, pluginNativeFault(protocol.ErrorCodeNotFound, "file reveal target is unavailable")
		}
		relative, err := filepath.Rel(base, target)
		if err != nil || relative == ".." || strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
			return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "file reveal target is outside the captured worktree")
		}
		return state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
			state.runtime.attachments.native.RevealLocal(target)
			return json.RawMessage("null"), nil
		})
	}
}

func (state *pluginNativeState) fileProject() (*workspaceProject, error) {
	info, available := state.packageInfo()
	if !available || info.Extension == nil || !slices.Contains(info.Extension.Actions, protocol.PluginActionReadFiles) || state.scope.Worktree == nil {
		return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "plugin does not have a captured file source")
	}
	workspace := state.runtime.workspace
	if workspace == nil || workspace.selected != *state.scope.Worktree {
		return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "captured workspace is inactive")
	}
	project := workspace.projects[*state.scope.Worktree]
	if project == nil || project.panel == nil {
		return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "document owner is unavailable")
	}
	return project, nil
}

func (state *pluginNativeState) fileComposer() *composer.State {
	if _, err := state.fileProject(); err != nil {
		return nil
	}
	selected := state.runtime.App.Sidebar.Selected
	session, exists := state.runtime.session(selected)
	if !exists || session.Worktree != *state.scope.Worktree || state.scope.Session != nil && session.ID != *state.scope.Session {
		return nil
	}
	view := state.runtime.App.Data.Find(selected)
	if view == nil || view.Node != state.runtime.controller.Target().String() {
		return nil
	}
	return state.runtime.App.Composer(selected)
}
