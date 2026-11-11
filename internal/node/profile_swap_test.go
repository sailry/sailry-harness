package node

import (
	"context"
	"database/sql"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Over 1 s: real backups of the profile's bundled packages are restored.
func TestSwapRestoredProfile(t *testing.T) {
	t.Parallel()
	ctx := context.Background()
	base, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	home := filepath.Join(base, "home")
	if err := os.Mkdir(home, 0o700); err != nil {
		t.Fatal(err)
	}
	profile := filepath.Join(home, "profile")
	node := backupFixtureNode(t, profile, nil)
	settings := backupFixtureConfigure(t, node, filepath.Join(base, "backups"))
	// A project folder inside the profile: its stored path follows the
	// profile. It is the user's folder, not a managed workspace, so the
	// backup leaves its content out instead of failing.
	workspace := filepath.Join(profile, "workspaces", "demo")
	if err := os.MkdirAll(workspace, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(workspace, "note.txt"), []byte("kept"), 0o600); err != nil {
		t.Fatal(err)
	}
	commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Demo", Path: workspace, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}})
	record := commandFixture(t, node, protocol.Command{Kind: "export_backup", Data: protocol.CommandExportBackupPayload{ExpectedRevision: settings.Revision}}).Data.(protocol.BackupRecord)
	// restore unpacks the backup as a new profile directory.
	restore := func(name string) string {
		t.Helper()
		target := filepath.Join(base, name)
		commandFixture(t, node, protocol.Command{Kind: "restore_backup", Data: protocol.CommandRestoreBackupPayload{
			ID: record.ID, ExpectedRevision: settings.Revision, Target: target, Location: protocol.BackupLocationLocal,
		}})
		return target
	}
	// stored is the project's path in the profile at path.
	stored := func(path string) (project string) {
		t.Helper()
		opened, err := OpenProfile(ctx, path)
		if err != nil {
			t.Fatal(err)
		}
		defer opened.Close()
		if err := opened.Store.Node.Read(ctx, func(db *sql.Conn) error {
			return db.QueryRowContext(ctx, "SELECT path FROM projects").Scan(&project)
		}); err != nil {
			t.Fatal(err)
		}
		return project
	}
	// Written after the backup: only the profile in use holds it.
	marker := filepath.Join("desktop", "preferences.json")
	if err := os.MkdirAll(filepath.Join(profile, "desktop"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(profile, marker), []byte("{}"), 0o600); err != nil {
		t.Fatal(err)
	}
	holds := func(path string) bool {
		_, err := os.Lstat(filepath.Join(path, marker))
		return err == nil
	}
	now := time.Date(2026, time.March, 7, 12, 0, 0, 0, time.UTC)
	restored := restore("restored")
	if got := stored(restored); got != filepath.Join(restored, "workspaces", "demo") {
		t.Fatalf("restored project path = %q", got)
	}

	// A restored profile is checked while the Node runs; the swap waits for its stop.
	if err := CheckRestoredProfile(ctx, profile, restored); err != nil {
		t.Fatalf("check of a restored profile: %v", err)
	}
	var owned *core.ProfileInUseError
	if _, err := SwapProfile(ctx, profile, restored, now); !errors.As(err, &owned) || owned.Path != profile {
		t.Fatalf("swap beneath a running Node: %v", err)
	}
	empty := filepath.Join(base, "empty")
	if err := os.Mkdir(empty, 0o700); err != nil {
		t.Fatal(err)
	}
	for name, candidate := range map[string]string{
		"a directory that does not exist": filepath.Join(base, "missing"),
		"a relative directory":            "restored",
		"the profile in use":              profile,
		"a directory inside the profile":  filepath.Join(profile, "storage"),
		"a directory around the profile":  home,
		"a directory without a database":  empty,
	} {
		if err := CheckRestoredProfile(ctx, profile, candidate); err == nil {
			t.Errorf("%s passed the check", name)
		}
		if _, err := SwapProfile(ctx, profile, candidate, now); err == nil {
			t.Errorf("%s was swapped in", name)
		}
	}
	if err := node.Stop(ctx); err != nil {
		t.Fatal(err)
	}
	// untouched reports the profile in use and the restored one as they were.
	untouched := func(restored string) {
		t.Helper()
		if !holds(profile) || holds(restored) {
			t.Fatal("the profile in use changed")
		}
		if got := stored(restored); got != filepath.Join(restored, "workspaces", "demo") {
			t.Fatalf("restored project path after a refused swap = %q", got)
		}
		if got := stored(profile); got != workspace {
			t.Fatalf("project path of the profile in use = %q", got)
		}
		siblings, err := filepath.Glob(profile + ".*")
		if err != nil || len(siblings) != 0 {
			t.Fatalf("a refused swap left %v, %v", siblings, err)
		}
	}
	untouched(restored)

	t.Run("a failure halfway leaves both profiles as they were", func(t *testing.T) {
		if os.Geteuid() == 0 {
			t.Skip("the superuser renames inside a read-only directory")
		}
		// Nothing can be renamed into or out of the profile's directory.
		if err := os.Chmod(home, 0o500); err != nil {
			t.Fatal(err)
		}
		_, err := SwapProfile(ctx, profile, restored, now)
		if restoreErr := os.Chmod(home, 0o700); restoreErr != nil {
			t.Fatal(restoreErr)
		}
		if err == nil {
			t.Fatal("a swap that cannot rename succeeded")
		}
		untouched(restored)
	})

	t.Run("the restored profile takes the place and the old one is kept", func(t *testing.T) {
		kept, err := SwapProfile(ctx, profile, restored, now)
		if err != nil {
			t.Fatal(err)
		}
		if kept != profile+".before-restore-20260307-120000" || !holds(kept) || holds(profile) {
			t.Fatalf("kept profile = %q", kept)
		}
		if _, err := os.Lstat(restored); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("the restored directory remains: %v", err)
		}
		// Both profiles point at their own files.
		if got := stored(profile); got != workspace {
			t.Fatalf("project path after the swap = %q", got)
		}
		if got := stored(kept); got != workspace {
			t.Fatalf("project path of the kept profile = %q", got)
		}
		if content, err := os.ReadFile(filepath.Join(kept, "workspaces", "demo", "note.txt")); err != nil || string(content) != "kept" {
			t.Fatalf("the kept profile's own folder = %q, %v", content, err)
		}
		next, err := startFixtureNode(ctx, profile)
		if err != nil {
			t.Fatalf("start on the restored profile: %v", err)
		}
		if next.ID() != node.ID() {
			t.Error("the restored profile has another identity")
		}
		// A second restore on the same second keeps the first kept profile.
		again := commandFixture(t, next, protocol.Command{Kind: "restore_backup", Data: protocol.CommandRestoreBackupPayload{
			ID: record.ID, ExpectedRevision: settings.Revision, Target: filepath.Join(base, "again"), Location: protocol.BackupLocationLocal,
		}}).Data.(protocol.OutputBackupRestoredPayload)
		if err := next.Stop(ctx); err != nil {
			t.Fatal(err)
		}
		second, err := SwapProfile(ctx, profile, again.Target, now)
		if err != nil || second != kept+"-2" || !holds(kept) {
			t.Fatalf("second swap kept %q, %v", second, err)
		}
	})
}
