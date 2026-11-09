package core

import (
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProfileLockExcludes(t *testing.T) {
	t.Parallel()
	t.Run("exclusive ownership and release", func(t *testing.T) {
		path := filepath.Join(t.TempDir(), "profile")
		profile, err := AcquireProfile(path)
		if err != nil {
			t.Fatal(err)
		}
		defer profile.Close()
		lockPath := filepath.Join(path, "node.lock")
		before, err := os.Stat(lockPath)
		if err != nil {
			t.Fatal(err)
		}
		for range 2 {
			_, err := AcquireProfile(path)
			var inUse *ProfileInUseError
			if !errors.As(err, &inUse) || inUse.Path != profile.Path {
				t.Fatalf("second owner: %v", err)
			}
		}
		if err := profile.Close(); err != nil {
			t.Fatal(err)
		}
		replacement, err := AcquireProfile(path)
		if err != nil {
			t.Fatal(err)
		}
		defer replacement.Close()
		after, err := os.Stat(lockPath)
		if err != nil || !os.SameFile(before, after) {
			t.Fatalf("lock file was replaced: %v", err)
		}
	})
	t.Run("private directories and file family", func(t *testing.T) {
		profile, err := AcquireProfile(filepath.Join(t.TempDir(), "profile"))
		if err != nil {
			t.Fatal(err)
		}
		defer profile.Close()
		temporary, err := Temporary(profile.Path)
		if err != nil || temporary != filepath.Join(profile.Path, "tmp") {
			t.Fatalf("temporary path: %s, %v", temporary, err)
		}
		for _, directory := range []string{profile.Path, filepath.Join(profile.Path, "storage"), temporary} {
			info, err := os.Stat(directory)
			if err != nil || !info.IsDir() {
				t.Fatalf("private directory %s: %v", directory, err)
			}
			if runtime.GOOS != "windows" && info.Mode().Perm() != 0o700 {
				t.Fatalf("private mode: %s", info.Mode())
			}
		}
		path := filepath.Join(profile.Path, "storage", "node.sqlite")
		want := []string{path, path + "-wal", path + "-shm"}
		if got := DatabaseFiles(path); !slices.Equal(got, want) {
			t.Fatalf("database family: %v", got)
		}
	})
	t.Run("resolves default without writing", func(t *testing.T) {
		home := filepath.Join(t.TempDir(), "user")
		path, err := dataDirFromHome("~/.sailry-go", home)
		if err != nil || path != filepath.Join(home, ".sailry-go") {
			t.Fatalf("default path: %s, %v", path, err)
		}
		if _, err := os.Stat(home); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("resolving the default wrote a directory: %v", err)
		}
		for _, home := range []string{"", "relative"} {
			if _, err := dataDirFromHome("~/.sailry-go", home); err == nil || !strings.Contains(err.Error(), "--data-dir") {
				t.Fatalf("invalid home accepted: %q, %v", home, err)
			}
		}
	})
	t.Run("exact workspace identities", func(t *testing.T) {
		profile, err := filepath.EvalSymlinks(t.TempDir())
		if err != nil {
			t.Fatal(err)
		}
		const identity = "0ed44566-789b-4abc-9abc-def123456789"
		worktree, _ := protocol.ParseWorktreeID(identity)
		session, _ := protocol.ParseSessionID(identity)
		database, _ := protocol.ParseDatabaseID(identity)
		ssh, _ := protocol.ParseSshID(identity)
		databaseRoot, err := ConnectionWorkspace(profile, protocol.ConnectionResource{Kind: "database", Data: database})
		if err != nil {
			t.Fatal(err)
		}
		sshRoot, err := ConnectionWorkspace(profile, protocol.ConnectionResource{Kind: "ssh", Data: &ssh})
		if err != nil {
			t.Fatal(err)
		}
		binding := protocol.PluginConversationBinding{Package: protocol.PluginReference{Name: "fixture-assistant", Digest: "first", SettingsRevision: 1}, ID: "notes"}
		assistant, err := AssistantWorkspace(profile, binding)
		if err != nil {
			t.Fatal(err)
		}
		revised := binding
		revised.Package.Digest, revised.Package.SettingsRevision = "second", 2
		if root, err := AssistantWorkspace(profile, revised); err != nil || root != assistant {
			t.Fatalf("package update changed workspace identity: %s, %v", root, err)
		}
		revised.ID = "other"
		if root, _ := AssistantWorkspace(profile, revised); root == assistant {
			t.Fatal("different assistant reused workspace identity")
		}
		roots := []string{
			filepath.Join(profile, "worktrees", worktree.String()), filepath.Join(Worktrees(profile), worktree.String()),
			SessionWorkspace(profile, session), databaseRoot, sshRoot, assistant,
		}
		for _, root := range roots {
			if !ManagedWorkspace(root, profile) || ManagedWorkspace(filepath.Join(root, "nested"), profile) || ManagedWorkspace(filepath.Dir(root), profile) {
				t.Fatalf("managed workspace ownership: %s", root)
			}
		}
		for _, relative := range []string{"storage", "plugins/data", "tmp", "worktrees/invalid", "worktrees/" + strings.ToUpper(identity), "workspaces/sessions/00000000-0000-0000-0000-000000000000", "connections/other/" + identity + "/workspace", "assistants/invalid/workspace"} {
			if ManagedWorkspace(filepath.Join(profile, filepath.FromSlash(relative)), profile) {
				t.Fatalf("invalid workspace accepted: %s", relative)
			}
		}
		for _, suffix := range []string{"worktrees/./" + identity, "worktrees//" + identity} {
			root := profile + string(filepath.Separator) + filepath.FromSlash(suffix)
			if ManagedWorkspace(root, profile) {
				t.Fatalf("workspace alias accepted: %s", root)
			}
		}
		if ManagedWorkspace("worktrees", "") || ManagedWorkspace(profile, profile) {
			t.Fatal("non-workspace root accepted")
		}
	})
}

