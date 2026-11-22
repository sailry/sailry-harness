package git

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestInspectGitResamplesChangingMetadata(t *testing.T) {
	for _, reason := range []protocol.GitInspectionReason{protocol.GitInspectionIndexChanged, protocol.GitInspectionHeadChanged} {
		for _, continuous := range []bool{false, true} {
			name := string(reason) + "/settled"
			if continuous {
				name = string(reason) + "/continuous"
			}
			t.Run(name, func(t *testing.T) {
				root, repo, commit := fixture(t)
				index := filepath.Join(root, ".git", "index")
				calls := 0
				service := New(Options{Run: func(_ context.Context, _ string, args []string, _ []byte) (CommandResult, error) {
					if !slices.Equal(args, StatusArgs(nil)) {
						return CommandResult{}, errors.New("unexpected Git command")
					}
					calls++
					branch, staged := "main", ""
					if calls == 2 {
						if reason == protocol.GitInspectionHeadChanged {
							branch = "(detached)"
						} else {
							staged = "1 D. N... 100644 000000 000000 abc def file.txt\x00"
						}
					}
					if calls == 1 || continuous {
						var err error
						if reason == protocol.GitInspectionHeadChanged {
							ref := commit
							if calls == 2 {
								ref = "ref: refs/heads/main"
							}
							err = os.WriteFile(filepath.Join(root, ".git", "HEAD"), []byte(ref+"\n"), 0o600)
						} else if calls == 1 {
							err = os.Rename(index, index+".previous")
						} else {
							err = os.Rename(index+".previous", index)
						}
						if err != nil {
							return CommandResult{}, err
						}
					}
					return CommandResult{Stdout: []byte("# branch.oid " + commit + "\x00# branch.head " + branch + "\x00" + staged)}, nil
				}})
				out, err := service.Read(t.Context(), root, protocol.Command{Kind: "inspect_git"})
				if calls != 2 {
					t.Fatalf("inspect_git ran %d status commands: %v", calls, err)
				}
				if continuous {
					var fault *protocol.Fault
					if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeConflict || fault.GitInspectionReason != reason {
						t.Fatalf("persistent metadata changes: %v", err)
					}
					return
				}
				if err != nil {
					t.Fatal(err)
				}
				status := out.Data.(protocol.GitStatus)
				revision, err := indexRevision(root, repo)
				if err != nil || status.IndexRevision == nil || *status.IndexRevision != revision ||
					(reason == protocol.GitInspectionHeadChanged && status.Branch != nil) ||
					(reason == protocol.GitInspectionIndexChanged && (len(status.Entries) != 1 || status.Entries[0].Staged == nil)) {
					t.Fatalf("inspect_git did not return the new snapshot: %+v, %v", status, err)
				}
			})
		}
	}
}

func TestInspectGitRejectsUnverifiedResample(t *testing.T) {
	root, _, commit := fixture(t)
	for _, tc := range []struct {
		name, headers string
		runErr        error
		code          protocol.ErrorCode
		reason        protocol.GitInspectionReason
	}{
		{"stale commit", "# branch.oid " + strings.Repeat("1", 40) + "\x00# branch.head (detached)\x00", nil, protocol.ErrorCodeConflict, protocol.GitInspectionHeadChanged},
		{"stale branch", "# branch.oid " + commit + "\x00# branch.head main\x00", nil, protocol.ErrorCodeConflict, protocol.GitInspectionHeadChanged},
		{"missing branch", strings.Repeat("# branch.oid "+commit+"\x00", 2), nil, protocol.ErrorCodeUnavailable, protocol.GitInspectionIncompleteHeaders},
		{"runner not found", "", os.ErrNotExist, protocol.ErrorCodeNotFound, protocol.GitInspectionStatusCommandFailed},
	} {
		t.Run(tc.name, func(t *testing.T) {
			headPath := filepath.Join(root, ".git", "HEAD")
			if err := os.WriteFile(headPath, []byte("ref: refs/heads/main\n"), 0o600); err != nil {
				t.Fatal(err)
			}
			calls := 0
			s := New(Options{Run: func(_ context.Context, _ string, args []string, _ []byte) (CommandResult, error) {
				if !slices.Equal(args, StatusArgs(nil)) {
					return CommandResult{}, errors.New("unexpected Git command")
				}
				calls++
				if calls == 1 {
					err := os.WriteFile(headPath, []byte(commit+"\n"), 0o600)
					return CommandResult{Stdout: []byte("# branch.oid " + commit + "\x00# branch.head main\x00")}, err
				}
				return CommandResult{Stdout: []byte(tc.headers)}, tc.runErr
			}})
			_, err := s.Read(t.Context(), root, protocol.Command{Kind: "inspect_git"})
			var f *protocol.Fault
			if !errors.As(err, &f) || f.Code != tc.code || f.GitInspectionReason != tc.reason || calls != 2 {
				t.Fatalf("%d status calls, fault %+v, want 2 calls and %s/%s", calls, f, tc.code, tc.reason)
			}
		})
	}
}

