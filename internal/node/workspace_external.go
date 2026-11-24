package node

import (
	"context"
	"encoding/json"
	"path/filepath"
	"strings"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Full permission runs commands under the Node's OS access, so its file tools
// reach the same places: an absolute path outside the session worktree runs
// the same confined file operation from that path's volume root instead of a
// registered worktree. Those calls have no worktree, so they take no worktree
// locks, checkpoints or durable admission; revisions still guard their writes.

// externalPathKeys are the file tool arguments that name paths.
var externalPathKeys = []string{"path", "destination"}

// patchPathPrefixes introduce the paths of an apply_patch input.
var patchPathPrefixes = []string{"*** Add File: ", "*** Update File: ", "*** Delete File: ", "*** Move to: "}

// within reports whether clean absolute path lies in root or is root.
func within(root, path string) bool {
	rel, err := filepath.Rel(root, path)
	return err == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator))
}

// localPath maps an absolute path inside the worktree to its worktree-relative
// form; other paths are unchanged.
func localPath(worktree, path string) string {
	if !filepath.IsAbs(path) {
		return path
	}
	clean := filepath.Clean(path)
	if clean == worktree {
		return "."
	}
	if within(worktree, clean) {
		rel, _ := filepath.Rel(worktree, clean)
		return filepath.ToSlash(rel)
	}
	return path
}

// externalRoot returns the volume root of the first absolute path outside the
// worktree, or "" when every path is in the worktree.
func externalRoot(worktree string, paths []string) string {
	for _, path := range paths {
		if filepath.IsAbs(path) && !within(worktree, filepath.Clean(path)) {
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
				trimmed := strings.TrimRight(path, "\r")
				next, err := change(strings.TrimSpace(trimmed))
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

// absoluteResult rewrites the paths of a file tool result run from root back
// to absolute paths, so the model reads, and later revisions match, the paths
// it named.
func absoluteResult(root string, value any) any {
	raw, err := json.Marshal(value)
	if err != nil {
		return value
	}
	var tree any
	if json.Unmarshal(raw, &tree) != nil {
		return value
	}
	var walk func(any)
	walk = func(node any) {
		switch v := node.(type) {
		case map[string]any:
			for key, child := range v {
				switch key {
				case "path", "destination", "from", "to":
					if path, ok := child.(string); ok && path != "" && !filepath.IsAbs(path) {
						v[key] = filepath.ToSlash(filepath.Join(root, filepath.FromSlash(path)))
						continue
					}
				}
				walk(child)
			}
		case []any:
			for _, child := range v {
				walk(child)
			}
		}
	}
	walk(tree)
	return tree
}

// rootRevisions keys observed revisions by their paths relative to root.
func rootRevisions(worktree, root string, observed map[string]string) map[string]string {
	keyed := make(map[string]string, len(observed))
	for path, revision := range observed {
		if rel, err := rootPath(worktree, root, path); err == nil {
			keyed[rel] = revision
		}
	}
	return keyed
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
	access := core.FileAccessForRequest(request, false)
	switch {
	case files.SupportsRead(command.Kind):
		return node.inspectWorkspaceFiles(ctx, request, root, access)
	case fileTextMutation(command.Kind):
		if err := node.protectWorkspaceMutation(root, root, command, prepared); err != nil {
			return protocol.Output{}, err
		}
		if prepared == nil {
			if prepared, err = files.Prepare(ctx, root, command, access); err != nil {
				return protocol.Output{}, err
			}
		}
		return files.Apply(ctx, root, prepared)
	case files.SupportsMutation(command.Kind):
		if err := node.protectWorkspaceMutation(root, root, command, prepared); err != nil {
			return protocol.Output{}, err
		}
		return files.MutateEntries(ctx, root, root, command, access)
	}
	return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "this operation needs a path inside the session worktree"}
}

// canonicalPath resolves the symbolic links in an absolute path's existing
// ancestors, as the confined file service accepts only ordinary parent
// directories (macOS /var and /tmp are links); other paths are unchanged.
func canonicalPath(path string) string {
	if !filepath.IsAbs(path) {
		return path
	}
	current, rest := filepath.Clean(path), ""
	for {
		if resolved, err := filepath.EvalSymlinks(current); err == nil {
			return filepath.Join(resolved, rest)
		}
		parent := filepath.Dir(current)
		if parent == current {
			return path
		}
		rest = filepath.Join(filepath.Base(current), rest)
		current = parent
	}
}
