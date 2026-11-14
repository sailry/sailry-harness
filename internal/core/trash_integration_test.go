//go:build integration && (darwin || linux || windows)

package core

import (
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"
	"time"

	"github.com/Bios-Marcel/wastebasket/v2"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestTrashNative(t *testing.T) {
	t.Run("confined relocation", func(t *testing.T) {
		root, err := filepath.EvalSymlinks(t.TempDir())
		if err != nil {
			t.Fatal(err)
		}
		files, err := OpenFiles(root, RestrictedFiles)
		if err != nil {
			t.Fatal(err)
		}
		defer files.Close()
		for _, relative := range []string{"", "../outside", "/absolute", "folder/../file", ".git", "a/.GIT/config", "missing", ".sailry/worktrees/fixture"} {
			if err := files.relocateTrash(relative, func(string) error { panic("invalid path reached Trash") }); err == nil {
				t.Errorf("invalid path accepted: %q", relative)
			}
		}
		if err := os.WriteFile(filepath.Join(root, "file"), []byte("retained"), 0o600); err != nil {
			t.Fatal(err)
		}
		for _, operation := range []func(string) error{
			func(string) error { return trashUnknown() },
			func(string) error { return nil },
		} {
			assertTrashFault(t, files.relocateTrash("file", operation), protocol.ErrorCodeOutcomeUnknown)
		}
		stamp, err := files.SourceRevision("file")
		if err != nil {
			t.Fatal(err)
		}
		for _, pair := range [][2]string{{"invalid", stamp}, {contentRevision([]byte("retained")), "invalid"}} {
			assertTrashFault(t, files.checkedTrash(context.Background(), "file", pair[0], pair[1], func(string) error { panic("invalid revision reached Trash") }), protocol.ErrorCodeInvalidRequest)
		}
		assertTrashFault(t, files.checkedTrash(context.Background(), "file", contentRevision([]byte("different")), stamp, func(string) error { panic("changed file reached Trash") }), protocol.ErrorCodeRevisionConflict)
		recovered := filepath.Join(t.TempDir(), "file")
		if err := files.checkedTrash(context.Background(), "file", contentRevision([]byte("retained")), stamp, func(target string) error { return os.Rename(target, recovered) }); err != nil {
			t.Fatal(err)
		}
		if body, err := os.ReadFile(recovered); err != nil || string(body) != "retained" {
			t.Fatalf("verified source changed: %q, %v", body, err)
		}
		if runtime.GOOS != "windows" {
			if err := os.Mkdir(filepath.Join(root, "folder"), 0o700); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(root, "folder", "file"), []byte("retained"), 0o600); err != nil {
				t.Fatal(err)
			}
			err := files.relocateTrash("folder/file", func(target string) error {
				if err := os.Rename(target, filepath.Join(root, "recovered")); err != nil {
					return err
				}
				if err := os.Rename(filepath.Join(root, "folder"), filepath.Join(root, "moved")); err != nil {
					return err
				}
				return os.Mkdir(filepath.Join(root, "folder"), 0o700)
			})
			assertTrashFault(t, err, protocol.ErrorCodeOutcomeUnknown)
		}
	})
	for _, kind := range []string{"file", "directory", "symlink", "dangling symlink"} {
		t.Run(kind, func(t *testing.T) {
			root, err := filepath.EvalSymlinks(t.TempDir())
			if err != nil {
				t.Fatal(err)
			}
			original := filepath.Join(root, "trash-fixture")
			external := filepath.Join(t.TempDir(), "target")
			switch kind {
			case "file":
				if err := os.WriteFile(original, []byte("recoverable"), 0o600); err != nil {
					t.Fatal(err)
				}
			case "directory":
				if err := os.Mkdir(original, 0o700); err != nil {
					t.Fatal(err)
				}
				if err := os.WriteFile(filepath.Join(original, "fixture.txt"), []byte("recoverable"), 0o600); err != nil {
					t.Fatal(err)
				}
			case "symlink":
				if err := os.WriteFile(external, []byte("outside"), 0o600); err != nil {
					t.Fatal(err)
				}
				fallthrough
			case "dangling symlink":
				if err := os.Symlink(external, original); err != nil {
					if runtime.GOOS == "windows" {
						t.Skipf("symlink fixture unrun: %v", err)
					}
					t.Fatal(err)
				}
			}
			files, err := OpenFiles(root, ManualFiles)
			if err != nil {
				t.Fatal(err)
			}
			defer files.Close()
			var destination string
			t.Cleanup(func() { restoreTrashFixture(t, original, destination) })
			if err := files.relocateTrash("trash-fixture", func(target string) error {
				var err error
				destination, err = trashNative(target)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			if _, err := os.Lstat(original); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("native Trash retained the source: %v", err)
			}
			restoreTrashFixture(t, original, destination)
			metadata, err := os.Lstat(original)
			if err != nil {
				t.Fatal(err)
			}
			switch kind {
			case "file", "directory":
				content := original
				if kind == "directory" {
					content = filepath.Join(original, "fixture.txt")
				}
				if body, err := os.ReadFile(content); err != nil || string(body) != "recoverable" {
					t.Fatalf("recovered fixture changed: %q, %v", body, err)
				}
			default:
				if metadata.Mode()&os.ModeSymlink == 0 {
					t.Fatal("recovered entry is not a symlink")
				}
				if target, err := os.Readlink(original); err != nil || target != external {
					t.Fatalf("recovered link changed: %q, %v", target, err)
				}
				if kind == "symlink" {
					if body, err := os.ReadFile(external); err != nil || string(body) != "outside" {
						t.Fatalf("link target changed: %q, %v", body, err)
					}
				}
			}
		})
	}
}

func restoreTrashFixture(t *testing.T, original, destination string) {
	t.Helper()
	if _, err := os.Lstat(original); err == nil {
		return
	} else if !errors.Is(err, os.ErrNotExist) {
		t.Errorf("inspect fixture before restore: %v", err)
		return
	}
	if destination != "" {
		if err := os.Rename(destination, original); err != nil {
			t.Errorf("restore fixture from native Trash: %v", err)
		}
		return
	}
	if runtime.GOOS == "darwin" {
		t.Error("native Trash did not return the fixture's recovery location")
		return
	}
	if runtime.GOOS == "windows" {
		restoreWindowsTrashFixture(t, original)
		return
	}
	result, err := wastebasket.Query(wastebasket.QueryOptions{Search: []string{original}})
	if err != nil {
		t.Errorf("query fixture in Trash: %v", err)
		return
	}
	matches := result.Matches[original]
	if len(matches) != 1 {
		t.Errorf("fixture has %d recovery matches, want one", len(matches))
		return
	}
	if err := matches[0].Restore(false); err != nil {
		t.Errorf("restore fixture from Trash: %v", err)
	}
}

func restoreWindowsTrashFixture(t *testing.T, original string) {
	t.Helper()
	// wastebasket.Query follows the recycled entry with os.Stat, hiding broken
	// links. The Shell restores the exact fixture without dereferencing it.
	const script = `param([string]$original)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$shell = New-Object -ComObject Shell.Application
$bin = $shell.NameSpace(10)
if ($null -eq $bin) { throw 'The Shell Recycle Bin is unavailable' }
$parent = [System.IO.Path]::GetDirectoryName($original)
$name = [System.IO.Path]::GetFileName($original)
$matches = @(foreach ($item in $bin.Items()) {
    $from = [string]$item.ExtendedProperty('System.Recycle.DeletedFrom')
    $leaf = [string]$item.ExtendedProperty('System.FileName')
    if ([string]::Equals($from, $parent, [System.StringComparison]::OrdinalIgnoreCase) -and [string]::Equals($leaf, $name, [System.StringComparison]::OrdinalIgnoreCase)) { $item }
})
if ($matches.Count -ne 1) { throw ('Fixture has ' + $matches.Count + ' recovery matches, want one') }
$matches[0].InvokeVerb('undelete')
`
	path := filepath.Join(t.TempDir(), "restore.ps1")
	if err := os.WriteFile(path, []byte(script), 0o600); err != nil {
		t.Errorf("prepare fixture restore: %v", err)
		return
	}
	// Shell verbs can finish asynchronously. This bounds the one invocation
	// and observes its result; cleanup also runs after t.Context is cancelled.
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	output, err := exec.CommandContext(ctx, "powershell.exe", "-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", path, original).CombinedOutput()
	if err != nil {
		t.Errorf("restore fixture from the Shell Recycle Bin: %v: %s", err, output)
		return
	}
	tick := time.NewTicker(10 * time.Millisecond)
	defer tick.Stop()
	for {
		if _, err := os.Lstat(original); err == nil {
			return
		} else if !errors.Is(err, os.ErrNotExist) {
			t.Errorf("inspect restored fixture: %v", err)
			return
		}
		select {
		case <-ctx.Done():
			t.Errorf("the Shell did not restore the fixture: %v", ctx.Err())
			return
		case <-tick.C:
		}
	}
}

func assertTrashFault(t *testing.T, err error, code protocol.ErrorCode) {
	t.Helper()
	var fault *protocol.Fault
	if !errors.As(err, &fault) || fault.Code != code {
		t.Fatalf("got %v, want fault %s", err, code)
	}
}