func TestInspectGitFailureDiagnostics(t *testing.T) {
	const localCause = "could not start in /private/worktree"
	const stderr = "fatal: unexpected object /private/entry"
	for _, tc := range []struct {
		name, message string
		prepare       func(*testing.T, string)
		result        CommandResult
		runErr        error
		code          protocol.ErrorCode
		reason        protocol.GitInspectionReason
	}{
		{
			name: "nonregular index", message: "expected a bounded regular Git index",
			prepare: func(t *testing.T, root string) {
				index := filepath.Join(root, ".git", "index")
				if err := os.Remove(index); err != nil {
					t.Fatal(err)
				}
				if err := os.Mkdir(index, 0o700); err != nil {
					t.Fatal(err)
				}
			},
			code: protocol.ErrorCodeInvalidRequest, reason: protocol.GitInspectionIndexReadFailed,
		},
		{
			name: "runner error", message: "Git repository operation failed: " + localCause,
			runErr: errors.New(localCause), code: protocol.ErrorCodeUnavailable, reason: protocol.GitInspectionStatusCommandFailed,
		},
		{
			name: "Git exited", message: "Git operation failed (exit 128)",
			result: CommandResult{ExitCode: 128, Stderr: []byte(stderr)}, code: protocol.ErrorCodeInvalidRequest, reason: protocol.GitInspectionStatusNonzeroExit,
		},
		{
			name: "path encoding", message: "Git status contains unsupported path encoding",
			result: CommandResult{Stdout: []byte("? /private/\xff\x00"), InvalidUTF8: true}, code: protocol.ErrorCodeUnavailable, reason: protocol.GitInspectionUnsupportedEncoding,
		},
		{
			name: "cancelled", message: "Git read cancelled", runErr: context.Canceled, code: protocol.ErrorCodeCancelled,
		},
	} {
		t.Run(tc.name, func(t *testing.T) {
			root, _, _ := fixture(t)
			if tc.prepare != nil {
				tc.prepare(t, root)
			}
			service := New(Options{Run: func(_ context.Context, _ string, args []string, _ []byte) (CommandResult, error) {
				if !slices.Equal(args, StatusArgs(nil)) {
					t.Errorf("unexpected Git args: %q", args)
				}
				return tc.result, tc.runErr
			}})
			_, err := service.Read(t.Context(), root, protocol.Command{Kind: "inspect_git"})
			var fault *protocol.Fault
			if !errors.As(err, &fault) || fault.Code != tc.code || fault.GitInspectionReason != tc.reason || fault.Message != tc.message {
				t.Fatalf("inspect_git failure = %#v, %v", fault, err)
			}
			if tc.result.ExitCode != 0 && !strings.Contains(service.Output(root), stderr) {
				t.Fatal("native stderr is missing from the local Git output")
			}
		})
	}
}
