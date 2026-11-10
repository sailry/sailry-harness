// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type GitChangeTarget struct {
	Kind string
	Data any
}
type GitChangeTargetWorktreePayload struct {
	Scope GitDiffScope `json:"scope"`
}

func DefaultGitChangeTargetWorktreePayload() GitChangeTargetWorktreePayload {
	return GitChangeTargetWorktreePayload{
		Scope: GitDiffScope(""),
	}
}
func (value *GitChangeTargetWorktreePayload) UnmarshalJSON(data []byte) error {
	type plain GitChangeTargetWorktreePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"scope"}, true); err != nil {
		return err
	}
	*value = GitChangeTargetWorktreePayload(decoded)
	return nil
}
func (value GitChangeTargetWorktreePayload) MarshalJSON() ([]byte, error) {
	type plain GitChangeTargetWorktreePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitChangeTargetCommitPayload struct {
	Revision string `json:"revision"`
}

func DefaultGitChangeTargetCommitPayload() GitChangeTargetCommitPayload {
	return GitChangeTargetCommitPayload{
		Revision: "",
	}
}
func (value *GitChangeTargetCommitPayload) UnmarshalJSON(data []byte) error {
	type plain GitChangeTargetCommitPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision"}, true); err != nil {
		return err
	}
	*value = GitChangeTargetCommitPayload(decoded)
	return nil
}
func (value GitChangeTargetCommitPayload) MarshalJSON() ([]byte, error) {
	type plain GitChangeTargetCommitPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitChangeTargetRangePayload struct {
	Base string `json:"base"`
	Head string `json:"head"`
}

func DefaultGitChangeTargetRangePayload() GitChangeTargetRangePayload {
	return GitChangeTargetRangePayload{
		Base: "",
		Head: "",
	}
}
func (value *GitChangeTargetRangePayload) UnmarshalJSON(data []byte) error {
	type plain GitChangeTargetRangePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"base", "head"}, true); err != nil {
		return err
	}
	*value = GitChangeTargetRangePayload(decoded)
	return nil
}
func (value GitChangeTargetRangePayload) MarshalJSON() ([]byte, error) {
	type plain GitChangeTargetRangePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var gitChangeTargetVariants = variantFactories{"worktree": func() any { return new(GitChangeTargetWorktreePayload) },
	"commit": func() any { return new(GitChangeTargetCommitPayload) },
	"range":  func() any { return new(GitChangeTargetRangePayload) }}

func (value GitChangeTarget) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, gitChangeTargetVariants)
}
func (value *GitChangeTarget) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", gitChangeTargetVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type GitChangeState struct {
	Kind string
	Data any
}
type GitChangeStateWorktreePayload struct {
	Scope         GitDiffScope `json:"scope"`
	Head          *string      `json:"head"`
	IndexRevision string       `json:"index_revision"`
}

func DefaultGitChangeStateWorktreePayload() GitChangeStateWorktreePayload {
	return GitChangeStateWorktreePayload{
		Scope:         GitDiffScope(""),
		Head:          nil,
		IndexRevision: "",
	}
}
func (value *GitChangeStateWorktreePayload) UnmarshalJSON(data []byte) error {
	type plain GitChangeStateWorktreePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"scope", "index_revision"}, true); err != nil {
		return err
	}
	*value = GitChangeStateWorktreePayload(decoded)
	return nil
}
func (value GitChangeStateWorktreePayload) MarshalJSON() ([]byte, error) {
	type plain GitChangeStateWorktreePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitChangeStateCommitPayload struct {
	Revision string  `json:"revision"`
	Parent   *string `json:"parent"`
}

func DefaultGitChangeStateCommitPayload() GitChangeStateCommitPayload {
	return GitChangeStateCommitPayload{
		Revision: "",
		Parent:   nil,
	}
}
func (value *GitChangeStateCommitPayload) UnmarshalJSON(data []byte) error {
	type plain GitChangeStateCommitPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision"}, true); err != nil {
		return err
	}
	*value = GitChangeStateCommitPayload(decoded)
	return nil
}
func (value GitChangeStateCommitPayload) MarshalJSON() ([]byte, error) {
	type plain GitChangeStateCommitPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitChangeStateRangePayload struct {
	Base string `json:"base"`
	Head string `json:"head"`
}

func DefaultGitChangeStateRangePayload() GitChangeStateRangePayload {
	return GitChangeStateRangePayload{
		Base: "",
		Head: "",
	}
}
func (value *GitChangeStateRangePayload) UnmarshalJSON(data []byte) error {
	type plain GitChangeStateRangePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"base", "head"}, true); err != nil {
		return err
	}
	*value = GitChangeStateRangePayload(decoded)
	return nil
}
func (value GitChangeStateRangePayload) MarshalJSON() ([]byte, error) {
	type plain GitChangeStateRangePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var gitChangeStateVariants = variantFactories{"worktree": func() any { return new(GitChangeStateWorktreePayload) },
	"commit": func() any { return new(GitChangeStateCommitPayload) },
	"range":  func() any { return new(GitChangeStateRangePayload) }}

func (value GitChangeState) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, gitChangeStateVariants)
}
func (value *GitChangeState) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", gitChangeStateVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type GitChangeSnapshot struct {
	Target           GitChangeState `json:"target"`
	MetadataRevision string         `json:"metadata_revision"`
}

func DefaultGitChangeSnapshot() GitChangeSnapshot {
	return GitChangeSnapshot{
		Target:           GitChangeState{},
		MetadataRevision: "",
	}
}
func (value *GitChangeSnapshot) UnmarshalJSON(data []byte) error {
	type plain GitChangeSnapshot
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"target", "metadata_revision"}, true); err != nil {
		return err
	}
	*value = GitChangeSnapshot(decoded)
	return nil
}
func (value GitChangeSnapshot) MarshalJSON() ([]byte, error) {
	type plain GitChangeSnapshot
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitChangesCursor struct {
	Snapshot  GitChangeSnapshot `json:"snapshot"`
	AfterPath *string           `json:"after_path"`
}

func DefaultGitChangesCursor() GitChangesCursor {
	return GitChangesCursor{
		Snapshot:  DefaultGitChangeSnapshot(),
		AfterPath: nil,
	}
}
func (value *GitChangesCursor) UnmarshalJSON(data []byte) error {
	type plain GitChangesCursor
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"snapshot"}, true); err != nil {
		return err
	}
	*value = GitChangesCursor(decoded)
	return nil
}
func (value GitChangesCursor) MarshalJSON() ([]byte, error) {
	type plain GitChangesCursor
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitChangeEntry struct {
	Path             string        `json:"path"`
	OldPath          *string       `json:"old_path"`
	Kind             GitChangeKind `json:"kind"`
	OldMode          *uint32       `json:"old_mode"`
	NewMode          *uint32       `json:"new_mode"`
	MetadataRevision string        `json:"metadata_revision"`
}

func DefaultGitChangeEntry() GitChangeEntry {
	return GitChangeEntry{
		Path:             "",
		OldPath:          nil,
		Kind:             GitChangeKind(""),
		OldMode:          nil,
		NewMode:          nil,
		MetadataRevision: "",
	}
}
func (value *GitChangeEntry) UnmarshalJSON(data []byte) error {
	type plain GitChangeEntry
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "kind", "metadata_revision"}, false); err != nil {
		return err
	}
	*value = GitChangeEntry(decoded)
	return nil
}
func (value GitChangeEntry) MarshalJSON() ([]byte, error) {
	type plain GitChangeEntry
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitChanges struct {
	Snapshot     GitChangeSnapshot `json:"snapshot"`
	Entries      []GitChangeEntry  `json:"entries"`
	Next         *GitChangesCursor `json:"next"`
	Exhausted    bool              `json:"exhausted"`
	OmittedPaths uint64            `json:"omitted_paths"`
}

func DefaultGitChanges() GitChanges {
	return GitChanges{
		Snapshot:     DefaultGitChangeSnapshot(),
		Entries:      []GitChangeEntry{},
		Next:         nil,
		Exhausted:    false,
		OmittedPaths: 0,
	}
}
func (value *GitChanges) UnmarshalJSON(data []byte) error {
	type plain GitChanges
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"snapshot", "entries", "exhausted", "omitted_paths"}, false); err != nil {
		return err
	}
	*value = GitChanges(decoded)
	return nil
}
func (value GitChanges) MarshalJSON() ([]byte, error) {
	type plain GitChanges
	encoded := plain(value)
	encoded.Entries = normalizeCollections(encoded.Entries)
	return json.Marshal(encoded)
}

