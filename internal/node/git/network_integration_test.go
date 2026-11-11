//go:build integration && unix

package git

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// A scripted executable named git stands in for the remote side, so the probes
// reach no network host. The script delegates repository work to the real Git.
func TestNetworkGitIntegration(t *testing.T) {
	real, err := exec.LookPath("git")
	if err != nil {
		t.Skip("Git executable is required")
	}
	ctx := context.Background()
	// script installs a fake git whose body receives the clone destination as
	// $target and the real executable as $REAL_GIT.
	script := func(t *testing.T, body string) *Service {
		t.Helper()
		bin, home := t.TempDir(), t.TempDir()
		text := "#!/bin/sh\nfor target; do :; done\n" + body + "\n"
		if err := os.WriteFile(filepath.Join(bin, "git"), []byte(text), 0o755); err != nil {
			t.Fatal(err)
		}
		t.Setenv("PATH", bin+string(os.PathListSeparator)+os.Getenv("PATH"))
		s := New(Options{Environment: []string{"PATH=" + os.Getenv("PATH"), "HOME=" + home, "REAL_GIT=" + real}})
		// Generous for a busy machine; the scripts that must stall lower it.
		s.idle = 1500 * time.Millisecond
		return s
	}
	stalling := func(t *testing.T, body string) *Service {
		s := script(t, body)
		s.idle = 300 * time.Millisecond
		return s
	}
	destination := func(t *testing.T) string {
		parent, err := filepath.EvalSymlinks(t.TempDir())
		if err != nil {
			t.Fatal(err)
		}
		return filepath.Join(parent, "checkout")
	}
	absent := func(t *testing.T, path string) {
		t.Helper()
		if _, err := os.Lstat(path); !os.IsNotExist(err) {
			t.Fatalf("partial clone directory kept: %v", err)
		}
	}
	t.Run("clone_outlives_the_idle_limit_while_reporting_progress", func(t *testing.T) {
		// Longer than the silence limit in total, never silent for all of it.
		s := script(t, `for step in 1 2 3 4 5 6 7 8; do printf 'Receiving objects: %s0%%\r' "$step" >&2; sleep 0.3; done
exec "$REAL_GIT" init -q "$target"`)
		target := destination(t)
		path, err := s.Clone(ctx, "https://example.invalid/repository.git", target, nil)
		if err != nil || path != target {
			t.Fatal(path, err)
		}
		if _, err := os.Stat(filepath.Join(target, ".git")); err != nil {
			t.Fatal(err)
		}
		// The capture keeps what a terminal would show, not every percentage.
		if output := s.Output(filepath.Dir(target)); strings.Count(output, "Receiving objects") > 1 || !strings.Contains(output, "clone --progress -- https://example.invalid/repository.git") {
			t.Fatalf("clone output: %q", output)
		}
	})
	t.Run("silent_clone_is_stopped_and_its_directory_removed", func(t *testing.T) {
		s := stalling(t, `echo partial > "$target/partial.pack"; sleep 30`)
		target := destination(t)
		started := time.Now()
		_, err := s.Clone(ctx, "https://example.invalid/repository.git", target, nil)
		requireFault(t, err, protocol.ErrorCodeOutcomeUnknown)
		if elapsed := time.Since(started); elapsed > 10*time.Second {
			t.Fatalf("stalled clone ran %s", elapsed)
		}
		absent(t, target)
		// A retry finds the destination free.
		s = script(t, `exec "$REAL_GIT" init -q "$target"`)
		if _, err := s.Clone(ctx, "https://example.invalid/repository.git", target, nil); err != nil {
			t.Fatal(err)
		}
	})
	t.Run("cancelled_clone_removes_its_directory", func(t *testing.T) {
		s := script(t, `echo partial > "$target/partial.pack"; while :; do echo working >&2; sleep 0.1; done`)
		target := destination(t)
		cancelled, cancel := context.WithCancel(ctx)
		time.AfterFunc(300*time.Millisecond, cancel)
		_, err := s.Clone(cancelled, "https://example.invalid/repository.git", target, nil)
		requireFault(t, err, protocol.ErrorCodeOutcomeUnknown)
		absent(t, target)
	})
	t.Run("refused_clone_is_a_plain_failure", func(t *testing.T) {
		for stderr, code := range map[string]protocol.ErrorCode{
			"fatal: Authentication failed for 'https://example.invalid/repository.git/'":                         protocol.ErrorCodePermissionDenied,
			"fatal: repository 'https://example.invalid/missing.git/' not found":                                 protocol.ErrorCodeInvalidRequest,
			"fatal: unable to access 'https://example.invalid/': Could not resolve host: example.invalid":        protocol.ErrorCodeInvalidRequest,
			"fatal: Unable to create '/work/.git/index.lock': File exists.":                                      protocol.ErrorCodeInvalidRequest,
			"fatal: Remote branch missing not found in upstream origin\nfatal: Could not read from remote (gpg)": protocol.ErrorCodeInvalidRequest,
		} {
			s := script(t, `mkdir -p "$target/.git"; echo "`+strings.ReplaceAll(stderr, "\n", `"; echo "`)+`" >&2; exit 128`)
			target := destination(t)
			_, err := s.Clone(ctx, "https://example.invalid/repository.git", target, nil)
			requireFault(t, err, code)
			absent(t, target)
		}
	})
	t.Run("existing_destination_is_kept", func(t *testing.T) {
		s := script(t, `exit 1`)
		target := destination(t)
		if err := os.Mkdir(target, 0o755); err != nil {
			t.Fatal(err)
		}
		_, err := s.Clone(ctx, "https://example.invalid/repository.git", target, nil)
		requireFault(t, err, protocol.ErrorCodeConflict)
		if _, err := os.Stat(target); err != nil {
			t.Fatal("a destination the clone did not create was removed", err)
		}
	})
	t.Run("fetch_has_no_total_deadline_and_stalls_are_uncertain", func(t *testing.T) {
		root, _, commit := fixture(t)
		action := func(s *Service) error {
			r, err := open(root)
			if err != nil {
				t.Fatal(err)
			}
			index, err := indexRevision(root, r)
			if err != nil {
				t.Fatal(err)
			}
			return s.Action(ctx, root, protocol.CommandRunGitActionPayload{Action: protocol.GitAction{Kind: "fetch", Data: protocol.GitActionFetchPayload{All: true}}, ExpectedIndex: index, ExpectedHead: &commit, ExpectedBranch: ptr("main")})
		}
		if err := action(script(t, `for step in 1 2 3 4 5 6 7 8; do echo "remote: $step" >&2; sleep 0.3; done`)); err != nil {
			t.Fatal(err)
		}
		requireFault(t, action(stalling(t, `sleep 30`)), protocol.ErrorCodeOutcomeUnknown)
		requireFault(t, action(script(t, `echo "fatal: Authentication failed" >&2; exit 128`)), protocol.ErrorCodePermissionDenied)
	})
}
