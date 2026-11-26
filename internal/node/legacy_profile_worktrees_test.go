package node

import (
	"bytes"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func migrationGit(t *testing.T, args ...string) string {
	t.Helper()
	git, err := exec.LookPath("git")
	if err != nil {
		t.Skip("Git executable unavailable")
	}
	cmd := exec.CommandContext(t.Context(), git, args...)
	cmd.Env = append(os.Environ(), "GIT_CONFIG_NOSYSTEM=1", "GIT_CONFIG_GLOBAL="+os.DevNull)
	body, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("git %v: %v: %s", args, err, body)
	}
	return strings.TrimSpace(string(body))
}

func initMigrationRepository(t *testing.T, repository string) {
	t.Helper()
	migrationGit(t, "init", "--initial-branch=main", repository)
	migrationGit(t, "-C", repository, "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
		"-c", "commit.gpgsign=false", "commit", "--allow-empty", "-m", "Fixture")
}

func TestMigrateLegacyProfileLinkedGitWorktrees(t *testing.T) {
	t.Parallel()
	for _, phase := range []string{"uninterrupted", "published"} {
		t.Run(phase, func(t *testing.T) {
			t.Parallel()
			fixture := newLegacyMigrationFixture(t)
			repo := filepath.Join(fixture.outside, "repository")
			initMigrationRepository(t, repo)
			head := migrationGit(t, "-C", repo, "rev-parse", "HEAD")
			// The latter is a registered project with a worktrees row, outside the managed roots.
			checkouts := []string{filepath.Join("worktrees", "managed"), filepath.Join("projects", "inside")}
			for _, relative := range checkouts {
				path := filepath.Join(fixture.legacy, relative)
				migrationGit(t, "-C", repo, "worktree", "add", "-b", filepath.Base(path), path)
				for _, name := range []string{"tracked", "untracked"} {
					if err := os.WriteFile(filepath.Join(path, name), []byte("draft"), 0o600); err != nil {
						t.Fatal(err)
					}
				}
				migrationGit(t, "-C", path, "add", "tracked")
				if err := os.WriteFile(filepath.Join(path, "tracked"), []byte("edited"), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			if phase == "published" {
				interrupted := errors.New("interrupted after publishing")
				err := migrateLegacyProfile(t.Context(), fixture.legacy, fixture.target, func(step string) error {
					if step == "published" {
						return interrupted
					}
					return nil
				})
				if !errors.Is(err, interrupted) {
					t.Fatal(err)
				}
				// One backlink was repaired before shutdown; recovery must accept both states.
				migrationGit(t, "-C", repo, "worktree", "repair", filepath.Join(fixture.target, checkouts[0]))
			}
			if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err != nil {
				t.Fatal(err)
			}
			fixture.assertMigrated(t)
			for _, relative := range checkouts {
				path := filepath.Join(fixture.target, relative)
				branch := filepath.Base(path)
				if got := migrationGit(t, "-C", path, "rev-parse", "HEAD"); got != head {
					t.Fatalf("migrated HEAD = %q, want %q", got, head)
				}
				if got := migrationGit(t, "-C", path, "symbolic-ref", "--short", "HEAD"); got != branch {
					t.Fatalf("migrated branch = %q, want %q", got, branch)
				}
				if got := migrationGit(t, "-C", path, "show", ":tracked"); got != "draft" {
					t.Fatalf("migrated index content = %q", got)
				}
				for name, want := range map[string]string{"tracked": "edited", "untracked": "draft"} {
					got, err := os.ReadFile(filepath.Join(path, name))
					if err != nil || string(got) != want {
						t.Fatalf("migrated %s = %q: %v", name, got, err)
					}
				}
				backlink, err := os.ReadFile(filepath.Join(repo, ".git", "worktrees", branch, "gitdir"))
				if err != nil || filepath.Clean(filepath.FromSlash(strings.TrimSpace(string(backlink)))) != filepath.Join(path, ".git") {
					t.Fatalf("migrated Git backlink = %q: %v", backlink, err)
				}
			}
		})
	}
}

func TestMigrateLegacyProfilePreservesUnverifiedGitLinks(t *testing.T) {
	t.Parallel()
	for _, kind := range []string{"internal primary", "missing admin", "foreign backlink"} {
		t.Run(kind, func(t *testing.T) {
			t.Parallel()
			fixture := newLegacyMigrationFixture(t)
			repo := filepath.Join(fixture.outside, "repository")
			checkout := filepath.Join(fixture.legacy, "worktrees", "linked")
			if kind == "internal primary" {
				repo = filepath.Join(fixture.legacy, "projects", "inside")
				checkout = filepath.Join(fixture.outside, "linked")
			}
			initMigrationRepository(t, repo)
			migrationGit(t, "-C", repo, "worktree", "add", "-b", "linked", checkout)
			admin := filepath.Join(repo, ".git", "worktrees", "linked")
			if kind == "missing admin" {
				offline := filepath.Join(fixture.outside, "offline")
				if err := os.Rename(admin, offline); err != nil {
					t.Fatal(err)
				}
				admin = offline
			} else if kind == "foreign backlink" {
				if err := os.WriteFile(filepath.Join(admin, "gitdir"), []byte(filepath.Join(fixture.outside, "unrelated", ".git")+"\n"), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			before := make(map[string][]byte)
			for _, path := range []string{filepath.Join(admin, "gitdir"), filepath.Join(checkout, ".git"), filepath.Join(fixture.legacy, "storage", "node.turso")} {
				body, err := os.ReadFile(path)
				if err != nil {
					t.Fatal(err)
				}
				before[path] = body
			}
			if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err == nil {
				t.Fatal("migration accepted an unverified worktree")
			}
			for path, expected := range before {
				actual, err := os.ReadFile(path)
				if err != nil || !bytes.Equal(actual, expected) {
					t.Fatalf("refusal changed %s: %v", path, err)
				}
			}
			if exists, err := legacyProfileExists(fixture.target); err != nil || exists {
				t.Fatalf("linked-worktree refusal published target: %v", err)
			}
		})
	}
}
