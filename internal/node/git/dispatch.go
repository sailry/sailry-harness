package git

import (
	"context"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func (s *Service) Read(ctx context.Context, root string, command protocol.Command) (protocol.Output, error) {
	duration := readTimeout
	if command.Kind == "list_git_remote_tags" {
		duration = 125 * time.Second
	}
	ctx, cancel := context.WithTimeout(ctx, duration)
	defer cancel()
	type result struct {
		output protocol.Output
		err    error
	}
	done := make(chan result, 1)
	go func() { output, err := s.read(ctx, root, command); done <- result{output, err} }()
	select {
	case <-ctx.Done():
		return protocol.Output{}, mapError(ctx.Err())
	case value := <-done:
		return value.output, value.err
	}
}
func (s *Service) read(ctx context.Context, root string, command protocol.Command) (protocol.Output, error) {
	switch command.Kind {
	case "inspect_git":
		v, e := s.Status(ctx, root)
		return protocol.Output{Kind: "git_status", Data: v}, e
	case "read_git_head":
		v, e := s.Head(ctx, root)
		return protocol.Output{Kind: "git_head", Data: v}, e
	case "resolve_git_revision":
		r, e := open(root)
		if e != nil {
			return protocol.Output{}, e
		}
		v, e := resolve(r, data[protocol.CommandResolveGitRevisionPayload](command.Data).Revision)
		return protocol.Output{Kind: "git_revision", Data: protocol.OutputGitRevisionPayload{Commit: v.String()}}, e
	case "read_git_diff":
		v := data[protocol.CommandReadGitDiffPayload](command.Data)
		result, e := s.Diff(ctx, root, v.Path, v.Scope)
		return protocol.Output{Kind: "git_diff", Data: result}, e
	case "read_git_log":
		v, e := s.Log(ctx, root, data[protocol.CommandReadGitLogPayload](command.Data))
		return protocol.Output{Kind: "git_log", Data: v}, e
	case "read_git_commit":
		v, e := s.Commit(ctx, root, data[protocol.CommandReadGitCommitPayload](command.Data).Commit)
		return protocol.Output{Kind: "git_commit", Data: v}, e
	case "read_git_show":
		return s.Show(ctx, root, data[protocol.CommandReadGitShowPayload](command.Data))
	case "read_git_blame":
		v, e := s.Blame(ctx, root, data[protocol.CommandReadGitBlamePayload](command.Data))
		return protocol.Output{Kind: "git_blame", Data: v}, e
	case "list_git_branches":
		v, e := s.Branches(ctx, root)
		return protocol.Output{Kind: "git_branches", Data: v}, e
	case "read_git_output":
		return protocol.Output{Kind: "git_output", Data: protocol.OutputGitOutputPayload{Text: s.Output(root)}}, nil
	case "list_git_remote_tags":
		v, e := s.RemoteTags(ctx, root, data[protocol.CommandListGitRemoteTagsPayload](command.Data).Remote)
		return protocol.Output{Kind: "git_remote_tags", Data: v}, e
	case "read_git_stash":
		v := data[protocol.CommandReadGitStashPayload](command.Data)
		position := -1
		stashes, e := s.stashes(ctx, root)
		if e != nil {
			return protocol.Output{}, e
		}
		for i, stash := range stashes {
			if stash.Commit == v.Commit {
				position = i
				break
			}
		}
		if position < 0 {
			return protocol.Output{}, conflict()
		}
		capture, e := s.command(ctx, root, []string{"stash", "show", "--patch", "--no-ext-diff", "--no-color", v.Commit}, nil, true)
		text, partial := bounded(string(capture.Stdout), maxDiff)
		return protocol.Output{Kind: "git_diff", Data: protocol.GitDiff{Scope: protocol.GitDiffScopeAll, Text: text, Truncated: partial || capture.Truncated}}, e
	case "list_git_changes":
		v, e := s.Changes(ctx, root, data[protocol.CommandListGitChangesPayload](command.Data))
		return protocol.Output{Kind: "git_changes", Data: v}, e
	case "read_git_change":
		v, e := s.Change(ctx, root, data[protocol.CommandReadGitChangePayload](command.Data))
		return protocol.Output{Kind: "git_change", Data: v}, e
	case "read_git_context":
		v, e := s.Context(ctx, root, data[protocol.CommandReadGitContextPayload](command.Data))
		return protocol.Output{Kind: "git_context", Data: v}, e
	}
	return protocol.Output{}, invalid("unsupported Git read")
}
func (s *Service) Execute(ctx context.Context, root string, command protocol.Command) (protocol.Output, error) {
	switch command.Kind {
	case "update_git_index":
		v, e := s.UpdateIndex(ctx, root, data[protocol.CommandUpdateGitIndexPayload](command.Data))
		return protocol.Output{Kind: "git_index", Data: v}, e
	case "run_git_action":
		e := s.Action(ctx, root, data[protocol.CommandRunGitActionPayload](command.Data))
		return protocol.Output{Kind: "git_action_completed"}, e
	case "create_git_commit":
		v, e := s.CreateCommit(ctx, root, data[protocol.CommandCreateGitCommitPayload](command.Data))
		return protocol.Output{Kind: "git_commit_created", Data: v}, e
	case "create_git_branch":
		v := data[protocol.CommandCreateGitBranchPayload](command.Data)
		result, e := s.CreateBranch(ctx, root, v.Name, v.Commit)
		return protocol.Output{Kind: "git_branch_created", Data: result}, e
	case "rename_git_branch":
		v := data[protocol.CommandRenameGitBranchPayload](command.Data)
		result, e := s.RenameBranch(ctx, root, v.Name, v.NewName, v.Commit)
		return protocol.Output{Kind: "git_branch_renamed", Data: result}, e
	case "delete_git_branch":
		v := data[protocol.CommandDeleteGitBranchPayload](command.Data)
		e := s.DeleteBranch(ctx, root, v)
		return protocol.Output{Kind: "git_branch_deleted", Data: protocol.OutputGitBranchDeletedPayload{Name: v.Name}}, e
	case "switch_git_branch":
		v := data[protocol.CommandSwitchGitBranchPayload](command.Data)
		result, e := s.SwitchBranch(ctx, root, v)
		return protocol.Output{Kind: "git_branch_switched", Data: result}, e
	case "merge_git_branch":
		v := data[protocol.CommandMergeGitBranchPayload](command.Data)
		result, e := s.MergeBranch(ctx, root, v)
		return protocol.Output{Kind: "git_merged", Data: result}, e
	}
	return protocol.Output{}, invalid("unsupported Git mutation")
}
func (s *Service) RemoteTags(ctx context.Context, root, remote string) ([]protocol.GitTag, error) {
	if err := name(remote); err != nil {
		return nil, err
	}
	r, err := open(root)
	if err != nil {
		return nil, err
	}
	if _, err := r.Remote(remote); err != nil {
		return nil, mapError(err)
	}
	capture, err := s.command(ctx, root, []string{"ls-remote", "--tags", "--refs", remote}, nil, false)
	if err != nil {
		return nil, err
	}
	if capture.Truncated || capture.InvalidUTF8 {
		return nil, fault(protocol.ErrorCodeUnavailable, "Git remote listing exceeds output limit")
	}
	result := []protocol.GitTag{}
	for _, line := range strings.Split(strings.TrimSpace(string(capture.Stdout)), "\n") {
		commit, ref, ok := strings.Cut(line, "\t")
		if !ok {
			continue
		}
		tag, ok := strings.CutPrefix(ref, "refs/tags/")
		if !ok {
			continue
		}
		if err := fullHash(commit); err != nil {
			return nil, err
		}
		result = append(result, protocol.GitTag{Name: tag, Commit: commit})
		if len(result) > maxEntries {
			return nil, fault(protocol.ErrorCodeUnavailable, "remote tag listing exceeds limit")
		}
	}
	return result, nil
}
