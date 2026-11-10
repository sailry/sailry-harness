package store

import (
	"context"
	"database/sql"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type checkpointSegment struct {
	before *string
	after  string
}
type checkpointVersions struct {
	worktree protocol.WorktreeID
	segments []checkpointSegment
}

func turnCheckpointVersions(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID) (map[string]*checkpointVersions, bool, error) {
	run, err := visibleAgentRun(ctx, db, session, turn)
	if err != nil {
		return nil, false, err
	}
	if run.Status == protocol.ConversationStatusQueued || run.Status == protocol.ConversationStatusRunning || run.Status == protocol.ConversationStatusStopping {
		return nil, false, fault(protocol.ErrorCodeBusy, "turn diff is not final")
	}
	page, err := ListFileCheckpoints(ctx, db, session, turn, nil, 100)
	if err != nil {
		return nil, false, err
	}
	partial := page.Next != nil
	out := map[string]*checkpointVersions{}
	slices.Reverse(page.Files)
	for _, file := range page.Files {
		switch files.CheckpointReceipt(file) {
		case protocol.ConversationCheckpointReceiptWritten:
		case protocol.ConversationCheckpointReceiptPending, protocol.ConversationCheckpointReceiptUncertain:
			partial = true
			continue
		default:
			continue
		}
		size := file.After.Size
		if file.Before != nil {
			size += file.Before.Size
		}
		if size > protocol.MAX_FRAME_BYTES/8 {
			partial = true
			continue
		}
		content, err := CheckpointContent(ctx, db, session, file.ID)
		if err != nil {
			return nil, false, err
		}
		version := out[file.Path]
		if version == nil {
			version = &checkpointVersions{worktree: file.Worktree}
			out[file.Path] = version
		}
		if version.worktree != file.Worktree {
			return nil, false, fault(protocol.ErrorCodeWrongTarget, "turn file changed worktree")
		}
		if len(version.segments) > 0 && content.Before != nil && *content.Before == version.segments[len(version.segments)-1].after {
			version.segments[len(version.segments)-1].after = content.After
		} else {
			version.segments = append(version.segments, checkpointSegment{content.Before, content.After})
		}
	}
	return out, partial, nil
}
func ReadTurnDiff(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID) (protocol.ConversationCheckpointTurnDiff, error) {
	out := protocol.ConversationCheckpointTurnDiff{Session: session, Turn: turn, Files: []protocol.GitDiff{}}
	versions, partial, err := turnCheckpointVersions(ctx, db, session, turn)
	if err != nil {
		return out, err
	}
	out.Partial = partial
	names := make([]string, 0, len(versions))
	for name := range versions {
		names = append(names, name)
	}
	slices.Sort(names)
	budget := protocol.MAX_DIFF_BYTES
	for _, name := range names {
		file := protocol.GitDiff{Path: name, Scope: protocol.GitDiffScopeAll}
		for _, segment := range versions[name].segments {
			before := ""
			if segment.before != nil {
				before = *segment.before
			}
			text, added, deleted := files.TextDiff(name, before, segment.after)
			file.Additions += added
			file.Deletions += deleted
			if len(text) > budget {
				text = text[:budget]
				for !utf8.ValidString(text) {
					text = text[:len(text)-1]
				}
				file.Truncated = true
				out.Partial = true
			}
			budget -= len(text)
			file.Text += text
		}
		if file.Additions > 0 || file.Deletions > 0 {
			out.Files = append(out.Files, file)
		}
	}
	return out, nil
}
func ReadTurnHunks(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID, path string) (protocol.ConversationCheckpointHunkReview, error) {
	out := protocol.ConversationCheckpointHunkReview{Session: session, Turn: turn, Path: path, Hunks: []protocol.ConversationCheckpointHunk{}}
	versions, partial, err := turnCheckpointVersions(ctx, db, session, turn)
	if err != nil {
		return out, err
	}
	file := versions[path]
	if file == nil {
		return out, fault(protocol.ErrorCodeNotFound, "turn file hunk is unavailable")
	}
	out.Worktree, out.Partial = file.worktree, partial
	budget := protocol.MAX_DIFF_BYTES
	for index, segment := range file.segments {
		before := ""
		if segment.before != nil {
			before = *segment.before
		}
		changes, err := files.SplitHunks(path, before, segment.after, index)
		if err != nil {
			out.Partial = true
			continue
		}
		for _, change := range changes {
			if len(change.Hunk.Text) > budget || len(out.Hunks) == 100 {
				out.Partial = true
				continue
			}
			budget -= len(change.Hunk.Text)
			out.Hunks = append(out.Hunks, change.Hunk)
		}
	}
	return out, nil
}
func PrepareTurnHunk(ctx context.Context, db *sql.Conn, selection protocol.ConversationCheckpointHunkSelection, root string) (files.HunkChange, error) {
	empty := files.HunkChange{}
	valid := func(s string) bool { return len(s) == 64 && strings.Trim(s, "0123456789abcdefABCDEF") == "" }
	if !valid(selection.Hunk) || !valid(selection.ExpectedRevision) || selection.Operation == protocol.ConversationCheckpointHunkOperationStage && selection.ExpectedIndex == nil || selection.Operation == protocol.ConversationCheckpointHunkOperationReject && (selection.ExpectedIndex != nil || selection.ExpectedHead != nil) {
		return empty, invalid("invalid turn hunk selection")
	}
	if selection.Operation != protocol.ConversationCheckpointHunkOperationStage && selection.Operation != protocol.ConversationCheckpointHunkOperationReject {
		return empty, invalid("invalid turn hunk operation")
	}
	versions, _, err := turnCheckpointVersions(ctx, db, selection.Session, selection.Turn)
	if err != nil {
		return empty, err
	}
	file := versions[selection.Path]
	if file == nil {
		return empty, fault(protocol.ErrorCodeNotFound, "turn file hunk is unavailable")
	}
	captured, err := ReadWorkspaceRoot(ctx, db, selection.Worktree, false)
	if err != nil {
		return empty, err
	}
	if file.worktree != selection.Worktree || captured != root {
		return empty, fault(protocol.ErrorCodeWrongTarget, "turn hunk belongs to another worktree")
	}
	for index, segment := range file.segments {
		before := ""
		if segment.before != nil {
			before = *segment.before
		}
		changes, err := files.SplitHunks(selection.Path, before, segment.after, index)
		if err != nil {
			continue
		}
		for _, change := range changes {
			if change.Hunk.ID == selection.Hunk {
				return change, nil
			}
		}
	}
	return empty, fault(protocol.ErrorCodeNotFound, "turn file hunk is unavailable")
}
