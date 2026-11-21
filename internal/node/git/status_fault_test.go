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