type GitChangeLineKind string

const (
	GitChangeLineKindAdded   GitChangeLineKind = "added"
	GitChangeLineKindDeleted GitChangeLineKind = "deleted"
	GitChangeLineKindContext GitChangeLineKind = "context"
)

func (value GitChangeLineKind) MarshalJSON() ([]byte, error) {
	switch value {
	case GitChangeLineKindAdded, GitChangeLineKindDeleted, GitChangeLineKindContext:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown GitChangeLineKind %q", value)
	}
}
func (value *GitChangeLineKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch GitChangeLineKind(decoded) {
	case GitChangeLineKindAdded, GitChangeLineKindDeleted, GitChangeLineKindContext:
		*value = GitChangeLineKind(decoded)
		return nil
	default:
		return fmt.Errorf("unknown GitChangeLineKind %q", decoded)
	}
}
func (value GitChangeLineKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *GitChangeLineKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type GitChangeLine struct {
	Kind    GitChangeLineKind `json:"kind"`
	OldLine *uint32           `json:"old_line"`
	NewLine *uint32           `json:"new_line"`
	Text    string            `json:"text"`
}

func DefaultGitChangeLine() GitChangeLine {
	return GitChangeLine{
		Kind:    GitChangeLineKind(""),
		OldLine: nil,
		NewLine: nil,
		Text:    "",
	}
}
func (value *GitChangeLine) UnmarshalJSON(data []byte) error {
	type plain GitChangeLine
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"kind", "text"}, false); err != nil {
		return err
	}
	*value = GitChangeLine(decoded)
	return nil
}
func (value GitChangeLine) MarshalJSON() ([]byte, error) {
	type plain GitChangeLine
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitChange struct {
	Snapshot         GitChangeSnapshot `json:"snapshot"`
	Path             string            `json:"path"`
	OldPath          *string           `json:"old_path"`
	MetadataRevision string            `json:"metadata_revision"`
	Revision         string            `json:"revision"`
	Offset           uint64            `json:"offset"`
	Next             *uint64           `json:"next"`
	Lines            []GitChangeLine   `json:"lines"`
	Binary           bool              `json:"binary"`
	SkipReason       *string           `json:"skip_reason"`
	FirstLine        *string           `json:"first_line"`
}

func DefaultGitChange() GitChange {
	return GitChange{
		Snapshot:         DefaultGitChangeSnapshot(),
		Path:             "",
		OldPath:          nil,
		MetadataRevision: "",
		Revision:         "",
		Offset:           0,
		Next:             nil,
		Lines:            []GitChangeLine{},
		Binary:           false,
		SkipReason:       nil,
		FirstLine:        nil,
	}
}
func (value *GitChange) UnmarshalJSON(data []byte) error {
	type plain GitChange
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"snapshot", "path", "metadata_revision", "revision", "offset", "lines", "binary"}, false); err != nil {
		return err
	}
	*value = GitChange(decoded)
	return nil
}
func (value GitChange) MarshalJSON() ([]byte, error) {
	type plain GitChange
	encoded := plain(value)
	encoded.Lines = normalizeCollections(encoded.Lines)
	return json.Marshal(encoded)
}

type GitContextSide string

const (
	GitContextSideOld GitContextSide = "old"
	GitContextSideNew GitContextSide = "new"
)

func (value GitContextSide) MarshalJSON() ([]byte, error) {
	switch value {
	case GitContextSideOld, GitContextSideNew:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown GitContextSide %q", value)
	}
}
func (value *GitContextSide) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch GitContextSide(decoded) {
	case GitContextSideOld, GitContextSideNew:
		*value = GitContextSide(decoded)
		return nil
	default:
		return fmt.Errorf("unknown GitContextSide %q", decoded)
	}
}
func (value GitContextSide) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *GitContextSide) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type GitContext struct {
	Snapshot         GitChangeSnapshot `json:"snapshot"`
	Path             string            `json:"path"`
	OldPath          *string           `json:"old_path"`
	MetadataRevision string            `json:"metadata_revision"`
	Revision         string            `json:"revision"`
	Side             GitContextSide    `json:"side"`
	Offset           uint64            `json:"offset"`
	Next             *uint64           `json:"next"`
	Text             string            `json:"text"`
	Binary           bool              `json:"binary"`
	SkipReason       *string           `json:"skip_reason"`
}

func DefaultGitContext() GitContext {
	return GitContext{
		Snapshot:         DefaultGitChangeSnapshot(),
		Path:             "",
		OldPath:          nil,
		MetadataRevision: "",
		Revision:         "",
		Side:             GitContextSide(""),
		Offset:           0,
		Next:             nil,
		Text:             "",
		Binary:           false,
		SkipReason:       nil,
	}
}
func (value *GitContext) UnmarshalJSON(data []byte) error {
	type plain GitContext
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"snapshot", "path", "metadata_revision", "revision", "side", "offset", "text", "binary"}, true); err != nil {
		return err
	}
	*value = GitContext(decoded)
	return nil
}
func (value GitContext) MarshalJSON() ([]byte, error) {
	type plain GitContext
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitFile struct {
	Commit        string  `json:"commit"`
	Path          string  `json:"path"`
	Offset        uint64  `json:"offset"`
	Next          *uint64 `json:"next"`
	TotalLines    uint64  `json:"total_lines"`
	Text          string  `json:"text"`
	Binary        bool    `json:"binary"`
	Truncated     bool    `json:"truncated"`
	TruncatedLine *uint64 `json:"truncated_line"`
}

func DefaultGitFile() GitFile {
	return GitFile{
		Commit:        "",
		Path:          "",
		Offset:        0,
		Next:          nil,
		TotalLines:    0,
		Text:          "",
		Binary:        false,
		Truncated:     false,
		TruncatedLine: nil,
	}
}
func (value *GitFile) UnmarshalJSON(data []byte) error {
	type plain GitFile
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"commit", "path", "offset", "total_lines", "text", "binary", "truncated"}, false); err != nil {
		return err
	}
	*value = GitFile(decoded)
	return nil
}
func (value GitFile) MarshalJSON() ([]byte, error) {
	type plain GitFile
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitBlame struct {
	Commit     string         `json:"commit"`
	Path       string         `json:"path"`
	Offset     uint64         `json:"offset"`
	Next       *uint64        `json:"next"`
	TotalLines uint64         `json:"total_lines"`
	Entries    []GitBlameLine `json:"entries"`
	Truncated  bool           `json:"truncated"`
}

func DefaultGitBlame() GitBlame {
	return GitBlame{
		Commit:     "",
		Path:       "",
		Offset:     0,
		Next:       nil,
		TotalLines: 0,
		Entries:    []GitBlameLine{},
		Truncated:  false,
	}
}
func (value *GitBlame) UnmarshalJSON(data []byte) error {
	type plain GitBlame
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"commit", "path", "offset", "total_lines", "entries", "truncated"}, false); err != nil {
		return err
	}
	*value = GitBlame(decoded)
	return nil
}
func (value GitBlame) MarshalJSON() ([]byte, error) {
	type plain GitBlame
	encoded := plain(value)
	encoded.Entries = normalizeCollections(encoded.Entries)
	return json.Marshal(encoded)
}

