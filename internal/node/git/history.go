package git

import (
	"context"
	"errors"
	"io"
	"sort"
	"strings"
	"unicode/utf8"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/filemode"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func entry(commit *object.Commit, refs map[plumbing.Hash][]string) protocol.GitLogEntry {
	message, partial := bounded(commit.Message, 4096)
	author, a := bounded(commit.Author.Name, 256)
	email, e := bounded(commit.Author.Email, 256)
	labels := refs[commit.Hash]
	if labels == nil {
		labels = []string{}
	}
	return protocol.GitLogEntry{ID: commit.Hash.String(), Message: message, Author: author, Email: email, Timestamp: commit.Author.When.Unix(), References: labels, Truncated: partial || a || e}
}
func references(ctx context.Context, r *gogit.Repository) (map[plumbing.Hash][]string, bool, error) {
	result := map[plumbing.Hash][]string{}
	iter, err := r.References()
	if err != nil {
		return nil, false, mapError(err)
	}
	defer iter.Close()
	count := 0
	truncated := false
	err = iter.ForEach(func(ref *plumbing.Reference) error {
		if err := checkContext(ctx); err != nil {
			return err
		}
		if count == 256 {
			truncated = true
			return io.EOF
		}
		count++
		if !strings.HasPrefix(ref.Name().String(), "refs/") {
			return nil
		}
		hash, err := resolve(r, ref.Name().String())
		if err != nil {
			return nil
		}
		name, partial := bounded(strings.TrimPrefix(ref.Name().String(), "refs/"), 256)
		truncated = truncated || partial
		result[hash] = append(result[hash], name)
		return nil
	})
	if err == io.EOF {
		err = nil
	}
	for hash := range result {
		sort.Strings(result[hash])
	}
	return result, truncated, err
}
func (s *Service) Log(ctx context.Context, root string, input protocol.CommandReadGitLogPayload) (protocol.GitLog, error) {
	result := protocol.GitLog{Kind: protocol.RepositoryKindDirectory, Entries: []protocol.GitLogEntry{}}
	if input.Limit == 0 {
		return result, invalid("Git history limit must be positive")
	}
	limit := min(input.Limit, 100)
	if input.Path != nil {
		if err := validPath(*input.Path); err != nil {
			return result, err
		}
	}
	if input.Cursor != nil && ((input.Path == nil) != (input.Cursor.Path == nil) || input.Path != nil && *input.Path != *input.Cursor.Path) {
		return result, invalid("Git history cursor belongs to another path")
	}
	r, err := open(root)
	if err != nil {
		head, e := s.Head(ctx, root)
		if e == nil && head.Kind == protocol.RepositoryKindDirectory {
			return result, nil
		}
		return result, err
	}
	h, err := head(r)
	if err != nil {
		return result, err
	}
	result.Kind = h.Kind
	if h.Head == nil && input.Revision == nil && input.Cursor == nil {
		return result, nil
	}
	revision := "HEAD"
	if input.Revision != nil {
		revision = *input.Revision
	}
	if input.Cursor != nil {
		revision = input.Cursor.Head
		result.Offset = input.Cursor.Offset
	}
	hash, err := resolve(r, revision)
	if err != nil {
		return result, err
	}
	result.Head = ptr(hash.String())
	result.Kind = protocol.RepositoryKindReady
	refs, truncated, err := references(ctx, r)
	if err != nil {
		return result, err
	}
	result.ReferencesTruncated = truncated
	options := &gogit.LogOptions{From: hash, Order: gogit.LogOrderCommitterTime}
	if input.Path != nil {
		path := *input.Path
		options.PathFilter = func(value string) bool { return value == path }
	}
	iter, err := r.Log(options)
	if err != nil {
		return result, mapError(err)
	}
	defer iter.Close()
	resumed := input.Cursor == nil
	for {
		if err := checkContext(ctx); err != nil {
			return result, err
		}
		commit, err := iter.Next()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return result, mapError(err)
		}
		if !resumed {
			resumed = commit.Hash.String() == input.Cursor.After
			continue
		}
		if uint64(len(result.Entries)) == limit {
			result.Truncated = true
			break
		}
		result.Entries = append(result.Entries, entry(commit, refs))
	}
	if !resumed || len(result.Entries) == 0 && result.Offset > 0 {
		return result, invalid("Git history cursor does not match the traversal")
	}
	if result.Truncated {
		result.Next = &protocol.GitLogCursor{Head: hash.String(), After: result.Entries[len(result.Entries)-1].ID, Offset: result.Offset + uint64(len(result.Entries)), Path: input.Path}
	}
	return result, nil
}
func (s *Service) Commit(ctx context.Context, root, revision string) (protocol.GitCommit, error) {
	r, err := open(root)
	if err != nil {
		return protocol.GitCommit{}, err
	}
	hash, err := resolve(r, revision)
	if err != nil {
		return protocol.GitCommit{}, err
	}
	commit, err := r.CommitObject(hash)
	if err != nil {
		return protocol.GitCommit{}, mapError(err)
	}
	refs, _, err := references(ctx, r)
	if err != nil {
		return protocol.GitCommit{}, err
	}
	result := protocol.GitCommit{Entry: entry(commit, refs), Files: []protocol.GitDiff{}}
	tree, err := commit.Tree()
	if err != nil {
		return result, mapError(err)
	}
	var parentTree *object.Tree
	if commit.NumParents() > 0 {
		parent, e := commit.Parent(0)
		if e != nil {
			return result, mapError(e)
		}
		parentTree, err = parent.Tree()
		if err != nil {
			return result, mapError(err)
		}
	}
	changes, err := object.DiffTreeContext(ctx, parentTree, tree)
	if err != nil {
		return result, mapError(err)
	}
	bytes := 0
	for _, change := range changes {
		if len(result.Files) == maxEntries {
			result.Truncated = true
			break
		}
		path := change.To.Name
		if path == "" {
			path = change.From.Name
		}
		if validPath(path) != nil {
			result.Truncated = true
			continue
		}
		a, b := content{}, content{}
		if change.From.Name != "" {
			a, err = blobContent(r, change.From.TreeEntry.Hash, change.From.TreeEntry.Mode, 1024*1024)
		}
		if err == nil && change.To.Name != "" {
			b, err = blobContent(r, change.To.TreeEntry.Hash, change.To.TreeEntry.Mode, 1024*1024)
		}
		if err != nil {
			return result, err
		}
		diff, err := renderDiff(path, protocol.GitDiffScopeAll, a, b)
		if err != nil {
			return result, err
		}
		if bytes+len(diff.Text) > maxDiff {
			diff.Text, diff.Truncated = bounded(diff.Text, maxDiff-bytes)
		}
		bytes += len(diff.Text)
		result.Files = append(result.Files, diff)
		result.Additions += diff.Additions
		result.Deletions += diff.Deletions
		result.Binary = result.Binary || diff.Binary
		result.Truncated = result.Truncated || diff.Truncated
		if bytes == maxDiff {
			result.Truncated = true
			break
		}
	}
	return result, nil
}
func committedFile(r *gogit.Repository, hash plumbing.Hash, path string) (content, error) {
	if err := validPath(path); err != nil {
		return content{}, err
	}
	commit, err := r.CommitObject(hash)
	if err != nil {
		return content{}, mapError(err)
	}
	tree, err := commit.Tree()
	if err != nil {
		return content{}, mapError(err)
	}
	value, err := treeContent(r, tree, path, maxFile)
	if err != nil {
		return value, err
	}
	if !value.exists {
		return value, fault(protocol.ErrorCodeNotFound, "Git file does not exist")
	}
	if value.mode != filemode.Regular && value.mode != filemode.Executable {
		return value, invalid("Git inspection requires a committed regular file")
	}
	return value, nil
}
func lines(text string) []string {
	if text == "" {
		return nil
	}
	result := strings.Split(strings.TrimSuffix(text, "\n"), "\n")
	for i := range result {
		result[i] = strings.TrimSuffix(result[i], "\r")
	}
	return result
}
func (s *Service) Show(ctx context.Context, root string, input protocol.CommandReadGitShowPayload) (protocol.Output, error) {
	if input.Limit == 0 {
		return protocol.Output{}, invalid("Git line page limit must be positive")
	}
	if input.Path == nil {
		if input.Offset != 0 {
			return protocol.Output{}, invalid("commit patches do not accept a line offset")
		}
		value, err := s.Commit(ctx, root, input.Revision)
		return protocol.Output{Kind: "git_commit", Data: value}, err
	}
	r, err := open(root)
	if err != nil {
		return protocol.Output{}, err
	}
	hash, err := resolve(r, input.Revision)
	if err != nil {
		return protocol.Output{}, err
	}
	value, err := committedFile(r, hash, *input.Path)
	if err != nil {
		return protocol.Output{}, err
	}
	result := protocol.GitFile{Commit: hash.String(), Path: *input.Path, Offset: input.Offset}
	result.Binary = !utf8.Valid(value.data) || strings.IndexByte(string(value.data), 0) >= 0
	if result.Binary {
		if input.Offset != 0 {
			return protocol.Output{}, invalid("binary Git files do not accept a line offset")
		}
		return protocol.Output{Kind: "git_file", Data: result}, nil
	}
	source := lines(string(value.data))
	result.TotalLines = uint64(len(source))
	if input.Offset > result.TotalLines {
		return protocol.Output{}, invalid("Git line offset is out of range")
	}
	for i := input.Offset; i < min(result.TotalLines, input.Offset+min(input.Limit, 2000)); i++ {
		if err := checkContext(ctx); err != nil {
			return protocol.Output{}, err
		}
		prefix := fmtLine(i + 1)
		budget := maxDiff - len(result.Text) - len(prefix) - 1
		if budget <= 0 {
			break
		}
		if len(source[i]) > budget && result.Text != "" {
			break
		}
		text, partial := bounded(source[i], budget)
		result.Text += prefix + text + "\n"
		if partial {
			result.TruncatedLine = ptr(i + 1)
			break
		}
	}
	rendered := uint64(strings.Count(result.Text, "\n"))
	if input.Offset+rendered < result.TotalLines {
		result.Next = ptr(input.Offset + rendered)
	}
	result.Truncated = result.Next != nil || result.TruncatedLine != nil
	return protocol.Output{Kind: "git_file", Data: result}, nil
}
