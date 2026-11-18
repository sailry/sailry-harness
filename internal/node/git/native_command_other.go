//go:build !linux

package git

import (
	"context"
	"os/exec"
)

func prepareGitCommand(_ context.Context, cmd *exec.Cmd, _ []string, _ map[string]string, _ string) (gitCommand, error) {
	return directGitCommand(cmd), nil
}

func prepareGitWorktree(ctx context.Context, _ string) (context.Context, func(), error) {
	return ctx, func() {}, nil
}

func (s *Service) removeGitWorktree(ctx context.Context, main, root string) error {
	_, err := s.command(ctx, main, []string{"worktree", "remove", "--", root}, nil, false)
	return err
}