type GitBlameLine struct {
	Line         uint64 `json:"line"`
	OriginalLine uint64 `json:"original_line"`
	Commit       string `json:"commit"`
	Author       string `json:"author"`
	Email        string `json:"email"`
	Timestamp    int64  `json:"timestamp"`
	Text         string `json:"text"`
	Truncated    bool   `json:"truncated"`
}

func DefaultGitBlameLine() GitBlameLine {
	return GitBlameLine{
		Line:         0,
		OriginalLine: 0,
		Commit:       "",
		Author:       "",
		Email:        "",
		Timestamp:    0,
		Text:         "",
		Truncated:    false,
	}
}
func (value *GitBlameLine) UnmarshalJSON(data []byte) error {
	type plain GitBlameLine
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"line", "original_line", "commit", "author", "email", "timestamp", "text", "truncated"}, false); err != nil {
		return err
	}
	*value = GitBlameLine(decoded)
	return nil
}
func (value GitBlameLine) MarshalJSON() ([]byte, error) {
	type plain GitBlameLine
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitWorktree struct {
	Path      string  `json:"path"`
	Branch    *string `json:"branch"`
	Head      *string `json:"head"`
	Main      bool    `json:"main"`
	Locked    bool    `json:"locked"`
	Available bool    `json:"available"`
}

func DefaultGitWorktree() GitWorktree {
	return GitWorktree{
		Path:      "",
		Branch:    nil,
		Head:      nil,
		Main:      false,
		Locked:    false,
		Available: false,
	}
}
func (value *GitWorktree) UnmarshalJSON(data []byte) error {
	type plain GitWorktree
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "main", "locked", "available"}, false); err != nil {
		return err
	}
	*value = GitWorktree(decoded)
	return nil
}
func (value GitWorktree) MarshalJSON() ([]byte, error) {
	type plain GitWorktree
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitWorktrees struct {
	Kind         RepositoryKind `json:"kind"`
	Entries      []GitWorktree  `json:"entries"`
	Truncated    bool           `json:"truncated"`
	OmittedPaths uint64         `json:"omitted_paths"`
}

func DefaultGitWorktrees() GitWorktrees {
	return GitWorktrees{
		Kind:         RepositoryKind(""),
		Entries:      []GitWorktree{},
		Truncated:    false,
		OmittedPaths: 0,
	}
}
func (value *GitWorktrees) UnmarshalJSON(data []byte) error {
	type plain GitWorktrees
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"kind", "entries", "truncated", "omitted_paths"}, false); err != nil {
		return err
	}
	*value = GitWorktrees(decoded)
	return nil
}
func (value GitWorktrees) MarshalJSON() ([]byte, error) {
	type plain GitWorktrees
	encoded := plain(value)
	encoded.Entries = normalizeCollections(encoded.Entries)
	return json.Marshal(encoded)
}

type GitBranch struct {
	Name    string `json:"name"`
	Commit  string `json:"commit"`
	Remote  bool   `json:"remote"`
	Current bool   `json:"current"`
}

func DefaultGitBranch() GitBranch {
	return GitBranch{
		Name:    "",
		Commit:  "",
		Remote:  false,
		Current: false,
	}
}
func (value *GitBranch) UnmarshalJSON(data []byte) error {
	type plain GitBranch
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "commit", "remote", "current"}, false); err != nil {
		return err
	}
	*value = GitBranch(decoded)
	return nil
}
func (value GitBranch) MarshalJSON() ([]byte, error) {
	type plain GitBranch
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitBranches struct {
	Sync         GitSync        `json:"sync"`
	Kind         RepositoryKind `json:"kind"`
	Current      *string        `json:"current"`
	Entries      []GitBranch    `json:"entries"`
	Truncated    bool           `json:"truncated"`
	OmittedNames uint64         `json:"omitted_names"`
}

func DefaultGitBranches() GitBranches {
	return GitBranches{
		Sync:         DefaultGitSync(),
		Kind:         RepositoryKind(""),
		Current:      nil,
		Entries:      []GitBranch{},
		Truncated:    false,
		OmittedNames: 0,
	}
}
func (value *GitBranches) UnmarshalJSON(data []byte) error {
	type plain GitBranches
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"sync", "kind", "entries", "truncated", "omitted_names"}, false); err != nil {
		return err
	}
	*value = GitBranches(decoded)
	return nil
}
func (value GitBranches) MarshalJSON() ([]byte, error) {
	type plain GitBranches
	encoded := plain(value)
	encoded.Entries = normalizeCollections(encoded.Entries)
	return json.Marshal(encoded)
}

type GitMergeKind string

const (
	GitMergeKindUpToDate    GitMergeKind = "up_to_date"
	GitMergeKindFastForward GitMergeKind = "fast_forward"
	GitMergeKindMergeCommit GitMergeKind = "merge_commit"
)

func (value GitMergeKind) MarshalJSON() ([]byte, error) {
	switch value {
	case GitMergeKindUpToDate, GitMergeKindFastForward, GitMergeKindMergeCommit:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown GitMergeKind %q", value)
	}
}
func (value *GitMergeKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch GitMergeKind(decoded) {
	case GitMergeKindUpToDate, GitMergeKindFastForward, GitMergeKindMergeCommit:
		*value = GitMergeKind(decoded)
		return nil
	default:
		return fmt.Errorf("unknown GitMergeKind %q", decoded)
	}
}
func (value GitMergeKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *GitMergeKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type GitMerge struct {
	Kind   GitMergeKind `json:"kind"`
	Commit string       `json:"commit"`
}

func DefaultGitMerge() GitMerge {
	return GitMerge{
		Kind:   GitMergeKind(""),
		Commit: "",
	}
}
func (value *GitMerge) UnmarshalJSON(data []byte) error {
	type plain GitMerge
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"kind", "commit"}, false); err != nil {
		return err
	}
	*value = GitMerge(decoded)
	return nil
}
func (value GitMerge) MarshalJSON() ([]byte, error) {
	type plain GitMerge
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitLogEntry struct {
	ID         string   `json:"id"`
	Message    string   `json:"message"`
	Author     string   `json:"author"`
	Email      string   `json:"email"`
	Timestamp  int64    `json:"timestamp"`
	References []string `json:"references"`
	Truncated  bool     `json:"truncated"`
}

func DefaultGitLogEntry() GitLogEntry {
	return GitLogEntry{
		ID:         "",
		Message:    "",
		Author:     "",
		Email:      "",
		Timestamp:  0,
		References: []string{},
		Truncated:  false,
	}
}
func (value *GitLogEntry) UnmarshalJSON(data []byte) error {
	type plain GitLogEntry
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "message", "author", "email", "timestamp", "references", "truncated"}, false); err != nil {
		return err
	}
	*value = GitLogEntry(decoded)
	return nil
}
func (value GitLogEntry) MarshalJSON() ([]byte, error) {
	type plain GitLogEntry
	encoded := plain(value)
	encoded.References = normalizeCollections(encoded.References)
	return json.Marshal(encoded)
}

