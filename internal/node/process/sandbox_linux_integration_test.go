//go:build integration && linux

package process

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestNativeProtectedParentsLinux(t *testing.T) {
	root := t.TempDir()
	files := map[string]string{
		"nested/repository/.git/config": "directory metadata",
		"nested/worktree/.git":          "file metadata",
		".sailry/worktrees/retained":    "managed checkout",
	}
	for relative, value := range files {
		path := filepath.Join(root, filepath.FromSlash(relative))
		if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(value), 0600); err != nil {
			t.Fatal(err)
		}
	}
	service, err := New(Options{
		Profile:     t.TempDir(),
		Environment: NewEnvironment([]string{"PATH=/usr/bin:/bin", "HOME=" + t.TempDir()}),
		CaptureManaged: func(string) (string, error) {
			return filepath.Join(root, ".sailry", "worktrees"), nil
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer service.Close()
	result, err := service.Execute(context.Background(), Launch{
		ID:      protocol.NewRequestID(),
		Session: protocol.NewSessionID(),
		Turn:    protocol.NewTurnID(),
		Root:    root,
		Command: `set -eu
deny() {
	if "$@" >/dev/null 2>&1; then
		printf 'protected operation succeeded: %s\n' "$*"
		exit 1
	fi
}
deny mv nested moved
deny mv nested/repository nested/replaced
deny mv nested/worktree nested/replaced-worktree
deny rm nested/repository/.git/config
deny rm nested/worktree/.git
deny mv .sailry moved-managed
deny mv .sailry/worktrees .sailry/replaced
printf changed > nested/repository/document
mv nested/repository/document nested/repository/renamed
`,
		Access:    protocol.ProcessAccessWorkspace,
		TimeoutMs: 10000,
	})
	if err != nil {
		t.Fatal(err)
	}
	if code, ok := result.Outcome.Data.(int32); result.Outcome.Kind != "exited" || !ok || code != 0 {
		t.Fatalf("protected parent operations: %+v", result)
	}
	for relative, want := range files {
		data, err := os.ReadFile(filepath.Join(root, filepath.FromSlash(relative)))
		if err != nil || string(data) != want {
			t.Fatalf("protected file changed: %s = %q, %v", relative, data, err)
		}
	}
	data, err := os.ReadFile(filepath.Join(root, "nested", "repository", "renamed"))
	if err != nil || string(data) != "changed" {
		t.Fatalf("ordinary workspace edits failed: %q, %v", data, err)
	}
}
