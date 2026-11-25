//go:build integration

package node

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProjectWorktreeIdentitySurvivesOmittingModule(t *testing.T) {
	git, err := exec.LookPath("git")
	if err != nil {
		t.Skip("Git executable is required")
	}
	profile := filepath.Join(t.TempDir(), "profile")
	node := acceptanceNode(t, profile, nil, Options{})
	t.Cleanup(func() { _ = node.Stop(context.Background()) })
	root, commit := gitProjectFixture(t, map[string]string{"file.txt": "linked contents\n"})
	project := acceptanceDo[protocol.Project](t, node, protocol.Command{Kind: "register_project", Data: protocol.CommandRegisterProjectPayload{Name: "Retained workspace", Path: root}})
	path := filepath.Join(t.TempDir(), "linked")
	cmd := exec.CommandContext(t.Context(), git, "-C", root, "worktree", "add", "-b", "linked", path, commit)
	cmd.Env = append(os.Environ(), "GIT_CONFIG_NOSYSTEM=1", "GIT_CONFIG_GLOBAL="+os.DevNull, "GIT_TERMINAL_PROMPT=0")
	if output, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("create checkout for a previously registered workspace: %v\n%s", err, output)
	}
	tree := recordWorkspaceFixture(t, node, project.ID, path)
	if err := node.Stop(context.Background()); err != nil {
		t.Fatal(err)
	}
	node = acceptanceNode(t, profile, nil, Options{})
	other, _ := gitProjectFixture(t, map[string]string{"file.txt": "different repository\n"})
	_, failure := acceptanceCall[protocol.Project](t, node, protocol.Command{Kind: "update_project", Data: protocol.CommandUpdateProjectPayload{
		Expected: project, Name: project.Name, Path: other, Appearance: project.Appearance,
	}})
	if failure == nil || failure.Code != protocol.ErrorCodeConflict {
		t.Fatalf("project move to another repository = %v", failure)
	}
	content := acceptanceDo[protocol.FileContent](t, node, protocol.Command{Kind: "read_file", Data: protocol.CommandReadFilePayload{Worktree: tree.ID, Path: "file.txt"}})
	if content.Text != "linked contents\n" {
		t.Fatalf("registered checkout was not retained: %+v", content)
	}
}