type GitLogCursor struct {
	Head   string  `json:"head"`
	After  string  `json:"after"`
	Offset uint64  `json:"offset"`
	Path   *string `json:"path,omitempty"`
}

func DefaultGitLogCursor() GitLogCursor {
	return GitLogCursor{
		Head:   "",
		After:  "",
		Offset: 0,
		Path:   nil,
	}
}
func (value *GitLogCursor) UnmarshalJSON(data []byte) error {
	type plain GitLogCursor
	decoded := plain{}
	decoded.Path = nil
	if err := DecodeObject(data, &decoded, []string{"head", "after", "offset"}, true); err != nil {
		return err
	}
	*value = GitLogCursor(decoded)
	return nil
}
func (value GitLogCursor) MarshalJSON() ([]byte, error) {
	type plain GitLogCursor
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitLog struct {
	Kind                RepositoryKind `json:"kind"`
	Head                *string        `json:"head"`
	Offset              uint64         `json:"offset"`
	Next                *GitLogCursor  `json:"next"`
	Entries             []GitLogEntry  `json:"entries"`
	Truncated           bool           `json:"truncated"`
	ReferencesTruncated bool           `json:"references_truncated"`
}

func DefaultGitLog() GitLog {
	return GitLog{
		Kind:                RepositoryKind(""),
		Head:                nil,
		Offset:              0,
		Next:                nil,
		Entries:             []GitLogEntry{},
		Truncated:           false,
		ReferencesTruncated: false,
	}
}
func (value *GitLog) UnmarshalJSON(data []byte) error {
	type plain GitLog
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"kind", "offset", "entries", "truncated", "references_truncated"}, false); err != nil {
		return err
	}
	*value = GitLog(decoded)
	return nil
}
func (value GitLog) MarshalJSON() ([]byte, error) {
	type plain GitLog
	encoded := plain(value)
	encoded.Entries = normalizeCollections(encoded.Entries)
	return json.Marshal(encoded)
}

type GitCommit struct {
	Entry     GitLogEntry `json:"entry"`
	Files     []GitDiff   `json:"files"`
	Additions uint64      `json:"additions"`
	Deletions uint64      `json:"deletions"`
	Binary    bool        `json:"binary"`
	Truncated bool        `json:"truncated"`
}

func DefaultGitCommit() GitCommit {
	return GitCommit{
		Entry:     DefaultGitLogEntry(),
		Files:     []GitDiff{},
		Additions: 0,
		Deletions: 0,
		Binary:    false,
		Truncated: false,
	}
}
func (value *GitCommit) UnmarshalJSON(data []byte) error {
	type plain GitCommit
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"entry", "files", "additions", "deletions", "binary", "truncated"}, false); err != nil {
		return err
	}
	*value = GitCommit(decoded)
	return nil
}
func (value GitCommit) MarshalJSON() ([]byte, error) {
	type plain GitCommit
	encoded := plain(value)
	encoded.Files = normalizeCollections(encoded.Files)
	return json.Marshal(encoded)
}

type RepositoryKind string

const (
	RepositoryKindDirectory RepositoryKind = "directory"
	RepositoryKindUnborn    RepositoryKind = "unborn"
	RepositoryKindReady     RepositoryKind = "ready"
)

