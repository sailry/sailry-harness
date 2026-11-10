package git

import (
	"context"
	"encoding/binary"
	"encoding/json"
	"errors"
	"io/fs"
	"os"
	"sort"
	"strings"
	"time"
	"unicode/utf8"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/filemode"
	fdiff "github.com/go-git/go-git/v5/plumbing/format/diff"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type resource struct {
	Hash     string `json:"oid,omitempty"`
	Mode     uint32 `json:"mode"`
	Working  bool   `json:"working,omitempty"`
	Size     int64  `json:"size,omitempty"`
	Modified int64  `json:"modified,omitempty"`
	Identity string `json:"identity,omitempty"`
}
type candidate struct {
	entry    protocol.GitChangeEntry
	old, new *resource
}
type changeCapture struct {
	root     string
	snapshot protocol.GitChangeSnapshot
	entries  []candidate
	omitted  uint64
	expires  time.Time
}

func digest(value any) string {
	body, _ := json.Marshal(value)
	sum := blake3.Sum256(body)
	return fmtHash(sum[:])
}
func (s *Service) changeCapture(ctx context.Context, root string, state protocol.GitChangeState) (*changeCapture, error) {
	r, err := open(root)
	if err != nil {
		return nil, err
	}
	old, new := map[string]*resource{}, map[string]*resource{}
	omitted := uint64(0)
	treeInventory := func(revision *string, target map[string]*resource) error {
		if revision == nil {
			return nil
		}
		hash, err := resolve(r, *revision)
		if err != nil {
			return err
		}
		commit, err := r.CommitObject(hash)
		if err != nil {
			return mapError(err)
		}
		tree, err := commit.Tree()
		if err != nil {
			return mapError(err)
		}
		iter := tree.Files()
		defer iter.Close()
		return iter.ForEach(func(file *object.File) error {
			if len(target) == 100000 {
				return fault(protocol.ErrorCodeUnavailable, "Git change inventory exceeds scan limit")
			}
			if validPath(file.Name) != nil {
				omitted++
				return nil
			}
			target[file.Name] = &resource{Hash: file.Hash.String(), Mode: uint32(file.Mode)}
			return checkContext(ctx)
		})
	}
	switch state.Kind {
	case "commit":
		v := data[protocol.GitChangeStateCommitPayload](state.Data)
		if err := treeInventory(v.Parent, old); err != nil {
			return nil, err
		}
		if err := treeInventory(&v.Revision, new); err != nil {
			return nil, err
		}
	case "range":
		v := data[protocol.GitChangeStateRangePayload](state.Data)
		if err := treeInventory(&v.Base, old); err != nil {
			return nil, err
		}
		if err := treeInventory(&v.Head, new); err != nil {
			return nil, err
		}
	case "worktree":
		v := data[protocol.GitChangeStateWorktreePayload](state.Data)
		h, err := head(r)
		if err != nil {
			return nil, err
		}
		index, err := indexRevision(root, r)
		if err != nil {
			return nil, err
		}
		if !sameString(h.Head, v.Head) || index != v.IndexRevision {
			return nil, conflict()
		}
		idx, err := r.Storer.Index()
		if err != nil {
			return nil, mapError(err)
		}
		staged := map[string]*resource{}
		for _, entry := range idx.Entries {
			if entry.Stage != 0 {
				return nil, fault(protocol.ErrorCodeConflict, "resolve index conflicts before reviewing changes")
			}
			if validPath(entry.Name) != nil {
				omitted++
				continue
			}
			staged[entry.Name] = &resource{Hash: entry.Hash.String(), Mode: uint32(entry.Mode)}
		}
		if v.Scope == protocol.GitDiffScopeUnstaged {
			old = staged
		} else if err := treeInventory(v.Head, old); err != nil {
			return nil, err
		}
		if v.Scope == protocol.GitDiffScopeStaged {
			new = staged
		} else {
			paths := map[string]bool{}
			for p := range old {
				paths[p] = true
			}
			for p := range staged {
				paths[p] = true
			}
			filter, err := NewWatchFilter(root)
			if err != nil {
				return nil, err
			}
			anchor, err := os.OpenRoot(root)
			if err != nil {
				return nil, mapError(err)
			}
			err = fs.WalkDir(anchor.FS(), ".", func(relative string, entry fs.DirEntry, walkErr error) error {
				if walkErr != nil {
					omitted++
					if entry != nil && entry.IsDir() {
						return fs.SkipDir
					}
					return nil
				}
				if err := checkContext(ctx); err != nil {
					return err
				}
				if relative == "." {
					return nil
				}
				if validPath(relative) != nil {
					if entry.IsDir() {
						return fs.SkipDir
					}
					return nil
				}
				if filter.matcher != nil && filter.matcher.Match(strings.Split(relative, "/"), entry.IsDir()) {
					if entry.IsDir() {
						return fs.SkipDir
					}
					return nil
				}
				if len(paths) >= 100000 {
					return fault(protocol.ErrorCodeUnavailable, "Git change inventory exceeds scan limit")
				}
				if !entry.IsDir() {
					paths[relative] = true
				}
				return nil
			})
			anchor.Close()
			if err != nil {
				return nil, err
			}
			for path := range paths {
				if err := checkContext(ctx); err != nil {
					return nil, err
				}
				target, e := safePath(root, path)
				if e != nil {
					omitted++
					continue
				}
				info, e := os.Lstat(target)
				if errors.Is(e, os.ErrNotExist) {
					continue
				}
				if e != nil {
					omitted++
					continue
				}
				mode := uint32(filemode.Regular)
				if info.IsDir() {
					mode = uint32(filemode.Submodule)
				} else if info.Mode()&os.ModeSymlink != 0 {
					mode = uint32(filemode.Symlink)
				} else if info.Mode()&0111 != 0 {
					mode = uint32(filemode.Executable)
				}
				new[path] = &resource{Mode: mode, Working: true, Size: info.Size(), Modified: info.ModTime().UnixNano(), Identity: fileIdentity(info)}
				if before := old[path]; before != nil && before.Mode == mode && info.Mode().IsRegular() && info.Size() <= maxFile {
					value, e := workingContent(root, path, maxFile)
					if e == nil && value.hash.String() == before.Hash {
						new[path] = before
					}
				}
			}
		}
	default:
		return nil, invalid("invalid Git change target")
	}
	names := map[string]bool{}
	for p := range old {
		names[p] = true
	}
	for p := range new {
		names[p] = true
	}
	paths := []string{}
	for p := range names {
		paths = append(paths, p)
	}
	sort.Strings(paths)
	capture := &changeCapture{root: root, entries: []candidate{}, omitted: omitted, expires: s.now().Add(5 * time.Second)}
	for _, path := range paths {
		a, b := old[path], new[path]
		if digest(a) == digest(b) {
			continue
		}
		kind := protocol.GitChangeKindModified
		if a == nil {
			kind = protocol.GitChangeKindAdded
		} else if b == nil {
			kind = protocol.GitChangeKindDeleted
		} else if a.Mode&0170000 != b.Mode&0170000 {
			kind = protocol.GitChangeKindTypeChanged
		}
		entry := protocol.GitChangeEntry{Path: path, Kind: kind, MetadataRevision: digest([]any{path, kind, a, b})}
		if a != nil {
			entry.OldMode = &a.Mode
		}
		if b != nil {
			entry.NewMode = &b.Mode
		}
		capture.entries = append(capture.entries, candidate{entry, a, b})
	}
	entries := []protocol.GitChangeEntry{}
	for _, c := range capture.entries {
		entries = append(entries, c.entry)
	}
	capture.snapshot = protocol.GitChangeSnapshot{Target: state, MetadataRevision: digest([]any{state, entries, omitted})}
	return capture, nil
}
func (s *Service) startChanges(ctx context.Context, root string, target protocol.GitChangeTarget) (*changeCapture, error) {
	r, err := open(root)
	if err != nil {
		return nil, err
	}
	state := protocol.GitChangeState{Kind: target.Kind}
	switch target.Kind {
	case "worktree":
		v := data[protocol.GitChangeTargetWorktreePayload](target.Data)
		h, err := head(r)
		if err != nil {
			return nil, err
		}
		index, err := indexRevision(root, r)
		if err != nil {
			return nil, err
		}
		state.Data = protocol.GitChangeStateWorktreePayload{Scope: v.Scope, Head: h.Head, IndexRevision: index}
	case "commit":
		v := data[protocol.GitChangeTargetCommitPayload](target.Data)
		hash, err := resolve(r, v.Revision)
		if err != nil {
			return nil, err
		}
		commit, err := r.CommitObject(hash)
		if err != nil {
			return nil, mapError(err)
		}
		vstate := protocol.GitChangeStateCommitPayload{Revision: hash.String()}
		if commit.NumParents() > 0 {
			vstate.Parent = ptr(commit.ParentHashes[0].String())
		}
		state.Data = vstate
	case "range":
		v := data[protocol.GitChangeTargetRangePayload](target.Data)
		base, err := resolve(r, v.Base)
		if err != nil {
			return nil, err
		}
		head, err := resolve(r, v.Head)
		if err != nil {
			return nil, err
		}
		state.Data = protocol.GitChangeStateRangePayload{Base: base.String(), Head: head.String()}
	default:
		return nil, invalid("invalid Git change target")
	}
	return s.changeCapture(ctx, root, state)
}
func (s *Service) rememberChanges(value *changeCapture) {
	s.mu.Lock()
	defer s.mu.Unlock()
	now := s.now()
	entries := s.changes[:0]
	for _, entry := range s.changes {
		if entry.expires.After(now) && !(entry.root == value.root && entry.snapshot.MetadataRevision == value.snapshot.MetadataRevision) {
			entries = append(entries, entry)
		}
	}
	for len(entries) >= 8 {
		entries = entries[1:]
	}
	s.changes = append(entries, value)
}
func (s *Service) restoreChanges(ctx context.Context, root string, snapshot protocol.GitChangeSnapshot, full bool) (*changeCapture, error) {
	if snapshot.Target.Kind == "worktree" {
		r, err := open(root)
		if err != nil {
			return nil, err
		}
		h, err := head(r)
		if err != nil {
			return nil, err
		}
		revision, err := indexRevision(root, r)
		if err != nil {
			return nil, err
		}
		target := data[protocol.GitChangeStateWorktreePayload](snapshot.Target.Data)
		if !sameString(h.Head, target.Head) || revision != target.IndexRevision {
			return nil, conflict()
		}
	}

	if !full {
		s.mu.Lock()
		for _, entry := range s.changes {
			if entry.root == root && entry.snapshot.MetadataRevision == snapshot.MetadataRevision && entry.expires.After(s.now()) {
				entry.expires = s.now().Add(5 * time.Second)
				s.mu.Unlock()
				if digest(entry.snapshot.Target) != digest(snapshot.Target) {
					return nil, conflict()
				}
				return entry, nil
			}
		}
		s.mu.Unlock()
	}
	capture, err := s.changeCapture(ctx, root, snapshot.Target)
	if err != nil {
		return nil, err
	}
	if capture.snapshot.MetadataRevision != snapshot.MetadataRevision {
		return nil, conflict()
	}
	s.rememberChanges(capture)
	return capture, nil
}
func (s *Service) Changes(ctx context.Context, root string, input protocol.CommandListGitChangesPayload) (protocol.GitChanges, error) {
	result := protocol.GitChanges{Entries: []protocol.GitChangeEntry{}}
	if input.Limit == 0 || (input.Target == nil) == (input.Cursor == nil) {
		return result, invalid("provide one Git change target or cursor and a positive limit")
	}
	var capture *changeCapture
	var err error
	after := ""
	if input.Target != nil {
		capture, err = s.startChanges(ctx, root, *input.Target)
		if err == nil {
			s.rememberChanges(capture)
		}
	} else {
		capture, err = s.restoreChanges(ctx, root, input.Cursor.Snapshot, input.Cursor.AfterPath == nil)
		if input.Cursor.AfterPath != nil {
			after = *input.Cursor.AfterPath
		}
	}
	if err != nil {
		return result, err
	}
	result.Snapshot = capture.snapshot
	result.OmittedPaths = capture.omitted
	found := after == ""
	remaining := false
	for _, candidate := range capture.entries {
		if candidate.entry.Path == after {
			found = true
		}
		if candidate.entry.Path <= after {
			continue
		}
		if uint64(len(result.Entries)) == min(input.Limit, 500) {
			remaining = true
			break
		}
		result.Entries = append(result.Entries, candidate.entry)
		probe := result
		probe.Next = &protocol.GitChangesCursor{Snapshot: result.Snapshot, AfterPath: ptr(candidate.entry.Path)}
		body, _ := json.Marshal(protocol.Output{Kind: "git_changes", Data: probe})
		if len(body) > maxDiff {
			result.Entries = result.Entries[:len(result.Entries)-1]
			remaining = true
			break
		}
	}
	if !found {
		return result, invalid("Git change cursor path is outside the snapshot")
	}
	if remaining {
		if len(result.Entries) == 0 {
			return result, invalid("Git change metadata exceeds the page size limit")
		}
		result.Next = &protocol.GitChangesCursor{Snapshot: result.Snapshot, AfterPath: ptr(result.Entries[len(result.Entries)-1].Path)}
	}
	result.Exhausted = !remaining && result.OmittedPaths == 0
	return result, nil
}
func readResource(root, path string, r *gogit.Repository, value *resource) ([]byte, string, error) {
	if value == nil {
		return nil, "", nil
	}
	if value.Mode == uint32(filemode.Submodule) {
		return nil, "submodule", nil
	}
	if value.Mode == uint32(filemode.Symlink) {
		return nil, "symlink", nil
	}
	if value.Size > maxFile {
		return nil, "too_large", nil
	}
	if !value.Working {
		blob, err := r.BlobObject(plumbing.NewHash(value.Hash))
		if err != nil {
			return nil, "", mapError(err)
		}
		if blob.Size > maxFile {
			return nil, "too_large", nil
		}
		c, e := blobContent(r, plumbing.NewHash(value.Hash), filemode.FileMode(value.Mode), maxFile)
		return c.data, "", e
	}
	target, err := safePath(root, path)
	if err != nil {
		return nil, "", err
	}
	stat, err := os.Lstat(target)
	if err != nil {
		return nil, "", conflict()
	}
	if stat.ModTime().UnixNano() != value.Modified || stat.Size() != value.Size || fileIdentity(stat) != value.Identity {
		return nil, "", conflict()
	}
	c, err := workingContent(root, path, maxFile)
	if err != nil {
		return nil, "", err
	}
	after, err := os.Lstat(target)
	if err != nil || !os.SameFile(stat, after) || after.ModTime() != stat.ModTime() || after.Size() != stat.Size() {
		return nil, "", conflict()
	}
	return c.data, "", nil
}
func (s *Service) changeBytes(ctx context.Context, root string, snapshot protocol.GitChangeSnapshot, path, metadata string) (candidate, []byte, []byte, string, string, error) {
	capture, err := s.restoreChanges(ctx, root, snapshot, false)
	if err != nil {
		return candidate{}, nil, nil, "", "", err
	}
	r, err := open(root)
	if err != nil {
		return candidate{}, nil, nil, "", "", err
	}
	var selected *candidate
	for i := range capture.entries {
		if capture.entries[i].entry.Path == path {
			selected = &capture.entries[i]
			break
		}
	}
	if selected == nil {
		return candidate{}, nil, nil, "", "", invalid("Git change path is outside the snapshot")
	}
	if selected.entry.MetadataRevision != metadata {
		return candidate{}, nil, nil, "", "", conflict()
	}
	a, sa, err := readResource(root, path, r, selected.old)
	if err != nil {
		return candidate{}, nil, nil, "", "", err
	}
	b, sb, err := readResource(root, path, r, selected.new)
	if err != nil {
		return candidate{}, nil, nil, "", "", err
	}
	hash := blake3.New(32, nil)
	hash.Write([]byte(metadata))
	var size [8]byte
	for _, v := range []struct {
		data []byte
		skip string
	}{{a, sa}, {b, sb}} {
		if v.skip != "" {
			hash.Write([]byte(v.skip))
		} else {
			binary.LittleEndian.PutUint64(size[:], uint64(len(v.data)))
			hash.Write(size[:])
			hash.Write(v.data)
		}
	}
	skip := sa
	if skip == "" {
		skip = sb
	}
	if skip == "" {
		if strings.IndexByte(string(a), 0) >= 0 || strings.IndexByte(string(b), 0) >= 0 {
			skip = "binary"
		} else if !utf8.Valid(a) || !utf8.Valid(b) {
			skip = "non_utf8"
		}
	}
	return *selected, a, b, fmtHash(hash.Sum(nil)), skip, nil
}
func diffLines(a, b []byte) []protocol.GitChangeLine {
	all := []protocol.GitChangeLine{}
	old, new := uint32(1), uint32(1)
	changed := []int{}
	for _, chunk := range chunks(a, b) {
		for _, line := range lines(chunk.Content()) {
			item := protocol.GitChangeLine{Text: line}
			switch chunk.Type() {
			case fdiff.Equal:
				item.Kind = protocol.GitChangeLineKindContext
				item.OldLine = ptr(old)
				item.NewLine = ptr(new)
				old++
				new++
			case fdiff.Add:
				item.Kind = protocol.GitChangeLineKindAdded
				item.NewLine = ptr(new)
				new++
				changed = append(changed, len(all))
			case fdiff.Delete:
				item.Kind = protocol.GitChangeLineKindDeleted
				item.OldLine = ptr(old)
				old++
				changed = append(changed, len(all))
			}
			all = append(all, item)
		}
	}
	selected := make([]bool, len(all))
	for _, i := range changed {
		for j := max(0, i-3); j < min(len(all), i+4); j++ {
			selected[j] = true
		}
	}
	result := []protocol.GitChangeLine{}
	for i, line := range all {
		if selected[i] {
			result = append(result, line)
		}
	}
	return result
}
func (s *Service) Change(ctx context.Context, root string, input protocol.CommandReadGitChangePayload) (protocol.GitChange, error) {
	result := protocol.GitChange{Snapshot: input.Snapshot, Path: input.Path, MetadataRevision: input.ExpectedMetadataRevision, Offset: input.Offset, Lines: []protocol.GitChangeLine{}}
	if input.Limit == 0 || input.Offset > 0 && input.ExpectedRevision == nil {
		return result, invalid("Git change continuation requires a positive limit and content revision")
	}
	selected, a, b, revision, skip, err := s.changeBytes(ctx, root, input.Snapshot, input.Path, input.ExpectedMetadataRevision)
	if err != nil {
		return result, err
	}
	result.Revision = revision
	result.OldPath = selected.entry.OldPath
	if input.ExpectedRevision != nil && *input.ExpectedRevision != revision {
		return result, conflict()
	}
	if skip != "" {
		if input.Offset != 0 {
			return result, invalid("skipped Git changes do not have line pages")
		}
		result.SkipReason = &skip
		result.Binary = skip == "binary"
		return result, nil
	}
	inspected := b
	if selected.new == nil {
		inspected = a
	}
	for _, line := range lines(string(inspected)) {
		if strings.TrimSpace(line) != "" {
			first, _ := bounded(strings.TrimLeft(line, " \t"), 256)
			result.FirstLine = &first
			break
		}
	}
	all := diffLines(a, b)
	if input.Offset > uint64(len(all)) {
		return result, invalid("Git change offset is beyond the diff")
	}
	for _, line := range all {
		body, _ := json.Marshal(line)
		if len(body) > maxDiff-2048 {
			result.SkipReason = ptr("line_too_large")
			return result, nil
		}
	}
	for i := input.Offset; i < min(uint64(len(all)), input.Offset+min(input.Limit, 500)); i++ {
		result.Lines = append(result.Lines, all[i])
		result.Next = ptr(i + 1)
		body, _ := json.Marshal(protocol.Output{Kind: "git_change", Data: result})
		if len(body) > maxDiff {
			result.Lines = result.Lines[:len(result.Lines)-1]
			result.Next = ptr(i)
			break
		}
	}
	if result.Next != nil && *result.Next >= uint64(len(all)) {
		result.Next = nil
	}
	return result, nil
}
func (s *Service) Context(ctx context.Context, root string, input protocol.CommandReadGitContextPayload) (protocol.GitContext, error) {
	result := protocol.GitContext{Snapshot: input.Snapshot, Path: input.Path, MetadataRevision: input.ExpectedMetadataRevision, Revision: input.ExpectedRevision, Offset: input.Offset, Side: input.Side}
	if input.Limit == 0 {
		return result, invalid("Git context requires a positive limit")
	}
	selected, a, b, revision, skip, err := s.changeBytes(ctx, root, input.Snapshot, input.Path, input.ExpectedMetadataRevision)
	if err != nil {
		return result, err
	}
	result.OldPath = selected.entry.OldPath
	if revision != input.ExpectedRevision {
		return result, conflict()
	}
	if skip != "" {
		if input.Offset != 0 {
			return result, invalid("skipped Git context does not have line pages")
		}
		result.SkipReason = &skip
		result.Binary = skip == "binary"
		return result, nil
	}
	source := b
	if input.Side == protocol.GitContextSideOld {
		source = a
	} else if input.Side != protocol.GitContextSideNew {
		return result, invalid("invalid Git context side")
	}
	segments := strings.SplitAfter(string(source), "\n")
	if len(segments) > 0 && segments[len(segments)-1] == "" {
		segments = segments[:len(segments)-1]
	}
	if input.Offset > uint64(len(segments)) {
		return result, invalid("Git context offset is beyond the file")
	}
	for i := input.Offset; i < min(uint64(len(segments)), input.Offset+min(input.Limit, 500)); i++ {
		before := result.Text
		result.Text += segments[i]
		result.Next = ptr(i + 1)
		body, _ := json.Marshal(protocol.Output{Kind: "git_context", Data: result})
		if len(body) > maxDiff {
			result.Text = before
			if before == "" {
				result.SkipReason = ptr("line_too_large")
				result.Next = nil
			} else {
				result.Next = ptr(i)
			}
			break
		}
	}
	if result.Next != nil && *result.Next >= uint64(len(segments)) {
		result.Next = nil
	}
	return result, nil
}
