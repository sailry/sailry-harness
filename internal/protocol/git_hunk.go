package protocol

import (
	"encoding/json"
	"fmt"
	"strings"
)

// GitHunkOperation acts on one captured working-tree diff hunk.
type GitHunkOperation string

const (
	GitHunkStage  GitHunkOperation = "stage"
	GitHunkReject GitHunkOperation = "reject"
)

// CommandApplyGitHunkPayload extends v1 for native Git hunks without a turn.
// Hunk is zero-based within the complete diff identified by DiffRevision.
type CommandApplyGitHunkPayload struct {
	Worktree         WorktreeID       `json:"worktree"`
	Path             string           `json:"path"`
	Scope            GitDiffScope     `json:"scope"`
	Operation        GitHunkOperation `json:"operation"`
	Hunk             uint32           `json:"hunk"`
	DiffRevision     string           `json:"diff_revision"`
	ExpectedRevision string           `json:"expected_revision"`
	ExpectedHead     *string          `json:"expected_head"`
	ExpectedBranch   *string          `json:"expected_branch"`
	ExpectedIndex    string           `json:"expected_index"`
}

func (value CommandApplyGitHunkPayload) Validate() error {
	if value.Worktree == (WorktreeID{}) || value.Path == "" {
		return fmt.Errorf("Git hunk requires a worktree and file path")
	}
	if value.Scope != GitDiffScopeAll && value.Scope != GitDiffScopeUnstaged {
		return fmt.Errorf("Git hunk scope must contain working-tree changes")
	}
	if value.Operation != GitHunkStage && value.Operation != GitHunkReject {
		return fmt.Errorf("invalid Git hunk operation")
	}
	for _, revision := range []string{value.DiffRevision, value.ExpectedRevision, value.ExpectedIndex} {
		if len(revision) != 64 || strings.Trim(revision, "0123456789abcdef") != "" {
			return fmt.Errorf("Git hunk requires complete content and index revisions")
		}
	}
	if value.ExpectedHead != nil && (len(*value.ExpectedHead) != 40 || strings.Trim(*value.ExpectedHead, "0123456789abcdef") != "") {
		return fmt.Errorf("Git hunk requires a complete HEAD revision")
	}
	return nil
}
func (value *CommandApplyGitHunkPayload) UnmarshalJSON(data []byte) error {
	type plain CommandApplyGitHunkPayload
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"worktree", "path", "scope", "operation", "hunk", "diff_revision", "expected_revision", "expected_index"}, true); err != nil {
		return err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return err
	}
	for _, name := range []string{"expected_head", "expected_branch"} {
		if _, exists := fields[name]; !exists {
			return fmt.Errorf("missing field %q", name)
		}
	}
	result := CommandApplyGitHunkPayload(decoded)
	if err := result.Validate(); err != nil {
		return err
	}
	*value = result
	return nil
}
func (value CommandApplyGitHunkPayload) MarshalJSON() ([]byte, error) {
	if err := value.Validate(); err != nil {
		return nil, err
	}
	type plain CommandApplyGitHunkPayload
	return json.Marshal(plain(value))
}

type GitHunkApplied struct {
	Worktree  WorktreeID       `json:"worktree"`
	Path      string           `json:"path"`
	Operation GitHunkOperation `json:"operation"`
	Hunk      uint32           `json:"hunk"`
	Revision  string           `json:"revision"`
	Index     *GitIndex        `json:"index"`
}

func (value *GitHunkApplied) UnmarshalJSON(data []byte) error {
	type plain GitHunkApplied
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"worktree", "path", "operation", "hunk", "revision"}, true); err != nil {
		return err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return err
	}
	if _, exists := fields["index"]; !exists {
		return fmt.Errorf("missing field %q", "index")
	}
	*value = GitHunkApplied(decoded)
	if value.Operation != GitHunkStage && value.Operation != GitHunkReject {
		return fmt.Errorf("invalid applied Git hunk operation")
	}
	if value.Operation == GitHunkStage && value.Index == nil || value.Operation == GitHunkReject && value.Index != nil {
		return fmt.Errorf("Git hunk result does not match its operation")
	}
	return nil
}

func init() {
	commandVariants["apply_git_hunk"] = func() any { return new(CommandApplyGitHunkPayload) }
	outputVariants["git_hunk_applied"] = func() any { return new(GitHunkApplied) }
}