func (value RepositoryKind) MarshalJSON() ([]byte, error) {
	switch value {
	case RepositoryKindDirectory, RepositoryKindUnborn, RepositoryKindReady:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown RepositoryKind %q", value)
	}
}
func (value *RepositoryKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch RepositoryKind(decoded) {
	case RepositoryKindDirectory, RepositoryKindUnborn, RepositoryKindReady:
		*value = RepositoryKind(decoded)
		return nil
	default:
		return fmt.Errorf("unknown RepositoryKind %q", decoded)
	}
}
func (value RepositoryKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *RepositoryKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type GitChangeKind string

const (
	GitChangeKindAdded       GitChangeKind = "added"
	GitChangeKindModified    GitChangeKind = "modified"
	GitChangeKindDeleted     GitChangeKind = "deleted"
	GitChangeKindRenamed     GitChangeKind = "renamed"
	GitChangeKindTypeChanged GitChangeKind = "type_changed"
)

func (value GitChangeKind) MarshalJSON() ([]byte, error) {
	switch value {
	case GitChangeKindAdded, GitChangeKindModified, GitChangeKindDeleted, GitChangeKindRenamed, GitChangeKindTypeChanged:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown GitChangeKind %q", value)
	}
}
func (value *GitChangeKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch GitChangeKind(decoded) {
	case GitChangeKindAdded, GitChangeKindModified, GitChangeKindDeleted, GitChangeKindRenamed, GitChangeKindTypeChanged:
		*value = GitChangeKind(decoded)
		return nil
	default:
		return fmt.Errorf("unknown GitChangeKind %q", decoded)
	}
}
func (value GitChangeKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *GitChangeKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type GitEntry struct {
	ModifiedMs   *uint64        `json:"modified_ms,omitempty"`
	Diff         GitLineStats   `json:"diff"`
	StagedDiff   GitLineStats   `json:"staged_diff"`
	UnstagedDiff GitLineStats   `json:"unstaged_diff"`
	Path         string         `json:"path"`
	Staged       *GitChangeKind `json:"staged"`
	Unstaged     *GitChangeKind `json:"unstaged"`
	Untracked    bool           `json:"untracked"`
	Conflicted   bool           `json:"conflicted"`
}

func DefaultGitEntry() GitEntry {
	return GitEntry{
		Diff:         DefaultGitLineStats(),
		StagedDiff:   DefaultGitLineStats(),
		UnstagedDiff: DefaultGitLineStats(),
		Path:         "",
		Staged:       nil,
		Unstaged:     nil,
		Untracked:    false,
		Conflicted:   false,
	}
}
func (value *GitEntry) UnmarshalJSON(data []byte) error {
	type plain GitEntry
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"diff", "staged_diff", "unstaged_diff", "path", "untracked", "conflicted"}, false); err != nil {
		return err
	}
	*value = GitEntry(decoded)
	return nil
}
func (value GitEntry) MarshalJSON() ([]byte, error) {
	type plain GitEntry
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitStatus struct {
	Kind          RepositoryKind `json:"kind"`
	Branch        *string        `json:"branch"`
	Head          *string        `json:"head"`
	IndexRevision *string        `json:"index_revision"`
	Entries       []GitEntry     `json:"entries"`
	Truncated     bool           `json:"truncated"`
	OmittedPaths  uint64         `json:"omitted_paths"`
	Repositories  []string       `json:"repositories,omitempty"`
}

func DefaultGitStatus() GitStatus {
	return GitStatus{
		Kind:          RepositoryKind(""),
		Branch:        nil,
		Head:          nil,
		IndexRevision: nil,
		Entries:       []GitEntry{},
		Truncated:     false,
		OmittedPaths:  0,
		Repositories:  []string{},
	}
}
func (value *GitStatus) UnmarshalJSON(data []byte) error {
	type plain GitStatus
	decoded := plain{}
	decoded.Repositories = []string{}
	if err := DecodeObject(data, &decoded, []string{"kind", "entries", "truncated", "omitted_paths"}, false); err != nil {
		return err
	}
	*value = GitStatus(decoded)
	return nil
}
func (value GitStatus) MarshalJSON() ([]byte, error) {
	type plain GitStatus
	encoded := plain(value)
	encoded.Entries = normalizeCollections(encoded.Entries)
	encoded.Repositories = normalizeCollections(encoded.Repositories)
	return json.Marshal(encoded)
}

type GitHead struct {
	Kind   RepositoryKind `json:"kind"`
	Branch *string        `json:"branch"`
	Head   *string        `json:"head"`
}

func DefaultGitHead() GitHead {
	return GitHead{
		Kind:   RepositoryKind(""),
		Branch: nil,
		Head:   nil,
	}
}
func (value *GitHead) UnmarshalJSON(data []byte) error {
	type plain GitHead
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"kind"}, false); err != nil {
		return err
	}
	*value = GitHead(decoded)
	return nil
}
func (value GitHead) MarshalJSON() ([]byte, error) {
	type plain GitHead
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitDiffScope string

const (
	GitDiffScopeAll      GitDiffScope = "all"
	GitDiffScopeStaged   GitDiffScope = "staged"
	GitDiffScopeUnstaged GitDiffScope = "unstaged"
)

func (value GitDiffScope) MarshalJSON() ([]byte, error) {
	switch value {
	case GitDiffScopeAll, GitDiffScopeStaged, GitDiffScopeUnstaged:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown GitDiffScope %q", value)
	}
}
func (value *GitDiffScope) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch GitDiffScope(decoded) {
	case GitDiffScopeAll, GitDiffScopeStaged, GitDiffScopeUnstaged:
		*value = GitDiffScope(decoded)
		return nil
	default:
		return fmt.Errorf("unknown GitDiffScope %q", decoded)
	}
}
func (value GitDiffScope) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *GitDiffScope) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type GitDiff struct {
	Path      string       `json:"path"`
	Scope     GitDiffScope `json:"scope"`
	Text      string       `json:"text"`
	Additions uint64       `json:"additions"`
	Deletions uint64       `json:"deletions"`
	Binary    bool         `json:"binary"`
	Truncated bool         `json:"truncated"`
}

func DefaultGitDiff() GitDiff {
	return GitDiff{
		Path:      "",
		Scope:     GitDiffScope(""),
		Text:      "",
		Additions: 0,
		Deletions: 0,
		Binary:    false,
		Truncated: false,
	}
}
func (value *GitDiff) UnmarshalJSON(data []byte) error {
	type plain GitDiff
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "scope", "text", "additions", "deletions", "binary", "truncated"}, false); err != nil {
		return err
	}
	*value = GitDiff(decoded)
	return nil
}
func (value GitDiff) MarshalJSON() ([]byte, error) {
	type plain GitDiff
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitIndexChange string

const (
	GitIndexChangeStage   GitIndexChange = "stage"
	GitIndexChangeUnstage GitIndexChange = "unstage"
)

func (value GitIndexChange) MarshalJSON() ([]byte, error) {
	switch value {
	case GitIndexChangeStage, GitIndexChangeUnstage:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown GitIndexChange %q", value)
	}
}
func (value *GitIndexChange) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch GitIndexChange(decoded) {
	case GitIndexChangeStage, GitIndexChangeUnstage:
		*value = GitIndexChange(decoded)
		return nil
	default:
		return fmt.Errorf("unknown GitIndexChange %q", decoded)
	}
}
func (value GitIndexChange) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *GitIndexChange) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type GitIndex struct {
	Revision string `json:"revision"`
}

