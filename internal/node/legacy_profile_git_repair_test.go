package node

import (
	"os"
	"path/filepath"
	"testing"
)

func TestLegacyGitRepairPreservesChangedRegistration(t *testing.T) {
	t.Parallel()
	for _, name := range []string{"gitdir", "commondir", ".git"} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			base, err := filepath.EvalSymlinks(t.TempDir())
			if err != nil {
				t.Fatal(err)
			}
			legacy, target := filepath.Join(base, "legacy"), filepath.Join(base, "code")
			repo := filepath.Join(base, "repo")
			initMigrationRepository(t, repo)
			tree := filepath.Join("worktrees", "side")
			migrationGit(t, "-C", repo, "worktree", "add", "-b", "side", filepath.Join(legacy, tree))
			linked := map[string]string{}
			if err := checkLegacyProfileWorktrees(legacy, linked); err != nil {
				t.Fatal(err)
			}
			if err := os.Rename(legacy, target); err != nil {
				t.Fatal(err)
			}
			admin := filepath.Join(repo, ".git", "worktrees", "side")
			path := filepath.Join(admin, name)
			if name == ".git" {
				path = filepath.Join(target, tree, name)
			}
			changed := filepath.Join(base, "other")
			if err := os.WriteFile(path, []byte(changed), 0o600); err != nil {
				t.Fatal(err)
			}
			backlink := filepath.Join(admin, "gitdir")
			before, err := os.ReadFile(backlink)
			if err != nil {
				t.Fatal(err)
			}
			if err := repairLegacyProfileWorktrees(legacy, target, linked); err == nil {
				t.Fatal("repair accepted a changed worktree registration")
			}
			after, err := os.ReadFile(backlink)
			if err != nil || string(after) != string(before) {
				t.Fatalf("repair changed foreign Git backlink: %q, %v", after, err)
			}
		})
	}
}
