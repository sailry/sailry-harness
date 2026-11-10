package files

import (
	"context"
	"encoding/binary"
	"fmt"
	"strconv"
	"strings"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func CheckpointReceipt(file protocol.ConversationCheckpointFile) protocol.ConversationCheckpointReceipt {
	switch file.Outcome.Kind {
	case "not_admitted":
		return protocol.ConversationCheckpointReceiptAbsent
	case "admitted":
		return protocol.ConversationCheckpointReceiptPending
	case "unknown":
		return protocol.ConversationCheckpointReceiptUncertain
	case "completed":
		result, ok := changePayload[protocol.Result[protocol.Output, protocol.Fault]](file.Outcome.Data)
		if !ok {
			return protocol.ConversationCheckpointReceiptAbsent
		}
		if result.Err != nil {
			if result.Err.Code == protocol.ErrorCodeOutcomeUnknown {
				return protocol.ConversationCheckpointReceiptUncertain
			}
			return protocol.ConversationCheckpointReceiptAbsent
		}
		if result.Ok == nil {
			return protocol.ConversationCheckpointReceiptAbsent
		}
		switch result.Ok.Kind {
		case "file_written":
			written, ok := changePayload[protocol.FileWritten](result.Ok.Data)
			if ok && written.Path == file.Path && written.Revision == file.After.Revision && written.Size == file.After.Size {
				return protocol.ConversationCheckpointReceiptWritten
			}
		case "file_changes":
			changes, ok := changePayload[protocol.FileChanges](result.Ok.Data)
			if ok {
				for _, item := range changes.Files {
					if item.Path != file.Path {
						continue
					}
					if item.Status == protocol.ChangeStatusUnknown {
						return protocol.ConversationCheckpointReceiptUncertain
					}
					if item.Status == protocol.ChangeStatusApplied && item.Revision != nil && *item.Revision == file.After.Revision && item.Size != nil && *item.Size == file.After.Size {
						return protocol.ConversationCheckpointReceiptWritten
					}
				}
			}
		}
	}
	return protocol.ConversationCheckpointReceiptAbsent
}
func RestoreCheckpoint(ctx context.Context, root string, content protocol.ConversationCheckpointContent) (protocol.ConversationCheckpointRestored, error) {
	out := protocol.ConversationCheckpointRestored{Session: content.Session, Checkpoint: content.File.ID, Path: content.File.Path}
	fs, err := core.OpenFiles(root, core.RestrictedFiles)
	if err != nil {
		return out, err
	}
	defer fs.Close()
	if content.Before != nil {
		written, err := fs.RestoreText(ctx, content.File.Path, *content.Before, content.File.After.Revision)
		if err != nil {
			return out, err
		}
		out.Revision = &written.Revision
		return out, nil
	}
	if err = fs.DeleteFile(ctx, content.File.Path, content.File.After.Revision); err != nil {
		return out, err
	}
	return out, nil
}

type HunkChange struct {
	Hunk          protocol.ConversationCheckpointHunk
	Before, After string
}

func SplitHunks(path, before, after string, segment int) ([]HunkChange, error) {
	text, _, _ := TextDiff(path, before, after)
	var out []HunkChange
	var current *HunkChange
	lines := strings.SplitAfter(text, "\n")
	last := byte(0)
	for _, line := range lines {
		if strings.HasPrefix(line, "@@ ") {
			fields := strings.Fields(line)
			if len(fields) < 4 {
				return nil, invalid("invalid generated hunk")
			}
			old, _, _ := strings.Cut(strings.TrimPrefix(fields[1], "-"), ",")
			next, _, _ := strings.Cut(strings.TrimPrefix(fields[2], "+"), ",")
			oldStart, e := strconv.ParseUint(old, 10, 32)
			if e != nil {
				return nil, e
			}
			newStart, e := strconv.ParseUint(next, 10, 32)
			if e != nil {
				return nil, e
			}
			out = append(out, HunkChange{Hunk: protocol.ConversationCheckpointHunk{OldStart: uint32(oldStart), NewStart: uint32(newStart), Text: line}})
			current = &out[len(out)-1]
			last = 0
			continue
		}
		if current == nil || line == "" {
			continue
		}
		if strings.HasPrefix(line, "\\ No newline at end of file") {
			if last == ' ' || last == '-' {
				current.Before = strings.TrimSuffix(current.Before, "\n")
			}
			if last == ' ' || last == '+' {
				current.After = strings.TrimSuffix(current.After, "\n")
			}
			current.Hunk.Text += line
			continue
		}
		kind := line[0]
		if kind != ' ' && kind != '+' && kind != '-' {
			continue
		}
		current.Hunk.Text += line
		switch kind {
		case ' ':
			current.Before += line[1:]
			current.After += line[1:]
		case '+':
			current.After += line[1:]
			current.Hunk.Additions++
		case '-':
			current.Before += line[1:]
			current.Hunk.Deletions++
		}
		last = kind
	}
	for index := range out {
		if len(out[index].Hunk.Text) > protocol.MAX_DIFF_BYTES {
			return nil, invalid("hunk exceeds the review response limit")
		}
		hash := blake3.New(32, nil)
		var size [8]byte
		for _, value := range []string{path, before, after} {
			binary.LittleEndian.PutUint64(size[:], uint64(len(value)))
			_, _ = hash.Write(size[:])
			_, _ = hash.Write([]byte(value))
		}
		binary.LittleEndian.PutUint64(size[:], uint64(segment))
		_, _ = hash.Write(size[:])
		binary.LittleEndian.PutUint64(size[:], uint64(index))
		_, _ = hash.Write(size[:])
		out[index].Hunk.ID = fmt.Sprintf("%x", hash.Sum(nil))
	}
	return out, nil
}
func ApplyHunkText(current, from, to string) (string, error) {
	if len(current) > protocol.MAX_EDIT_BYTES || !nonnull(current) {
		return "", invalid("hunk target exceeds the text limit")
	}
	start := -1
	if from == "" {
		if current != "" {
			return "", conflict("hunk no longer matches the file")
		}
		start = 0
	} else {
		for offset := 0; offset < len(current); {
			relative := strings.Index(current[offset:], from)
			if relative < 0 {
				break
			}
			at := offset + relative
			end := at + len(from)
			if (at == 0 || current[at-1] == '\n') && (strings.HasSuffix(from, "\n") || end == len(current)) {
				if start >= 0 {
					return "", conflict("hunk matches more than one location")
				}
				start = at
			}
			offset = at + 1
		}
		if start < 0 {
			return "", conflict("hunk no longer matches the file")
		}
	}
	if len(current)-len(from)+len(to) > protocol.MAX_EDIT_BYTES {
		return "", invalid("hunk result exceeds the text limit")
	}
	return current[:start] + to + current[start+len(from):], nil
}
func RejectHunk(ctx context.Context, root string, selection protocol.ConversationCheckpointHunkSelection, change HunkChange) (protocol.ConversationCheckpointHunkApplied, error) {
	out := protocol.ConversationCheckpointHunkApplied{Session: selection.Session, Turn: selection.Turn, Path: selection.Path, Hunk: selection.Hunk}
	if selection.Operation != protocol.ConversationCheckpointHunkOperationReject || selection.ExpectedIndex != nil || selection.ExpectedHead != nil {
		return out, invalid("invalid reject hunk selection")
	}
	fs, err := core.OpenFiles(root, core.RestrictedFiles)
	if err != nil {
		return out, err
	}
	defer fs.Close()
	current, err := fs.CaptureText(ctx, selection.Path, selection.ExpectedRevision)
	if err != nil {
		return out, err
	}
	text, err := ApplyHunkText(current, change.After, change.Before)
	if err != nil {
		return out, err
	}
	written, err := fs.RestoreText(ctx, selection.Path, text, selection.ExpectedRevision)
	out.Revision = written.Revision
	return out, err
}