func DefaultGitIndex() GitIndex {
	return GitIndex{
		Revision: "",
	}
}
func (value *GitIndex) UnmarshalJSON(data []byte) error {
	type plain GitIndex
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision"}, false); err != nil {
		return err
	}
	*value = GitIndex(decoded)
	return nil
}
func (value GitIndex) MarshalJSON() ([]byte, error) {
	type plain GitIndex
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitAction struct {
	Kind string
	Data any
}
type GitActionPruneWorktreePayload struct {
	Path string `json:"path"`
}

func DefaultGitActionPruneWorktreePayload() GitActionPruneWorktreePayload {
	return GitActionPruneWorktreePayload{
		Path: "",
	}
}
func (value *GitActionPruneWorktreePayload) UnmarshalJSON(data []byte) error {
	type plain GitActionPruneWorktreePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path"}, false); err != nil {
		return err
	}
	*value = GitActionPruneWorktreePayload(decoded)
	return nil
}
func (value GitActionPruneWorktreePayload) MarshalJSON() ([]byte, error) {
	type plain GitActionPruneWorktreePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionStartBranchPayload struct {
	Name string `json:"name"`
}

func DefaultGitActionStartBranchPayload() GitActionStartBranchPayload {
	return GitActionStartBranchPayload{
		Name: "",
	}
}
func (value *GitActionStartBranchPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionStartBranchPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name"}, false); err != nil {
		return err
	}
	*value = GitActionStartBranchPayload(decoded)
	return nil
}
func (value GitActionStartBranchPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionStartBranchPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionAddRemotePayload struct {
	Name string `json:"name"`
	Url  string `json:"url"`
}

func DefaultGitActionAddRemotePayload() GitActionAddRemotePayload {
	return GitActionAddRemotePayload{
		Name: "",
		Url:  "",
	}
}
func (value *GitActionAddRemotePayload) UnmarshalJSON(data []byte) error {
	type plain GitActionAddRemotePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "url"}, false); err != nil {
		return err
	}
	*value = GitActionAddRemotePayload(decoded)
	return nil
}
func (value GitActionAddRemotePayload) MarshalJSON() ([]byte, error) {
	type plain GitActionAddRemotePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionFetchPayload struct {
	Remote *string `json:"remote"`
	Prune  bool    `json:"prune"`
	All    bool    `json:"all"`
}

func DefaultGitActionFetchPayload() GitActionFetchPayload {
	return GitActionFetchPayload{
		Remote: nil,
		Prune:  false,
		All:    false,
	}
}
func (value *GitActionFetchPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionFetchPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"prune", "all"}, false); err != nil {
		return err
	}
	*value = GitActionFetchPayload(decoded)
	return nil
}
func (value GitActionFetchPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionFetchPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionPullPayload struct {
	Rebase bool    `json:"rebase"`
	Remote *string `json:"remote"`
	Branch *string `json:"branch"`
}

func DefaultGitActionPullPayload() GitActionPullPayload {
	return GitActionPullPayload{
		Rebase: false,
		Remote: nil,
		Branch: nil,
	}
}
func (value *GitActionPullPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionPullPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"rebase"}, false); err != nil {
		return err
	}
	*value = GitActionPullPayload(decoded)
	return nil
}
func (value GitActionPullPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionPullPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionSyncPayload struct {
	Rebase bool `json:"rebase"`
}

func DefaultGitActionSyncPayload() GitActionSyncPayload {
	return GitActionSyncPayload{
		Rebase: false,
	}
}
func (value *GitActionSyncPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionSyncPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"rebase"}, false); err != nil {
		return err
	}
	*value = GitActionSyncPayload(decoded)
	return nil
}
func (value GitActionSyncPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionSyncPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionRemoveRemotePayload struct {
	Name string `json:"name"`
}

func DefaultGitActionRemoveRemotePayload() GitActionRemoveRemotePayload {
	return GitActionRemoveRemotePayload{
		Name: "",
	}
}
func (value *GitActionRemoveRemotePayload) UnmarshalJSON(data []byte) error {
	type plain GitActionRemoveRemotePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name"}, false); err != nil {
		return err
	}
	*value = GitActionRemoveRemotePayload(decoded)
	return nil
}
func (value GitActionRemoveRemotePayload) MarshalJSON() ([]byte, error) {
	type plain GitActionRemoveRemotePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionDeleteRemoteBranchPayload struct {
	Branch string `json:"branch"`
	Commit string `json:"commit"`
}

func DefaultGitActionDeleteRemoteBranchPayload() GitActionDeleteRemoteBranchPayload {
	return GitActionDeleteRemoteBranchPayload{
		Branch: "",
		Commit: "",
	}
}
func (value *GitActionDeleteRemoteBranchPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionDeleteRemoteBranchPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"branch", "commit"}, false); err != nil {
		return err
	}
	*value = GitActionDeleteRemoteBranchPayload(decoded)
	return nil
}
func (value GitActionDeleteRemoteBranchPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionDeleteRemoteBranchPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionCreateTagPayload struct {
	Name    string  `json:"name"`
	Commit  string  `json:"commit"`
	Message *string `json:"message"`
}

func DefaultGitActionCreateTagPayload() GitActionCreateTagPayload {
	return GitActionCreateTagPayload{
		Name:    "",
		Commit:  "",
		Message: nil,
	}
}
func (value *GitActionCreateTagPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionCreateTagPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "commit"}, false); err != nil {
		return err
	}
	*value = GitActionCreateTagPayload(decoded)
	return nil
}
func (value GitActionCreateTagPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionCreateTagPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionDeleteTagPayload struct {
	Name   string  `json:"name"`
	Commit string  `json:"commit"`
	Remote *string `json:"remote"`
}

func DefaultGitActionDeleteTagPayload() GitActionDeleteTagPayload {
	return GitActionDeleteTagPayload{
		Name:   "",
		Commit: "",
		Remote: nil,
	}
}
func (value *GitActionDeleteTagPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionDeleteTagPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "commit"}, false); err != nil {
		return err
	}
	*value = GitActionDeleteTagPayload(decoded)
	return nil
}
func (value GitActionDeleteTagPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionDeleteTagPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionPushTagsPayload struct {
	Remote string `json:"remote"`
}

func DefaultGitActionPushTagsPayload() GitActionPushTagsPayload {
	return GitActionPushTagsPayload{
		Remote: "",
	}
}
func (value *GitActionPushTagsPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionPushTagsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"remote"}, false); err != nil {
		return err
	}
	*value = GitActionPushTagsPayload(decoded)
	return nil
}
func (value GitActionPushTagsPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionPushTagsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionDropAllStashesPayload struct {
	Commits []string `json:"commits"`
}

func DefaultGitActionDropAllStashesPayload() GitActionDropAllStashesPayload {
	return GitActionDropAllStashesPayload{
		Commits: []string{},
	}
}
func (value *GitActionDropAllStashesPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionDropAllStashesPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"commits"}, false); err != nil {
		return err
	}
	*value = GitActionDropAllStashesPayload(decoded)
	return nil
}
func (value GitActionDropAllStashesPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionDropAllStashesPayload
	encoded := plain(value)
	encoded.Commits = normalizeCollections(encoded.Commits)
	return json.Marshal(encoded)
}

type GitActionSwitchTrackingPayload struct {
	Remote string `json:"remote"`
	Local  string `json:"local"`
	Stash  bool   `json:"stash"`
}

