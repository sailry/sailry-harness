// Package files owns workspace effects outside the Node database writer.
package files

import (
	"context"
	"encoding/hex"
	"errors"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/office"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
	"strings"
	"time"
)

func invalid(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}
func conflict(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: message}
}
func fault(err error) *protocol.Fault {
	var value *protocol.Fault
	if errors.As(err, &value) {
		return value
	}
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: err.Error()}
}
func revision(data []byte) string { sum := blake3.Sum256(data); return hex.EncodeToString(sum[:]) }
func validRevision(value string) bool {
	raw, err := hex.DecodeString(value)
	return err == nil && len(raw) == 32
}
func SupportsRead(kind string) bool {
	switch kind {
	case "browse_files", "list_directory", "read_file", "read_file_lines", "find_files", "search_files", "read_image", "read_notebook", "read_office":
		return true
	}
	return false
}
func Inspect(ctx context.Context, root string, command protocol.Command, access core.FileAccess) (protocol.Output, error) {
	timeout := 5 * time.Second
	if command.Kind == "find_files" || command.Kind == "search_files" {
		timeout = 20 * time.Second
	}
	ctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	if command.Kind == "browse_files" {
		v := command.Data.(*protocol.CommandBrowseFilesPayload)
		result, err := Browse(ctx, v.Directory, v.After)
		return protocol.Output{Kind: "file_listing", Data: result}, err
	}
	fs, err := core.OpenFiles(root, access)
	if err != nil {
		return protocol.Output{}, err
	}
	defer fs.Close()
	var value any
	var kind string
	switch command.Kind {
	case "list_directory":
		v := command.Data.(*protocol.CommandListDirectoryPayload)
		kind = "directory"
		value, err = fs.ListDirectory(ctx, v.Path, v.After)
	case "read_file":
		v := command.Data.(*protocol.CommandReadFilePayload)
		kind = "file_content"
		value, err = fs.ReadContent(ctx, v.Path)
	case "read_file_lines":
		v := command.Data.(*protocol.CommandReadFileLinesPayload)
		kind = "file_lines"
		value, err = fs.ReadLines(ctx, v.Path, v.Offset, v.Limit, v.ExpectedRevision)
	case "find_files":
		v := command.Data.(*protocol.CommandFindFilesPayload)
		kind = "file_names"
		value, err = fs.FindFiles(ctx, v.Globs, v.Path, v.Limit, v.Offset)
	case "search_files":
		v := command.Data.(*protocol.CommandSearchFilesPayload)
		kind = "search_results"
		value, err = Search(ctx, fs, v.Options)
	case "read_notebook":
		v := command.Data.(*protocol.CommandReadNotebookPayload)
		kind = "notebook_page"
		value, err = ReadNotebook(ctx, fs, v.Options)
	case "read_image":
		v := command.Data.(*protocol.CommandReadImagePayload)
		kind = "file_image"
		value, err = ReadImage(ctx, fs, v.Path)
	case "read_office":
		v := command.Data.(*protocol.CommandReadOfficePayload)
		kind = "office_content"
		if v.Options.Source.Kind != "path" {
			return protocol.Output{}, invalid("attachment inspection requires a captured turn")
		}
		path, ok := v.Options.Source.Data.(string)
		if !ok {
			pointer, yes := v.Options.Source.Data.(*string)
			if !yes || pointer == nil {
				return protocol.Output{}, invalid("invalid Office source")
			}
			path = *pointer
		}
		file, e := fs.Open(path)
		if e != nil {
			return protocol.Output{}, e
		}
		defer file.Close()
		data, e := readBounded(ctx, file, office.Limit)
		if e != nil {
			return protocol.Output{}, e
		}
		value, err = office.Inspect(path, data, v.Options.Offset)
	default:
		return protocol.Output{}, invalid("invalid file inspection command")
	}
	return protocol.Output{Kind: kind, Data: value}, err
}

func changePayload[T any](data any) (T, bool) {
	if value, ok := data.(T); ok {
		return value, true
	}
	if value, ok := data.(*T); ok && value != nil {
		return *value, true
	}
	var zero T
	return zero, false
}
func nonnull(text string) bool { return !strings.ContainsRune(text, 0) }
