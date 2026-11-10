package git

import (
	"bytes"
	"context"
	"fmt"
	"unicode/utf8"

	"github.com/go-git/go-git/v5/plumbing/filemode"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// ApplyHunk verifies the complete displayed patch before selecting a single
// working-tree hunk. It shares the same confined files and index publisher as
// checkpoint review, without claiming a conversation checkpoint identity.
func (s *Service) ApplyHunk(ctx context.Context, root string, input protocol.CommandApplyGitHunkPayload) (protocol.GitHunkApplied, error) {
	result := protocol.GitHunkApplied{Worktree: input.Worktree, Path: input.Path, Operation: input.Operation, Hunk: input.Hunk, Revision: input.ExpectedRevision}
	if err := input.Validate(); err != nil {
		return result, invalid(err.Error())
	}
	if err := validPath(input.Path); err != nil {
		return result, err
	}
	if err := s.Check(ctx, root, input.ExpectedIndex, input.ExpectedHead, input.ExpectedBranch); err != nil {
		return result, err
	}
	r, err := open(root)
	if err != nil {
		return result, err
	}
	index, err := r.Storer.Index()
	if err != nil {
		return result, mapError(err)
	}
	for _, entry := range index.Entries {
		if entry.Name == input.Path && entry.Stage != 0 {
			return result, fault(protocol.ErrorCodeConflict, "resolve file conflicts before applying a Git hunk")
		}
	}
	fs, err := core.OpenFiles(root, core.RestrictedFiles)
	if err != nil {
		return result, err
	}
	defer fs.Close()
	current, err := fs.CaptureText(ctx, input.Path, input.ExpectedRevision)
	if err != nil {
		return result, err
	}
	before, after, err := s.contents(ctx, root, input.Path, input.Scope)
	if err != nil {
		return result, err
	}
	regular := func(value content) bool {
		return !value.exists || value.mode == filemode.Regular || value.mode == filemode.Executable
	}
	if !after.exists || !regular(before) || !regular(after) || before.binary || after.binary || !utf8.Valid(before.data) || !utf8.Valid(after.data) || bytes.IndexByte(before.data, 0) >= 0 || bytes.IndexByte(after.data, 0) >= 0 {
		return result, invalid("only complete ordinary text files support Git hunk actions")
	}
	if string(after.data) != current {
		return result, conflict()
	}
	diff, err := renderDiff(input.Path, input.Scope, before, after)
	if err != nil {
		return result, err
	}
	if diff.Binary || diff.Truncated {
		return result, invalid("binary or partial diffs do not support Git hunk actions")
	}
	digest := blake3.Sum256([]byte(diff.Text))
	if fmt.Sprintf("%x", digest) != input.DiffRevision {
		return result, conflict()
	}
	hunks, err := files.SplitHunks(input.Path, string(before.data), string(after.data), 0)
	if err != nil {
		return result, err
	}
	if uint64(input.Hunk) >= uint64(len(hunks)) {
		return result, conflict()
	}
	selected := hunks[input.Hunk]
	if err = s.Check(ctx, root, input.ExpectedIndex, input.ExpectedHead, input.ExpectedBranch); err != nil {
		return result, err
	}
	if input.Operation == protocol.GitHunkStage {
		index, err := s.EditIndex(ctx, root, input.Path, input.ExpectedIndex, input.ExpectedHead, input.ExpectedRevision, func(staged string) (string, error) {
			if err := s.Check(ctx, root, input.ExpectedIndex, input.ExpectedHead, input.ExpectedBranch); err != nil {
				return "", err
			}
			return files.ApplyHunkText(staged, selected.Before, selected.After)
		})
		if err != nil {
			return result, err
		}
		result.Index = &index
		return result, nil
	}
	text, err := files.ApplyHunkText(current, selected.After, selected.Before)
	if err != nil {
		return result, err
	}
	written, err := fs.RestoreText(ctx, input.Path, text, input.ExpectedRevision)
	if err != nil {
		return result, err
	}
	result.Revision = written.Revision
	s.Invalidate(root)
	return result, nil
}