func DefaultGitActionSwitchTrackingPayload() GitActionSwitchTrackingPayload {
	return GitActionSwitchTrackingPayload{
		Remote: "",
		Local:  "",
		Stash:  false,
	}
}
func (value *GitActionSwitchTrackingPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionSwitchTrackingPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"remote", "local", "stash"}, false); err != nil {
		return err
	}
	*value = GitActionSwitchTrackingPayload(decoded)
	return nil
}
func (value GitActionSwitchTrackingPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionSwitchTrackingPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionSwitchStashingPayload struct {
	Branch string `json:"branch"`
	Commit string `json:"commit"`
}

func DefaultGitActionSwitchStashingPayload() GitActionSwitchStashingPayload {
	return GitActionSwitchStashingPayload{
		Branch: "",
		Commit: "",
	}
}
func (value *GitActionSwitchStashingPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionSwitchStashingPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"branch", "commit"}, false); err != nil {
		return err
	}
	*value = GitActionSwitchStashingPayload(decoded)
	return nil
}
func (value GitActionSwitchStashingPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionSwitchStashingPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionPushPayload struct {
	Remote  *string `json:"remote"`
	Publish bool    `json:"publish"`
	Force   bool    `json:"force"`
}

func DefaultGitActionPushPayload() GitActionPushPayload {
	return GitActionPushPayload{
		Remote:  nil,
		Publish: false,
		Force:   false,
	}
}
func (value *GitActionPushPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionPushPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"publish", "force"}, false); err != nil {
		return err
	}
	*value = GitActionPushPayload(decoded)
	return nil
}
func (value GitActionPushPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionPushPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionMergePayload struct {
	Commit string `json:"commit"`
}

func DefaultGitActionMergePayload() GitActionMergePayload {
	return GitActionMergePayload{
		Commit: "",
	}
}
func (value *GitActionMergePayload) UnmarshalJSON(data []byte) error {
	type plain GitActionMergePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"commit"}, false); err != nil {
		return err
	}
	*value = GitActionMergePayload(decoded)
	return nil
}
func (value GitActionMergePayload) MarshalJSON() ([]byte, error) {
	type plain GitActionMergePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionRebasePayload struct {
	Commit string `json:"commit"`
}

func DefaultGitActionRebasePayload() GitActionRebasePayload {
	return GitActionRebasePayload{
		Commit: "",
	}
}
func (value *GitActionRebasePayload) UnmarshalJSON(data []byte) error {
	type plain GitActionRebasePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"commit"}, false); err != nil {
		return err
	}
	*value = GitActionRebasePayload(decoded)
	return nil
}
func (value GitActionRebasePayload) MarshalJSON() ([]byte, error) {
	type plain GitActionRebasePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionCherryPickPayload struct {
	Commit string `json:"commit"`
}

func DefaultGitActionCherryPickPayload() GitActionCherryPickPayload {
	return GitActionCherryPickPayload{
		Commit: "",
	}
}
func (value *GitActionCherryPickPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionCherryPickPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"commit"}, false); err != nil {
		return err
	}
	*value = GitActionCherryPickPayload(decoded)
	return nil
}
func (value GitActionCherryPickPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionCherryPickPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionTrackPayload struct {
	Branch string `json:"branch"`
}

func DefaultGitActionTrackPayload() GitActionTrackPayload {
	return GitActionTrackPayload{
		Branch: "",
	}
}
func (value *GitActionTrackPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionTrackPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"branch"}, false); err != nil {
		return err
	}
	*value = GitActionTrackPayload(decoded)
	return nil
}
func (value GitActionTrackPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionTrackPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionSwitchRemotePayload struct {
	Branch string `json:"branch"`
}

func DefaultGitActionSwitchRemotePayload() GitActionSwitchRemotePayload {
	return GitActionSwitchRemotePayload{
		Branch: "",
	}
}
func (value *GitActionSwitchRemotePayload) UnmarshalJSON(data []byte) error {
	type plain GitActionSwitchRemotePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"branch"}, false); err != nil {
		return err
	}
	*value = GitActionSwitchRemotePayload(decoded)
	return nil
}
func (value GitActionSwitchRemotePayload) MarshalJSON() ([]byte, error) {
	type plain GitActionSwitchRemotePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionStashPayload struct {
	Mode GitStashMode `json:"mode"`
}

func DefaultGitActionStashPayload() GitActionStashPayload {
	return GitActionStashPayload{
		Mode: GitStashMode(""),
	}
}
func (value *GitActionStashPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionStashPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"mode"}, false); err != nil {
		return err
	}
	*value = GitActionStashPayload(decoded)
	return nil
}
func (value GitActionStashPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionStashPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionApplyStashPayload struct {
	Commit string `json:"commit"`
	Pop    bool   `json:"pop"`
}

func DefaultGitActionApplyStashPayload() GitActionApplyStashPayload {
	return GitActionApplyStashPayload{
		Commit: "",
		Pop:    false,
	}
}
func (value *GitActionApplyStashPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionApplyStashPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"commit", "pop"}, false); err != nil {
		return err
	}
	*value = GitActionApplyStashPayload(decoded)
	return nil
}
func (value GitActionApplyStashPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionApplyStashPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionDropStashPayload struct {
	Commit string `json:"commit"`
}

func DefaultGitActionDropStashPayload() GitActionDropStashPayload {
	return GitActionDropStashPayload{
		Commit: "",
	}
}
func (value *GitActionDropStashPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionDropStashPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"commit"}, false); err != nil {
		return err
	}
	*value = GitActionDropStashPayload(decoded)
	return nil
}
func (value GitActionDropStashPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionDropStashPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitActionDiscardAllPayload struct {
	Paths []string `json:"paths"`
}

func DefaultGitActionDiscardAllPayload() GitActionDiscardAllPayload {
	return GitActionDiscardAllPayload{
		Paths: []string{},
	}
}
func (value *GitActionDiscardAllPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionDiscardAllPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"paths"}, false); err != nil {
		return err
	}
	*value = GitActionDiscardAllPayload(decoded)
	return nil
}
func (value GitActionDiscardAllPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionDiscardAllPayload
	encoded := plain(value)
	encoded.Paths = normalizeCollections(encoded.Paths)
	return json.Marshal(encoded)
}

type GitActionIgnorePayload struct {
	Paths       []string `json:"paths"`
	Directories bool     `json:"directories"`
}

func DefaultGitActionIgnorePayload() GitActionIgnorePayload {
	return GitActionIgnorePayload{
		Paths:       []string{},
		Directories: false,
	}
}
func (value *GitActionIgnorePayload) UnmarshalJSON(data []byte) error {
	type plain GitActionIgnorePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"paths", "directories"}, false); err != nil {
		return err
	}
	*value = GitActionIgnorePayload(decoded)
	return nil
}
func (value GitActionIgnorePayload) MarshalJSON() ([]byte, error) {
	type plain GitActionIgnorePayload
	encoded := plain(value)
	encoded.Paths = normalizeCollections(encoded.Paths)
	return json.Marshal(encoded)
}

type GitActionDiscardPayload struct {
	Paths []string `json:"paths"`
}

func DefaultGitActionDiscardPayload() GitActionDiscardPayload {
	return GitActionDiscardPayload{
		Paths: []string{},
	}
}
func (value *GitActionDiscardPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionDiscardPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"paths"}, false); err != nil {
		return err
	}
	*value = GitActionDiscardPayload(decoded)
	return nil
}
func (value GitActionDiscardPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionDiscardPayload
	encoded := plain(value)
	encoded.Paths = normalizeCollections(encoded.Paths)
	return json.Marshal(encoded)
}

type GitActionTrashPayload struct {
	Paths []string `json:"paths"`
}

func DefaultGitActionTrashPayload() GitActionTrashPayload {
	return GitActionTrashPayload{
		Paths: []string{},
	}
}
func (value *GitActionTrashPayload) UnmarshalJSON(data []byte) error {
	type plain GitActionTrashPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"paths"}, false); err != nil {
		return err
	}
	*value = GitActionTrashPayload(decoded)
	return nil
}
func (value GitActionTrashPayload) MarshalJSON() ([]byte, error) {
	type plain GitActionTrashPayload
	encoded := plain(value)
	encoded.Paths = normalizeCollections(encoded.Paths)
	return json.Marshal(encoded)
}

var gitActionVariants = variantFactories{"initialize": nil,
	"prune_worktree":       func() any { return new(GitActionPruneWorktreePayload) },
	"start_branch":         func() any { return new(GitActionStartBranchPayload) },
	"add_remote":           func() any { return new(GitActionAddRemotePayload) },
	"fetch":                func() any { return new(GitActionFetchPayload) },
	"pull":                 func() any { return new(GitActionPullPayload) },
	"sync":                 func() any { return new(GitActionSyncPayload) },
	"remove_remote":        func() any { return new(GitActionRemoveRemotePayload) },
	"delete_remote_branch": func() any { return new(GitActionDeleteRemoteBranchPayload) },
	"create_tag":           func() any { return new(GitActionCreateTagPayload) },
	"delete_tag":           func() any { return new(GitActionDeleteTagPayload) },
	"push_tags":            func() any { return new(GitActionPushTagsPayload) },
	"drop_all_stashes":     func() any { return new(GitActionDropAllStashesPayload) },
	"switch_tracking":      func() any { return new(GitActionSwitchTrackingPayload) },
	"switch_stashing":      func() any { return new(GitActionSwitchStashingPayload) },
	"push":                 func() any { return new(GitActionPushPayload) },
	"merge":                func() any { return new(GitActionMergePayload) },
	"rebase":               func() any { return new(GitActionRebasePayload) },
	"cherry_pick":          func() any { return new(GitActionCherryPickPayload) },
	"track":                func() any { return new(GitActionTrackPayload) },
	"switch_remote":        func() any { return new(GitActionSwitchRemotePayload) },
	"undo_commit":          nil,
	"stash":                func() any { return new(GitActionStashPayload) },
	"apply_stash":          func() any { return new(GitActionApplyStashPayload) },
	"drop_stash":           func() any { return new(GitActionDropStashPayload) },
	"discard_all":          func() any { return new(GitActionDiscardAllPayload) },
	"ignore":               func() any { return new(GitActionIgnorePayload) },
	"discard":              func() any { return new(GitActionDiscardPayload) },
	"trash":                func() any { return new(GitActionTrashPayload) },
	"continue":             nil,
	"abort":                nil}

