package files

import (
	"context"
	"errors"
	"fmt"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	"io/fs"
	"strings"
	"unicode/utf8"
)

type PreparedChange struct {
	Operation        string
	Kind             protocol.ChangeKind
	Path             string
	Destination      *string
	Before           *string
	After            *string
	ExpectedRevision *string
}
type Prepared struct {
	Command  protocol.Command
	Access   core.FileAccess
	Changes  []PreparedChange
	Preview  protocol.ChangePreview
	Rejected *protocol.FileChanges
}

func Prepare(ctx context.Context, root string, command protocol.Command, access core.FileAccess) (*Prepared, error) {
	p := &Prepared{Command: command, Access: access, Preview: protocol.ChangePreview{Files: []protocol.PreviewFile{}}}
	var changes []protocol.FileChange
	switch command.Kind {
	case "write_file":
		v := command.Data.(*protocol.CommandWriteFilePayload)
		if v.ExpectedRevision == nil {
			changes = []protocol.FileChange{{Kind: "create", Data: protocol.FileChangeCreatePayload{Path: v.Path, Text: v.Text}}}
		} else {
			changes = []protocol.FileChange{{Kind: "write", Data: protocol.FileChangeWritePayload{Path: v.Path, Text: v.Text, ExpectedRevision: *v.ExpectedRevision}}}
		}
	case "edit_file":
		v := command.Data.(*protocol.CommandEditFilePayload)
		var edits []protocol.FileEdit
		if v.Edits != nil && v.OldText == nil && v.NewText == nil && !v.ReplaceAll {
			edits = *v.Edits
		} else if v.Edits == nil && v.OldText != nil && v.NewText != nil {
			edits = []protocol.FileEdit{{OldText: *v.OldText, NewText: *v.NewText, ReplaceAll: v.ReplaceAll}}
		} else {
			return nil, invalid("provide either old_text and new_text or an edits array")
		}
		changes = []protocol.FileChange{{Kind: "edit", Data: protocol.FileChangeEditPayload{Path: v.Path, ExpectedRevision: v.ExpectedRevision, Edits: edits}}}
	case "delete_file":
		v := command.Data.(*protocol.CommandDeleteFilePayload)
		changes = []protocol.FileChange{{Kind: "delete", Data: protocol.FileChangeDeletePayload{Path: v.Path, ExpectedRevision: v.ExpectedRevision}}}
	case "move_file":
		v := command.Data.(*protocol.CommandMoveFilePayload)
		changes = []protocol.FileChange{{Kind: "move", Data: protocol.FileChangeMovePayload{Path: v.From, Destination: v.To, ExpectedRevision: v.ExpectedRevision}}}
	case "apply_changes":
		changes = command.Data.(*protocol.CommandApplyChangesPayload).Changes
	case "edit_notebook":
		v := command.Data.(*protocol.CommandEditNotebookPayload)
		f, e := core.OpenFiles(root, access)
		if e != nil {
			return nil, e
		}
		defer f.Close()
		before, e := f.CompleteText(ctx, v.Edit.Path)
		if e != nil {
			return nil, e
		}
		after, e := PrepareNotebook(before, v.Edit)
		if e != nil {
			return nil, e
		}
		if revision([]byte(before)) != v.Edit.ExpectedRevision {
			return nil, conflict("file changed before editing")
		}
		p.Changes = []PreparedChange{{Operation: "edit", Kind: protocol.ChangeKindModified, Path: v.Edit.Path, Before: &before, After: &after, ExpectedRevision: &v.Edit.ExpectedRevision}}
		p.Preview.Files = append(p.Preview.Files, preview(p.Changes[0]))
		return p, nil
	default:
		return nil, invalid("invalid file mutation command")
	}
	if len(changes) == 0 || len(changes) > protocol.MAX_CHANGES {
		return nil, invalid("a change set requires 1 to 16 entries")
	}
	f, err := core.OpenFiles(root, access)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	seen := map[string]bool{}
	budget := 0
	results := protocol.FileChanges{Status: protocol.ChangesStatusRejected, Files: []protocol.ChangedFile{}}
	failed := false
	for _, change := range changes {
		entry, e := prepareChange(ctx, f, change, seen, &budget)
		item := protocol.ChangedFile{Path: entry.Path, Destination: entry.Destination, Kind: entry.Kind, Status: protocol.ChangeStatusNotAttempted}
		if e != nil {
			failed = true
			item.Status = protocol.ChangeStatusFailed
			item.Error = fault(e)
		}
		results.Files = append(results.Files, item)
		p.Changes = append(p.Changes, entry)
		if e == nil {
			p.Preview.Files = append(p.Preview.Files, preview(entry))
		}
	}
	if failed {
		if command.Kind != "apply_changes" {
			for _, item := range results.Files {
				if item.Error != nil {
					return nil, item.Error
				}
			}
		}
		p.Rejected = &results
	}
	return p, nil
}
func prepareChange(ctx context.Context, f *core.Files, change protocol.FileChange, seen map[string]bool, budget *int) (PreparedChange, error) {
	p := PreparedChange{Operation: change.Kind}
	var edits []protocol.FileEdit
	switch change.Kind {
	case "create":
		v, ok := changePayload[protocol.FileChangeCreatePayload](change.Data)
		if !ok {
			return p, invalid("invalid file change")
		}
		p.Kind, p.Path, p.After = protocol.ChangeKindAdded, v.Path, &v.Text
	case "write":
		v, ok := changePayload[protocol.FileChangeWritePayload](change.Data)
		if !ok {
			return p, invalid("invalid file change")
		}
		p.Kind, p.Path, p.After, p.ExpectedRevision = protocol.ChangeKindModified, v.Path, &v.Text, &v.ExpectedRevision
	case "edit":
		v, ok := changePayload[protocol.FileChangeEditPayload](change.Data)
		if !ok {
			return p, invalid("invalid file change")
		}
		p.Kind, p.Path, p.ExpectedRevision, edits = protocol.ChangeKindModified, v.Path, &v.ExpectedRevision, v.Edits
	case "move":
		v, ok := changePayload[protocol.FileChangeMovePayload](change.Data)
		if !ok {
			return p, invalid("invalid file change")
		}
		p.Kind, p.Path, p.Destination, p.ExpectedRevision = protocol.ChangeKindRenamed, v.Path, &v.Destination, &v.ExpectedRevision
	case "delete":
		v, ok := changePayload[protocol.FileChangeDeletePayload](change.Data)
		if !ok {
			return p, invalid("invalid file change")
		}
		p.Kind, p.Path, p.ExpectedRevision = protocol.ChangeKindDeleted, v.Path, &v.ExpectedRevision
	default:
		return p, invalid("invalid file change kind")
	}
	paths := []string{p.Path}
	if p.Destination != nil {
		paths = append(paths, *p.Destination)
	}
	for _, path := range paths {
		if e := f.ValidatePath(path, true); e != nil {
			return p, e
		}
		key := strings.ToLower(path)
		if seen[key] {
			return p, invalid("each path may appear in only one change")
		}
		seen[key] = true
	}
	if p.ExpectedRevision != nil && !validRevision(*p.ExpectedRevision) {
		return p, invalid("expected_revision must be the full file revision")
	}
	if p.Kind == protocol.ChangeKindAdded {
		text, e := PrepareCreation(p.Path, *p.After)
		if e != nil {
			return p, e
		}
		p.After = &text
		handle, e := f.Open(p.Path)
		if e == nil {
			handle.Close()
			return p, conflict("file already exists")
		}
		var faultValue *protocol.Fault
		if !errors.Is(e, fs.ErrNotExist) && (!errors.As(e, &faultValue) || faultValue.Code != protocol.ErrorCodeNotFound) {
			return p, e
		}
	} else if p.Kind == protocol.ChangeKindRenamed || p.Kind == protocol.ChangeKindDeleted {
		handle, e := f.Open(p.Path)
		if e != nil {
			return p, e
		}
		data, e := readBounded(ctx, handle, protocol.MAX_EDIT_BYTES)
		handle.Close()
		if e != nil {
			return p, e
		}
		if revision(data) != *p.ExpectedRevision {
			return p, conflict("file changed before editing")
		}
		if p.Kind == protocol.ChangeKindRenamed {
			target, e := f.Open(*p.Destination)
			if e == nil {
				target.Close()
				return p, conflict("move destination already exists")
			}
			if !errors.Is(e, fs.ErrNotExist) {
				var fv *protocol.Fault
				if !errors.As(e, &fv) || fv.Code != protocol.ErrorCodeNotFound {
					return p, e
				}
			}
		}
		if utf8.Valid(data) && nonnull(string(data)) {
			text := string(data)
			p.Before = &text
		}
	} else {
		before, e := f.CaptureText(ctx, p.Path, *p.ExpectedRevision)
		if e != nil {
			return p, e
		}
		p.Before = &before
		if p.Operation == "write" && len(before) > protocol.MAX_FILE_BYTES {
			return p, invalid("file exceeds the whole-file write limit; edit it instead")
		}
		if p.Operation == "edit" {
			after, e := PrepareEdits(before, edits)
			if e != nil {
				return p, e
			}
			p.After = &after
		}
	}
	if p.After != nil {
		limit := protocol.MAX_EDIT_BYTES
		if p.Operation != "edit" {
			limit = protocol.MAX_FILE_BYTES
		}
		if len(*p.After) > limit || !nonnull(*p.After) {
			return p, invalid("file text exceeds the content limit")
		}
		*budget += len(*p.After)
	}
	if p.Before != nil {
		*budget += len(*p.Before)
	}
	if *budget > protocol.MAX_CHANGE_BYTES {
		return p, invalid("change set exceeds the combined text limit")
	}
	return p, nil
}
func Apply(ctx context.Context, root string, p *Prepared) (protocol.Output, error) {
	if p.Rejected != nil {
		return protocol.Output{Kind: "file_changes", Data: *p.Rejected}, nil
	}
	f, e := core.OpenFiles(root, p.Access)
	if e != nil {
		return protocol.Output{}, e
	}
	defer f.Close()
	result := protocol.FileChanges{Status: protocol.ChangesStatusApplied, Files: []protocol.ChangedFile{}}
	stopped := false
	for _, entry := range p.Changes {
		item := protocol.ChangedFile{Path: entry.Path, Destination: entry.Destination, Kind: entry.Kind, Status: protocol.ChangeStatusNotAttempted}
		var written protocol.FileWritten
		var err error
		if !stopped {
			switch entry.Operation {
			case "create", "write":
				written, err = f.Save(ctx, entry.Path, *entry.After, entry.ExpectedRevision)
			case "edit":
				written, err = f.RestoreText(ctx, entry.Path, *entry.After, *entry.ExpectedRevision)
			case "delete":
				err = f.DeleteFile(ctx, entry.Path, *entry.ExpectedRevision)
			case "move":
				err = f.MoveFile(ctx, entry.Path, *entry.Destination, *entry.ExpectedRevision)
			}
			if err != nil {
				stopped = true
				item.Error = fault(err)
				item.Status = protocol.ChangeStatusFailed
				if item.Error.Code == protocol.ErrorCodeOutcomeUnknown {
					item.Status = protocol.ChangeStatusUnknown
				}
				result.Status = protocol.ChangesStatusRejected
				for _, prior := range result.Files {
					if prior.Status == protocol.ChangeStatusApplied || prior.Status == protocol.ChangeStatusUnknown {
						result.Status = protocol.ChangesStatusPartial
						break
					}
				}
				if item.Status == protocol.ChangeStatusUnknown {
					result.Status = protocol.ChangesStatusPartial
				}
			} else {
				item.Status = protocol.ChangeStatusApplied
				if written.Revision != "" {
					item.Revision = &written.Revision
					item.Size = &written.Size
					item.Excerpt = written.Excerpt
					if item.Excerpt == nil && entry.Before != nil && entry.After != nil {
						item.Excerpt = ChangedExcerpt(*entry.Before, *entry.After)
					}
				}
			}
		}
		result.Files = append(result.Files, item)
	}
	if p.Command.Kind == "apply_changes" {
		return protocol.Output{Kind: "file_changes", Data: result}, nil
	}
	if len(result.Files) != 1 {
		return protocol.Output{}, invalid("invalid prepared change")
	}
	item := result.Files[0]
	if item.Error != nil {
		return protocol.Output{}, item.Error
	}
	switch p.Command.Kind {
	case "delete_file":
		return protocol.Output{Kind: "entry_trashed", Data: protocol.OutputEntryTrashedPayload{Path: item.Path}}, nil
	case "move_file":
		return protocol.Output{Kind: "entry_moved", Data: protocol.OutputEntryMovedPayload{From: item.Path, To: *item.Destination}}, nil
	}
	if item.Revision == nil || item.Size == nil {
		return protocol.Output{}, fmt.Errorf("file write did not return a revision")
	}
	return protocol.Output{Kind: "file_written", Data: protocol.FileWritten{Path: item.Path, Revision: *item.Revision, Size: *item.Size, Excerpt: item.Excerpt}}, nil
}
func preview(p PreparedChange) protocol.PreviewFile {
	out := protocol.PreviewFile{Path: p.Path, Destination: p.Destination, Kind: p.Kind}
	before, after := "", ""
	if p.Before != nil {
		before = *p.Before
	}
	if p.After != nil {
		after = *p.After
	}
	out.Diff, out.Additions, out.Deletions = TextDiff(p.Path, before, after)
	if len(out.Diff) > protocol.MAX_PREVIEW_BYTES {
		out.Diff = out.Diff[:protocol.MAX_PREVIEW_BYTES]
		for !utf8.ValidString(out.Diff) {
			out.Diff = out.Diff[:len(out.Diff)-1]
		}
		out.Truncated = true
	}
	return out
}
