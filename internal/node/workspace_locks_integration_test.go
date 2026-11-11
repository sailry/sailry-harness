//go:build integration && unix

package node

import (
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"

	workspacegit "github.com/sailry/sailry-harness/internal/node/git"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A push that is still transferring holds no lock that a file write, a local
// Git effect or a project registration needs. The remote is a script.
func TestGitTransferDoesNotHoldWorkspaceEffects(t *testing.T) {
	real, err := exec.LookPath("git")
	if err != nil {
		t.Skip("Git executable is required")
	}
	bin, home, signals := t.TempDir(), t.TempDir(), t.TempDir()
	started, release := filepath.Join(signals, "started"), filepath.Join(signals, "release")
	script := `#!/bin/sh
for argument; do
	if [ "$argument" = push ]; then
		: > "$STARTED"
		while [ ! -e "$RELEASE" ]; do echo transferring >&2; sleep 0.05; done
		exit 0
	fi
done
exec "$REAL_GIT" "$@"
`
	if err := os.WriteFile(filepath.Join(bin, "git"), []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", bin+string(os.PathListSeparator)+os.Getenv("PATH"))
	t.Cleanup(func() { _ = os.WriteFile(release, nil, 0o644) })
	node := acceptanceNode(t, filepath.Join(t.TempDir(), "profile"), nil, Options{Git: workspacegit.Options{Environment: []string{
		"PATH=" + os.Getenv("PATH"), "HOME=" + home, "REAL_GIT=" + real, "STARTED=" + started, "RELEASE=" + release,
		"GIT_AUTHOR_NAME=Fixture", "GIT_AUTHOR_EMAIL=fixture@example.test", "GIT_COMMITTER_NAME=Fixture", "GIT_COMMITTER_EMAIL=fixture@example.test",
	}}})
	t.Cleanup(func() { _ = node.Stop(t.Context()) })
	project, main := projectFixture(t, node)
	inspect := func() protocol.GitStatus {
		return acceptanceDo[protocol.GitStatus](t, node, protocol.Command{Kind: "inspect_git", Data: protocol.CommandInspectGitPayload{Worktree: main}})
	}
	status := inspect()
	push, err := node.Dispatch(t.Context(), node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "run_git_action", Data: protocol.CommandRunGitActionPayload{
		Worktree: main, ExpectedIndex: *status.IndexRevision, ExpectedHead: status.Head, ExpectedBranch: status.Branch,
		Action: protocol.GitAction{Kind: "push", Data: protocol.GitActionPushPayload{}},
	}}))
	if err != nil {
		t.Fatal(err)
	}
	acceptanceEventually(t, "the push reaches its remote", func() bool { _, err := os.Stat(started); return err == nil })

	// The same worktree accepts a file write and a local Git effect meanwhile.
	acceptanceDo[json.RawMessage](t, node, protocol.Command{Kind: "write_file", Data: protocol.CommandWriteFilePayload{Worktree: main, Path: "note.txt", Text: "written during the push\n"}})
	status = inspect()
	acceptanceDo[json.RawMessage](t, node, protocol.Command{Kind: "update_git_index", Data: protocol.CommandUpdateGitIndexPayload{
		Worktree: main, Paths: []string{"note.txt"}, Operation: protocol.GitIndexChangeStage, ExpectedIndex: *status.IndexRevision, ExpectedHead: status.Head,
	}})
	// So does a registration.
	other := protocol.ProjectsDraft{Name: "Other", Path: t.TempDir(), Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"}}
	acceptanceDo[protocol.Project](t, node, protocol.Command{Kind: "create_project", Data: other})
	select {
	case result := <-push.Completion:
		t.Fatalf("the push ended before its remote answered: %+v", result)
	default:
	}
	if err := os.WriteFile(release, nil, 0o644); err != nil {
		t.Fatal(err)
	}
	select {
	case result := <-push.Completion:
		if result.Fault != nil {
			t.Fatal(result.Fault)
		}
	case <-time.After(acceptanceWait):
		t.Fatal("the push did not finish")
	}
	_ = project
}
