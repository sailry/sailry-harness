package files

import (
	"context"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func SupportsMutation(kind string) bool {
	switch kind {
	case "write_file", "edit_file", "edit_notebook", "delete_file", "move_file", "apply_changes", "create_directory", "rename_entry", "copy_entry", "copy_entry_to", "move_entry_to", "trash_entry", "trash_file", "manage_files":
		return true
	}
	return false
}

// MutateEntries receives independently captured source and destination roots.
func MutateEntries(ctx context.Context, source, root string, command protocol.Command, access core.FileAccess) (protocol.Output, error) {
	if command.Kind == "manage_files" {
		action, ok := changePayload[protocol.FileBrowserAction](command.Data)
		if !ok {
			return protocol.Output{}, invalid("invalid file browser action")
		}
		results, err := Manage(ctx, action)
		return protocol.Output{Kind: "files_managed", Data: results}, err
	}
	fs, err := core.OpenFiles(root, access)
	if err != nil {
		return protocol.Output{}, err
	}
	defer fs.Close()
	switch command.Kind {
	case "create_directory":
		v := command.Data.(*protocol.CommandCreateDirectoryPayload)
		err = fs.CreateDirectory(v.Path)
		return protocol.Output{Kind: "directory_created", Data: protocol.OutputDirectoryCreatedPayload{Path: v.Path}}, err
	case "rename_entry":
		v := command.Data.(*protocol.CommandRenameEntryPayload)
		err = fs.MoveEntryTo(ctx, fs, v.From, v.To)
		return protocol.Output{Kind: "entry_renamed", Data: protocol.OutputEntryRenamedPayload{From: v.From, To: v.To}}, err
	case "copy_entry":
		v := command.Data.(*protocol.CommandCopyEntryPayload)
		err = fs.CopyEntry(ctx, v.From, v.To)
		return protocol.Output{Kind: "entry_copied", Data: protocol.OutputEntryCopiedPayload{From: v.From, To: v.To}}, err
	case "copy_entry_to":
		v := command.Data.(*protocol.CommandCopyEntryToPayload)
		origin, e := core.OpenFiles(source, access)
		if e != nil {
			return protocol.Output{}, e
		}
		defer origin.Close()
		err = origin.CopyEntryTo(ctx, fs, v.From, v.To, false)
		return protocol.Output{Kind: "entry_copied", Data: protocol.OutputEntryCopiedPayload{From: v.From, To: v.To}}, err
	case "move_entry_to":
		v := command.Data.(*protocol.CommandMoveEntryToPayload)
		origin, e := core.OpenFiles(source, access)
		if e != nil {
			return protocol.Output{}, e
		}
		defer origin.Close()
		err = origin.MoveEntryTo(ctx, fs, v.From, v.To)
		return protocol.Output{Kind: "entry_moved", Data: protocol.OutputEntryMovedPayload{From: v.From, To: v.To}}, err
	case "trash_entry":
		v := command.Data.(*protocol.CommandTrashEntryPayload)
		err = fs.Trash(v.Path)
		return protocol.Output{Kind: "entry_trashed", Data: protocol.OutputEntryTrashedPayload{Path: v.Path}}, err
	case "trash_file":
		v := command.Data.(*protocol.CommandTrashFilePayload)
		err = fs.RemoveVerified(ctx, v.Path, v.ExpectedRevision, v.ExpectedStamp)
		return protocol.Output{Kind: "entry_trashed", Data: protocol.OutputEntryTrashedPayload{Path: v.Path}}, err
	}
	return protocol.Output{}, invalid("entry mutation command required")
}

type BrowserTarget struct{ Root, Name string }

func browserSplit(path string) (BrowserTarget, error) {
	if !filepath.IsAbs(path) || !utf8.ValidString(path) || strings.ContainsRune(path, 0) {
		return BrowserTarget{}, invalid("an absolute UTF-8 entry path is required")
	}
	clean := filepath.Clean(path)
	parent := filepath.Dir(clean)
	if clean == parent {
		return BrowserTarget{}, invalid("a filesystem root cannot be modified")
	}
	parent, err := filepath.EvalSymlinks(parent)
	if err != nil {
		return BrowserTarget{}, err
	}
	name := filepath.Base(clean)
	fs, err := core.OpenFiles(parent, core.ManualFiles)
	if err != nil {
		return BrowserTarget{}, err
	}
	defer fs.Close()
	if err = fs.ValidatePath(name, true); err != nil {
		return BrowserTarget{}, err
	}
	return BrowserTarget{parent, name}, nil
}
func BrowserTargets(action protocol.FileBrowserAction) ([]BrowserTarget, error) {
	var paths []string
	switch action.Kind {
	case "create_directory":
		v, ok := changePayload[protocol.FileBrowserActionCreateDirectoryPayload](action.Data)
		if !ok {
			return nil, invalid("invalid browser action")
		}
		paths = []string{v.Path}
	case "rename":
		v, ok := changePayload[protocol.FileBrowserActionRenamePayload](action.Data)
		if !ok {
			return nil, invalid("invalid browser action")
		}
		paths = []string{v.From, v.To}
	case "transfer":
		v, ok := changePayload[protocol.FileBrowserActionTransferPayload](action.Data)
		if !ok || len(v.Paths) < 1 || len(v.Paths) > 100 {
			return nil, invalid("select between one and one hundred entries")
		}
		if !filepath.IsAbs(v.Destination) {
			return nil, invalid("an absolute destination directory is required")
		}
		destination, err := filepath.EvalSymlinks(v.Destination)
		if err != nil {
			return nil, err
		}
		for _, path := range v.Paths {
			source, err := browserSplit(path)
			if err != nil {
				return nil, err
			}
			paths = append(paths, path, filepath.Join(destination, source.Name))
		}
	case "trash":
		v, ok := changePayload[protocol.FileBrowserActionTrashPayload](action.Data)
		if !ok || len(v.Paths) < 1 || len(v.Paths) > 100 {
			return nil, invalid("select between one and one hundred entries")
		}
		paths = v.Paths
	default:
		return nil, invalid("invalid browser action")
	}
	var out []BrowserTarget
	home, _ := os.UserHomeDir()
	home, _ = filepath.EvalSymlinks(home)
	for _, path := range paths {
		target, err := browserSplit(path)
		if err != nil {
			return nil, err
		}
		absolute := filepath.Join(target.Root, target.Name)
		if home != "" {
			relative, e := filepath.Rel(absolute, home)
			if e == nil && (relative == "." || relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator))) {
				return nil, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "a home directory or its ancestors cannot be modified"}
			}
		}
		out = append(out, target)
	}
	return out, nil
}
func Manage(ctx context.Context, action protocol.FileBrowserAction) ([]protocol.FileBrowserOutcome, error) {
	if _, err := BrowserTargets(action); err != nil {
		return nil, err
	}
	var paths []string
	switch action.Kind {
	case "create_directory":
		v, _ := changePayload[protocol.FileBrowserActionCreateDirectoryPayload](action.Data)
		paths = []string{v.Path}
	case "rename":
		v, _ := changePayload[protocol.FileBrowserActionRenamePayload](action.Data)
		paths = []string{v.From}
	case "transfer":
		v, _ := changePayload[protocol.FileBrowserActionTransferPayload](action.Data)
		paths = v.Paths
	case "trash":
		v, _ := changePayload[protocol.FileBrowserActionTrashPayload](action.Data)
		paths = v.Paths
	}
	out := []protocol.FileBrowserOutcome{}
	for _, path := range paths {
		destination, err := browserApply(ctx, action, path)
		item := protocol.FileBrowserOutcome{Path: path, Destination: destination}
		if err != nil {
			item.Error = fault(err)
		}
		out = append(out, item)
		if err != nil {
			break
		}
	}
	return out, nil
}
func browserApply(ctx context.Context, action protocol.FileBrowserAction, path string) (*string, error) {
	source, err := browserSplit(path)
	if err != nil {
		return nil, err
	}
	fs, err := core.OpenFiles(source.Root, core.ManualFiles)
	if err != nil {
		return nil, err
	}
	defer fs.Close()
	switch action.Kind {
	case "create_directory":
		err = fs.CreateDirectory(source.Name)
		destination := filepath.Join(source.Root, source.Name)
		return &destination, err
	case "trash":
		return nil, fs.Trash(source.Name)
	case "rename":
		v, _ := changePayload[protocol.FileBrowserActionRenamePayload](action.Data)
		target, err := browserSplit(v.To)
		if err != nil {
			return nil, err
		}
		destination, err := core.OpenFiles(target.Root, core.ManualFiles)
		if err != nil {
			return nil, err
		}
		defer destination.Close()
		err = fs.MoveEntryTo(ctx, destination, source.Name, target.Name)
		text := filepath.Join(target.Root, target.Name)
		return &text, err
	case "transfer":
		v, _ := changePayload[protocol.FileBrowserActionTransferPayload](action.Data)
		target, err := filepath.EvalSymlinks(v.Destination)
		if err != nil {
			return nil, err
		}
		destination, err := core.OpenFiles(target, core.ManualFiles)
		if err != nil {
			return nil, err
		}
		defer destination.Close()
		name := source.Name
		original := filepath.Join(source.Root, name)
		metadata, err := os.Lstat(original)
		if err != nil {
			return nil, err
		}
		if metadata.Mode()&os.ModeSymlink != 0 || !metadata.IsDir() && !metadata.Mode().IsRegular() {
			return nil, invalid("only regular files and directories can be transferred")
		}
		relative, e := filepath.Rel(original, target)
		if metadata.IsDir() && e == nil && (relative == "." || relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator))) {
			return nil, invalid("a directory cannot be transferred into itself")
		}
		replaced := false
		if _, err = os.Lstat(filepath.Join(target, name)); err == nil {
			switch v.Conflict {
			case protocol.FileBrowserConflictStop:
				return nil, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "an entry already exists at the destination"}
			case protocol.FileBrowserConflictReplace:
				relative, e := filepath.Rel(filepath.Join(target, name), original)
				if e == nil && (relative == "." || relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator))) {
					return nil, invalid("an entry cannot replace itself or its ancestors")
				}
				if err = destination.Trash(name); err != nil {
					return nil, err
				}
				replaced = true
			case protocol.FileBrowserConflictKeepBoth:
				stem, suffix := name, ""
				if !metadata.IsDir() {
					suffix = filepath.Ext(name)
					stem = strings.TrimSuffix(name, suffix)
				}
				found := false
				for index := 2; index <= 10000; index++ {
					candidate := fmtBrowserName(stem, suffix, index)
					if _, err = os.Lstat(filepath.Join(target, candidate)); os.IsNotExist(err) {
						name = candidate
						found = true
						break
					} else if err != nil {
						return nil, err
					}
				}
				if !found {
					return nil, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "no available destination name"}
				}
			default:
				return nil, invalid("invalid browser conflict choice")
			}
		} else if !os.IsNotExist(err) {
			return nil, err
		}
		if v.Cut {
			err = fs.MoveEntryTo(ctx, destination, source.Name, name)
		} else {
			err = fs.CopyEntryTo(ctx, destination, source.Name, name, false)
		}
		if err != nil && replaced {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "destination was moved to Trash but transfer failed; inspect both locations"}
		}
		text := filepath.Join(target, name)
		return &text, err
	}
	return nil, invalid("invalid browser action")
}

func fmtBrowserName(stem, suffix string, index int) string {
	return stem + " (" + strconv.Itoa(index) + ")" + suffix
}
