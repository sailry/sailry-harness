package node

import (
	"context"
	"encoding/json"
	"path/filepath"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// An outside path runs the confined operation from its volume root. A change
// set that also names worktree files retains that worktree's effect ownership.
type externalFileScope struct {
	root     string
	worktree string
}

// externalPathKeys are the file tool arguments that name paths.
var externalPathKeys = []string{"path", "destination"}

// patchPathPrefixes introduce the paths of an apply_patch input.
var patchPathPrefixes = []string{"*** Add File: ", "*** Update File: ", "*** Delete File: ", "*** Move to: "}

// localPath maps an absolute path inside the worktree to its worktree-relative
// form; other paths are unchanged.
func localPath(worktree, path string) string {
	if !filepath.IsAbs(path) {
		return path
	}
	clean := filepath.Clean(path)
	if withinWorkspace(clean, worktree) {
		rel, _ := filepath.Rel(worktree, clean)
		if rel == "." {
			return ""
		}
		return filepath.ToSlash(rel)
	}
	return path
}

// externalRoot returns the volume root of the first absolute path outside the
// worktree, or "" when every path is in the worktree.
func externalRoot(worktree string, paths []string) string {
	for _, path := range paths {
		if filepath.IsAbs(path) && !withinWorkspace(filepath.Clean(path), worktree) {
			return filepath.VolumeName(path) + string(filepath.Separator)
		}
	}
	return ""
}

// rootPath maps a worktree-relative or absolute path to its form relative to
// root, the volume root the call runs from.
func rootPath(worktree, root, path string) (string, error) {
	absolute := path
	if !filepath.IsAbs(path) {
		if err := core.CheckFilePath(path); err != nil {
			return "", err
		}
		absolute = filepath.Join(worktree, filepath.FromSlash(path))
	}
	absolute = filepath.Clean(absolute)
	if !strings.EqualFold(filepath.VolumeName(absolute), filepath.VolumeName(root)) {
		return "", &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "one file operation cannot span volumes"}
	}
	rel, err := filepath.Rel(root, absolute)
	if err != nil {
		return "", err
	}
	return filepath.ToSlash(rel), nil
}

// fileArgPaths rewrites the path arguments of a file tool call with change.
func fileArgPaths(op protocol.ToolOperation, args json.RawMessage, change func(string) (string, error)) (json.RawMessage, error) {
	var fields map[string]json.RawMessage
	if json.Unmarshal(args, &fields) != nil || fields == nil {
		return args, nil
	}
	changed := false
	rewrite := func(key string, edit func(string) (string, error)) error {
		raw, ok := fields[key]
		if !ok {
			return nil
		}
		var value string
		if json.Unmarshal(raw, &value) != nil {
			return nil
		}
		next, err := edit(value)
		if err != nil || next == value {
			return err
		}
		fields[key], _ = json.Marshal(next)
		changed = true
		return nil
	}
	if op == protocol.ToolOperationApplyChanges {
		if err := rewrite("input", func(input string) (string, error) { return patchPaths(input, change) }); err != nil {
			return nil, err
		}
	} else {
		for _, key := range externalPathKeys {
			if err := rewrite(key, func(path string) (string, error) {
				if path == "" {
					return path, nil
				}
				return change(path)
			}); err != nil {
				return nil, err
			}
		}
	}
	if !changed {
		return args, nil
	}
	return json.Marshal(fields)
}

// patchPaths rewrites the file paths an apply_patch input names.
func patchPaths(input string, change func(string) (string, error)) (string, error) {
	lines := strings.Split(input, "\n")
	for i, line := range lines {
		for _, prefix := range patchPathPrefixes {
			if path, ok := strings.CutPrefix(line, prefix); ok {
				trimmed := strings.TrimSuffix(path, "\r")
				next, err := change(trimmed)
				if err != nil {
					return "", err
				}
				lines[i] = prefix + next + path[len(trimmed):]
				break
			}
		}
	}
	return strings.Join(lines, "\n"), nil
}

// collectFileArgPaths lists the paths a file tool call names.
func collectFileArgPaths(op protocol.ToolOperation, args json.RawMessage) []string {
	var paths []string
	fileArgPaths(op, args, func(path string) (string, error) {
		paths = append(paths, path)
		return path, nil
	})
	return paths
}

func (scope externalFileScope) resultPath(path string) string {
	if filepath.IsAbs(path) {
		return path
	}
	path = filepath.Join(scope.root, filepath.FromSlash(path))
	if scope.worktree != "" {
		path = localPath(scope.worktree, path)
	}
	return filepath.ToSlash(path)
}