func TestControllerProfileRefused(t *testing.T) {
	t.Parallel()
	t.Run("controller profile", func(t *testing.T) {
		path := filepath.Join(t.TempDir(), "controller")
		if err := PrivateDirectory(path); err != nil {
			t.Fatal(err)
		}
		peers := filepath.Join(path, "peers.json")
		if err := os.WriteFile(peers, []byte("{}"), 0o600); err != nil {
			t.Fatal(err)
		}
		_, err := AcquireProfile(path)
		var invalid *InvalidProfileError
		if !errors.As(err, &invalid) || !strings.Contains(invalid.Reason, "controller profile") {
			t.Fatalf("controller profile accepted: %v", err)
		}
		if _, err := os.Stat(filepath.Join(path, "storage")); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("controller refusal prepared storage: %v", err)
		}
		if err := os.Remove(peers); err != nil {
			t.Fatal(err)
		}
		profile, err := AcquireProfile(path)
		if err != nil {
			t.Fatalf("refusal leaked ownership: %v", err)
		}
		defer profile.Close()
	})
	t.Run("invalid profile paths", func(t *testing.T) {
		if _, err := AcquireProfile("relative"); err == nil {
			t.Fatal("relative profile accepted")
		}
		if runtime.GOOS == "windows" {
			return
		}
		base := t.TempDir()
		for _, name := range []string{"profile", "temporary", "lock", "permissions"} {
			t.Run(name, func(t *testing.T) {
				root := filepath.Join(base, name)
				if err := os.Mkdir(root, 0o700); err != nil {
					t.Fatal(err)
				}
				outside := t.TempDir()
				switch name {
				case "profile":
					root = filepath.Join(base, "profile-link")
					if err := os.Symlink(outside, root); err != nil {
						t.Fatal(err)
					}
				case "temporary", "lock":
					leaf := "tmp"
					if name == "lock" {
						leaf = "node.lock"
					}
					if err := os.Symlink(outside, filepath.Join(root, leaf)); err != nil {
						t.Fatal(err)
					}
				case "permissions":
					if err := os.Chmod(root, 0o755); err != nil {
						t.Fatal(err)
					}
				}
				if profile, err := AcquireProfile(root); err == nil {
					_ = profile.Close()
					t.Fatal("invalid profile accepted")
				}
				entries, err := os.ReadDir(outside)
				if err != nil || len(entries) != 0 {
					t.Fatalf("outside directory changed: %v, %v", entries, err)
				}
			})
		}
	})
}