func (value GitAction) MarshalJSON() ([]byte, error) {
	return MarshalUnion("", "", value.Kind, value.Data, gitActionVariants)
}
func (value *GitAction) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "", "", gitActionVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type GitSync struct {
	Tags        []GitTag          `json:"tags"`
	RemoteUrls  map[string]string `json:"remote_urls"`
	Tracking    map[string]string `json:"tracking"`
	Stashes     []GitStash        `json:"stashes"`
	HeadSummary *string           `json:"head_summary"`
	HeadMessage *string           `json:"head_message"`
	Remotes     []string          `json:"remotes"`
	Upstream    *string           `json:"upstream"`
	Ahead       uint64            `json:"ahead"`
	Behind      uint64            `json:"behind"`
	Operation   *string           `json:"operation"`
}

func DefaultGitSync() GitSync {
	return GitSync{
		Tags:        []GitTag{},
		RemoteUrls:  map[string]string{},
		Tracking:    map[string]string{},
		Stashes:     []GitStash{},
		HeadSummary: nil,
		HeadMessage: nil,
		Remotes:     []string{},
		Upstream:    nil,
		Ahead:       0,
		Behind:      0,
		Operation:   nil,
	}
}
func (value *GitSync) UnmarshalJSON(data []byte) error {
	type plain GitSync
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"tags", "remote_urls", "tracking", "stashes", "remotes", "ahead", "behind"}, false); err != nil {
		return err
	}
	*value = GitSync(decoded)
	return nil
}
func (value GitSync) MarshalJSON() ([]byte, error) {
	type plain GitSync
	encoded := plain(value)
	encoded.Tags = normalizeCollections(encoded.Tags)
	encoded.RemoteUrls = normalizeCollections(encoded.RemoteUrls)
	encoded.Tracking = normalizeCollections(encoded.Tracking)
	encoded.Stashes = normalizeCollections(encoded.Stashes)
	encoded.Remotes = normalizeCollections(encoded.Remotes)
	return json.Marshal(encoded)
}

type GitCommitOptions struct {
	Signoff    bool           `json:"signoff"`
	SkipHooks  bool           `json:"skip_hooks"`
	Tracked    bool           `json:"tracked"`
	All        bool           `json:"all"`
	After      GitAfterCommit `json:"after"`
	PushRemote *string        `json:"push_remote"`
}

func DefaultGitCommitOptions() GitCommitOptions {
	return GitCommitOptions{
		Signoff:    false,
		SkipHooks:  false,
		Tracked:    false,
		All:        false,
		After:      GitAfterCommitNone,
		PushRemote: nil,
	}
}
func (value *GitCommitOptions) UnmarshalJSON(data []byte) error {
	type plain GitCommitOptions
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"signoff", "skip_hooks", "tracked", "all", "after"}, false); err != nil {
		return err
	}
	*value = GitCommitOptions(decoded)
	return nil
}
func (value GitCommitOptions) MarshalJSON() ([]byte, error) {
	type plain GitCommitOptions
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitLineStats struct {
	Additions uint64 `json:"additions"`
	Deletions uint64 `json:"deletions"`
	Truncated bool   `json:"truncated"`
}

func DefaultGitLineStats() GitLineStats {
	return GitLineStats{
		Additions: 0,
		Deletions: 0,
		Truncated: false,
	}
}
func (value *GitLineStats) UnmarshalJSON(data []byte) error {
	type plain GitLineStats
	decoded := plain{}
	decoded.Truncated = false
	if err := DecodeObject(data, &decoded, []string{"additions", "deletions"}, false); err != nil {
		return err
	}
	*value = GitLineStats(decoded)
	return nil
}
func (value GitLineStats) MarshalJSON() ([]byte, error) {
	type plain GitLineStats
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitStashMode string

const (
	GitStashModeAll     GitStashMode = "all"
	GitStashModeTracked GitStashMode = "tracked"
	GitStashModeStaged  GitStashMode = "staged"
)

func (value GitStashMode) MarshalJSON() ([]byte, error) {
	switch value {
	case GitStashModeAll, GitStashModeTracked, GitStashModeStaged:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown GitStashMode %q", value)
	}
}
func (value *GitStashMode) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch GitStashMode(decoded) {
	case GitStashModeAll, GitStashModeTracked, GitStashModeStaged:
		*value = GitStashMode(decoded)
		return nil
	default:
		return fmt.Errorf("unknown GitStashMode %q", decoded)
	}
}
func (value GitStashMode) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *GitStashMode) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type GitStash struct {
	Commit  string `json:"commit"`
	Message string `json:"message"`
}

func DefaultGitStash() GitStash {
	return GitStash{
		Commit:  "",
		Message: "",
	}
}
func (value *GitStash) UnmarshalJSON(data []byte) error {
	type plain GitStash
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"commit", "message"}, false); err != nil {
		return err
	}
	*value = GitStash(decoded)
	return nil
}
func (value GitStash) MarshalJSON() ([]byte, error) {
	type plain GitStash
	encoded := plain(value)
	return json.Marshal(encoded)
}

type GitAfterCommit string

const (
	GitAfterCommitNone GitAfterCommit = "none"
	GitAfterCommitPush GitAfterCommit = "push"
	GitAfterCommitSync GitAfterCommit = "sync"
)

func (value GitAfterCommit) MarshalJSON() ([]byte, error) {
	switch value {
	case GitAfterCommitNone, GitAfterCommitPush, GitAfterCommitSync:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown GitAfterCommit %q", value)
	}
}
func (value *GitAfterCommit) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch GitAfterCommit(decoded) {
	case GitAfterCommitNone, GitAfterCommitPush, GitAfterCommitSync:
		*value = GitAfterCommit(decoded)
		return nil
	default:
		return fmt.Errorf("unknown GitAfterCommit %q", decoded)
	}
}
func (value GitAfterCommit) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *GitAfterCommit) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type GitTag struct {
	Name   string `json:"name"`
	Commit string `json:"commit"`
}

func DefaultGitTag() GitTag {
	return GitTag{
		Name:   "",
		Commit: "",
	}
}
func (value *GitTag) UnmarshalJSON(data []byte) error {
	type plain GitTag
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "commit"}, false); err != nil {
		return err
	}
	*value = GitTag(decoded)
	return nil
}
func (value GitTag) MarshalJSON() ([]byte, error) {
	type plain GitTag
	encoded := plain(value)
	return json.Marshal(encoded)
}
