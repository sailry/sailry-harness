//go:build unix

package backup

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"syscall"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// A workspace holds links, special files, files it cannot read and files that
// are being written. None of them may fail the backup of the databases.
// Over 1 s: one archive is sealed and opened at the production work factor.
func TestBackupAwkwardContent(t *testing.T) {
	t.Parallel()
	profile, files := fixtureProfile(t)
	item := filepath.Join(profile, "artifacts", "session", "item")
	links := map[string]string{
		"artifacts/session/item/bin/tool": "../1.html",
		"artifacts/session/item/dangling": "missing",
		"artifacts/session/item/linked":   "bin",
		"artifacts/session/item/python":   "/usr/bin/python3",
	}
	if err := os.Mkdir(filepath.Join(item, "bin"), 0o700); err != nil {
		t.Fatal(err)
	}
	for path, target := range links {
		if err := os.Symlink(target, filepath.Join(profile, filepath.FromSlash(path))); err != nil {
			t.Fatal(err)
		}
	}
	if err := syscall.Mkfifo(filepath.Join(item, "pipe"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(item, `back\slash`), []byte("name"), 0o600); err != nil {
		t.Fatal(err)
	}
	skipped := []Skipped{
		{Path: `artifacts/session/item/back\slash`, Reason: skipName},
		{Path: "artifacts/session/item/growing", Reason: skipChanged},
		{Path: "artifacts/session/item/pipe", Reason: skipSpecial},
		{Path: "attachments/gone", Reason: skipMissing},
	}
	if os.Geteuid() != 0 {
		if err := os.WriteFile(filepath.Join(item, "secret"), []byte("unreadable"), 0); err != nil {
			t.Fatal(err)
		}
		skipped = append(skipped, Skipped{Path: "artifacts/session/item/secret", Reason: skipUnreadable})
	}
	// Three files are written while the snapshot copies them, each exactly
	// between its copy and the copy's verification, at every attempt: one
	// grows, one is rewritten in place a second later, and one is rewritten
	// in place leaving its size and modification time as they were.
	growing, later, same := filepath.Join(item, "growing"), filepath.Join(item, "later"), filepath.Join(item, "same")
	for _, path := range []string{growing, later, same} {
		if err := os.WriteFile(path, bytes.Repeat([]byte{0}, 300*1024), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	// An old file is trusted by its timestamps, a recent one is read twice.
	old := time.Now().Add(-time.Hour)
	if err := os.Chtimes(later, old, old); err != nil {
		t.Fatal(err)
	}
	skipped = append(skipped,
		Skipped{Path: "artifacts/session/item/later", Reason: skipChanged},
		Skipped{Path: "artifacts/session/item/same", Reason: skipChanged})
	copies := map[string]int{}
	rewrite := func(path string, flag int, keep bool) {
		t.Helper()
		info, err := os.Stat(path)
		if err != nil {
			t.Fatal(err)
		}
		file, err := os.OpenFile(path, os.O_WRONLY|flag, 0o600)
		if err != nil {
			t.Fatal(err)
		}
		_, err = file.Write([]byte{byte(copies[path])})
		if closeErr := file.Close(); err != nil || closeErr != nil {
			t.Fatal(err, closeErr)
		}
		stamp := info.ModTime()
		if !keep {
			stamp = stamp.Add(time.Second)
		}
		if err := os.Chtimes(path, stamp, stamp); err != nil {
			t.Fatal(err)
		}
	}
	copied := func(relative string) {
		path := filepath.Join(profile, filepath.FromSlash(relative))
		copies[path]++
		switch path {
		case growing:
			rewrite(path, os.O_APPEND, true)
		case later:
			rewrite(path, 0, false)
		case same:
			rewrite(path, 0, true)
		}
	}

	snapshot := fixtureSnapshotCopied(t, profile, files, append([]string{"attachments/gone"}, fixtureRoots...), copied)
	for _, path := range []string{growing, later, same} {
		if copies[path] != readAttempts {
			t.Fatalf("%s was copied %d times, want %d", filepath.Base(path), copies[path], readAttempts)
		}
	}
	manifest := snapshot.Manifest
	slices.SortFunc(manifest.Skipped, func(left, right Skipped) int { return strings.Compare(left.Path, right.Path) })
	slices.SortFunc(skipped, func(left, right Skipped) int { return strings.Compare(left.Path, right.Path) })
	if !slices.Equal(manifest.Skipped, skipped) || int(manifest.SkippedTotal) != len(skipped) {
		t.Fatalf("skipped = %v (%d), want %v", manifest.Skipped, manifest.SkippedTotal, skipped)
	}
	if len(manifest.Links) != len(links) {
		t.Fatalf("links = %v", manifest.Links)
	}
	for _, link := range manifest.Links {
		if links[link.Path] != link.Target {
			t.Fatalf("link %q -> %q", link.Path, link.Target)
		}
	}
	if len(manifest.Files) != len(files) {
		t.Fatalf("files = %d, want the %d ordinary files", len(manifest.Files), len(files))
	}
	for _, name := range []string{"growing", "later", "same"} {
		if _, err := os.Lstat(filepath.Join(snapshot.Directory, "artifacts", "session", "item", name)); !os.IsNotExist(err) {
			t.Fatalf("skipped %s stayed in the snapshot: %v", name, err)
		}
	}

	t.Run("the identity is never skipped", func(t *testing.T) {
		broken := filepath.Join(t.TempDir(), "profile")
		if err := os.MkdirAll(broken, 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.Symlink(filepath.Join(profile, "link.key"), filepath.Join(broken, "link.key")); err != nil {
			t.Fatal(err)
		}
		for _, name := range []string{"link.key", "absent.key"} {
			strict, err := NewSnapshot(broken, protocol.NodeID{1})
			if err != nil {
				t.Fatal(err)
			}
			defer strict.Close()
			if name == "absent.key" {
				if err := os.Remove(filepath.Join(broken, "link.key")); err != nil {
					t.Fatal(err)
				}
			}
			if err := strict.Capture(t.Context(), []string{"link.key"}); err == nil {
				t.Fatalf("a backup without a readable identity succeeded (%s)", name)
			}
		}
	})

	t.Run("links are restored as links", func(t *testing.T) {
		packed := fixturePack(t, snapshot)
		sealed, err := os.Open(packed.Path)
		if err != nil {
			t.Fatal(err)
		}
		defer sealed.Close()
		opened, err := Open(t.Context(), sealed, protocol.NewSecret(fixturePassphrase))
		if err != nil {
			t.Fatal(err)
		}
		staged, err := Stage(t.Context(), opened, t.TempDir(), nil, "", packed.Size, packed.SHA256)
		if err != nil {
			t.Fatal(err)
		}
		defer staged.Close()
		if len(staged.Manifest.Skipped) != len(skipped) || len(staged.Manifest.Links) != len(links) {
			t.Fatalf("restored manifest = %#v", staged.Manifest)
		}
		// Staging holds no link: they are created only in the profile.
		if _, err := os.Lstat(filepath.Join(staged.Directory, "artifacts", "session", "item", "python")); !os.IsNotExist(err) {
			t.Fatalf("link in staging: %v", err)
		}
		target := filepath.Join(t.TempDir(), "restored")
		if err := os.Mkdir(target, 0o700); err != nil {
			t.Fatal(err)
		}
		destination, err := os.OpenRoot(target)
		if err != nil {
			t.Fatal(err)
		}
		defer destination.Close()
		if err := Transfer(context.Background(), staged.Directory, destination, staged.Manifest); err != nil {
			t.Fatal(err)
		}
		for path, want := range links {
			if got, err := os.Readlink(filepath.Join(target, filepath.FromSlash(path))); err != nil || got != want {
				t.Fatalf("restored link %q -> %q, %v", path, got, err)
			}
		}
		for name, want := range files {
			if got, err := os.ReadFile(filepath.Join(target, filepath.FromSlash(name))); err != nil || !bytes.Equal(got, want) {
				t.Fatalf("restored %q differs: %v", name, err)
			}
		}
		for _, entry := range skipped {
			if _, err := os.Lstat(filepath.Join(target, filepath.FromSlash(entry.Path))); !os.IsNotExist(err) {
				t.Fatalf("skipped %q was restored: %v", entry.Path, err)
			}
		}
	})
}
