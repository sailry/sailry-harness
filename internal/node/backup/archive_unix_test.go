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
	// A file under constant writes never reads the same twice.
	growing, err := os.OpenFile(filepath.Join(item, "growing"), os.O_WRONLY|os.O_CREATE|os.O_APPEND, 0o600)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := growing.Write(make([]byte, 8*1024*1024)); err != nil {
		t.Fatal(err)
	}
	stop, stopped := make(chan struct{}), make(chan struct{})
	go func() {
		defer close(stopped)
		for {
			select {
			case <-stop:
				return
			default:
				_, _ = growing.Write([]byte{1})
			}
		}
	}()
	defer func() {
		close(stop)
		<-stopped
		_ = growing.Close()
	}()

	snapshot := fixtureSnapshot(t, profile, files, append([]string{"attachments/gone"}, fixtureRoots...))
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
	if _, err := os.Lstat(filepath.Join(snapshot.Directory, "artifacts", "session", "item", "growing")); !os.IsNotExist(err) {
		t.Fatalf("a skipped file stayed in the snapshot: %v", err)
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
