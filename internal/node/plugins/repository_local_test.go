package plugins

import (
	"context"
	"errors"
	"os"
	"testing"
	"time"

	git "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestLocalRepositoryAcquisitionWithoutGit(t *testing.T) {
	source := t.TempDir()
	repository, err := git.PlainInit(source, false)
	if err != nil {
		t.Fatal(err)
	}
	packageFixture(t, source, "SKILL.md", "---\nname: local\ndescription: Local fixture\n---\nBody")
	worktree, err := repository.Worktree()
	if err != nil {
		t.Fatal(err)
	}
	if _, err := worktree.Add("."); err != nil {
		t.Fatal(err)
	}
	commit, err := worktree.Commit("Local fixture", &git.CommitOptions{Author: &object.Signature{Name: "Fixture", Email: "fixture@example.test", When: time.Unix(0, 0)}})
	if err != nil {
		t.Fatal(err)
	}
	selected, err := ParseRepositorySource(protocol.PluginSkillsSource{Repository: source})
	if err != nil {
		t.Fatal(err)
	}
	staging := t.TempDir()
	for name, value := range map[string]string{"PATH": "", "TMPDIR": staging, "TMP": staging, "TEMP": staging} {
		t.Setenv(name, value)
	}
	resolved, entries, issues, err := acquireRepository(t.Context(), selected, "")
	if err != nil || resolved.Commit != commit.String() || len(entries) != 1 || entries[0].name != "SKILL.md" || len(issues) != 0 {
		t.Fatalf("local acquisition without Git = %+v, %+v, %+v, %v", resolved, entries, issues, err)
	}
	var fault *protocol.Fault
	if _, _, _, err := acquireRepository(t.Context(), selected, "", repositoryCaptureOptions{maximum: MaxPackageEntries, transfer: 1}); !errors.As(err, &fault) || fault.Message != "Git transfer exceeds its size limit" {
		t.Fatalf("local transfer limit = %v", err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, _, _, err := acquireRepository(ctx, selected, ""); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled local acquisition = %v", err)
	}
	if entries, err := os.ReadDir(staging); err != nil || len(entries) != 0 {
		t.Fatalf("local acquisition left temporary storage: %v, %v", entries, err)
	}
}