// result names only the paths defined by the file protocol. Opaque notebook
// values, cursor names and other file content are returned in their own format.
func (scope externalFileScope) result(output protocol.Output) protocol.Output {
	path := scope.resultPath
	paths := func(values []string) []string {
		values = slices.Clone(values)
		for i := range values {
			values[i] = path(values[i])
		}
		return values
	}
	diagnostics := func(values []protocol.SearchDiagnostic) []protocol.SearchDiagnostic {
		values = slices.Clone(values)
		for i := range values {
			values[i].Path = path(values[i].Path)
		}
		return values
	}
	switch value := output.Data.(type) {
	case protocol.Directory:
		value.Path = path(value.Path)
		output.Data = value
	case protocol.FileContent:
		value.Path = path(value.Path)
		output.Data = value
	case protocol.FileLines:
		value.Path = path(value.Path)
		output.Data = value
	case protocol.FileImage:
		value.Path = path(value.Path)
		output.Data = value
	case protocol.FileNames:
		value.Paths, value.Diagnostics = paths(value.Paths), diagnostics(value.Diagnostics)
		output.Data = value
	case protocol.SearchResults:
		value.Paths, value.Diagnostics = paths(value.Paths), diagnostics(value.Diagnostics)
		value.Matches, value.Counts = slices.Clone(value.Matches), slices.Clone(value.Counts)
		for i := range value.Matches {
			value.Matches[i].Path = path(value.Matches[i].Path)
		}
		for i := range value.Counts {
			value.Counts[i].Path = path(value.Counts[i].Path)
		}
		output.Data = value
	case protocol.NotebookPage:
		value.Path = path(value.Path)
		output.Data = value
	case protocol.FileWritten:
		value.Path = path(value.Path)
		output.Data = value
	case protocol.OutputDirectoryCreatedPayload:
		value.Path = path(value.Path)
		output.Data = value
	case protocol.OutputEntryTrashedPayload:
		value.Path = path(value.Path)
		output.Data = value
	case protocol.OutputEntryMovedPayload:
		value.From, value.To = path(value.From), path(value.To)
		output.Data = value
	case protocol.FileChanges:
		value.Files = slices.Clone(value.Files)
		for i := range value.Files {
			entry := &value.Files[i]
			if entry.Path != "" {
				entry.Path = path(entry.Path)
			}
			if entry.Destination != nil {
				destination := path(*entry.Destination)
				entry.Destination = &destination
			}
		}
		output.Data = value
	}
	return output
}

// checkpoints presents only the worktree writes to the checkpoint store,
// retaining the original command bound to its admission receipt.
func (scope externalFileScope) checkpoints(prepared *files.Prepared) *files.Prepared {
	if scope.worktree == "" {
		return prepared
	}
	local := *prepared
	local.Changes = nil
	for _, entry := range prepared.Changes {
		entry.Path = scope.resultPath(entry.Path)
		if !filepath.IsAbs(entry.Path) {
			local.Changes = append(local.Changes, entry)
		}
	}
	return &local
}

// externalFileOperation reports the file tools that may name paths outside
// the session worktree.
func externalFileOperation(op protocol.ToolOperation) bool {
	switch op {
	case protocol.ToolOperationListDirectory, protocol.ToolOperationReadFile, protocol.ToolOperationReadFileLines,
		protocol.ToolOperationWriteFile, protocol.ToolOperationEditFile, protocol.ToolOperationDeleteFile,
		protocol.ToolOperationMoveFile, protocol.ToolOperationApplyChanges, protocol.ToolOperationCreateDirectory,
		protocol.ToolOperationFindFiles, protocol.ToolOperationSearchFiles,
		protocol.ToolOperationReadNotebook, protocol.ToolOperationEditNotebook:
		return true
	}
	return false
}

// externalFileIO runs a prepared file command from root, after the same
// caller authorization and entry protection as a worktree command.
func (node *Node) externalFileIO(ctx context.Context, caller protocol.NodeID, request protocol.Request, prepared *files.Prepared, root string) (protocol.Output, error) {
	ctx, err := node.authorizeCommand(ctx, caller, request)
	if err != nil {
		return protocol.Output{}, err
	}
	command := request.Command
	if files.SupportsMutation(command.Kind) {
		if create, ok := command.Data.(*protocol.CommandCreateDirectoryPayload); ok {
			defer node.locks.joinEffect(node.locks.reserve([]string{workspaceFileKey(root, create.Path)}, false))()
		} else {
			defer node.locks.fileEffect(root, prepared)()
		}
	}
	access := core.FileAccessForRequest(request, false)
	switch {
	case files.SupportsRead(command.Kind):
		return node.inspectWorkspaceFiles(ctx, request, root, access)
	case fileTextMutation(command.Kind):
		if err := node.protectWorkspaceMutation(root, root, command, prepared, externalFileScope{}); err != nil {
			return protocol.Output{}, err
		}
		if prepared == nil {
			if prepared, err = files.Prepare(ctx, root, command, access); err != nil {
				return protocol.Output{}, err
			}
		}
		return files.Apply(ctx, root, prepared)
	case files.SupportsMutation(command.Kind):
		if err := node.protectWorkspaceMutation(root, root, command, prepared, externalFileScope{}); err != nil {
			return protocol.Output{}, err
		}
		return files.MutateEntries(ctx, root, root, command, access)
	}
	return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "this operation needs a path inside the session worktree"}
}

// canonicalPath resolves the symbolic links in an absolute path's existing
// ancestors, as the confined file service accepts only ordinary parent
// directories (macOS /var and /tmp are links); other paths are unchanged.
// The named entry itself stays as given, so a link it names is the entry the
// file service checks, never the link's target.
func canonicalPath(path string) string {
	if !filepath.IsAbs(path) {
		return path
	}
	clean := filepath.Clean(path)
	parent := filepath.Dir(clean)
	if parent == clean {
		return clean
	}
	return filepath.Join(resolvedPath(parent), filepath.Base(clean))
}

// resolvedPath resolves every symbolic link in an absolute path, through its
// deepest existing directory.
func resolvedPath(path string) string {
	return filepath.Join(resolvedPathParts(path))
}

func resolvedPathParts(path string) (existing, missing string) {
	current, rest := path, ""
	for {
		if resolved, err := filepath.EvalSymlinks(current); err == nil {
			return resolved, rest
		}
		parent := filepath.Dir(current)
		if parent == current {
			return path, ""
		}
		rest = filepath.Join(filepath.Base(current), rest)
		current = parent
	}
}
