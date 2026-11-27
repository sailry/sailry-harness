package node

import (
	"bytes"
	"database/sql"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (fixture legacyMigrationFixture) interrupt(t *testing.T, at string) {
	t.Helper()
	interrupted := errors.New("interrupted")
	if err := migrateLegacyProfile(t.Context(), fixture.legacy, fixture.target, func(step string) error {
		if step == at {
			return interrupted
		}
		return nil
	}); !errors.Is(err, interrupted) {
		t.Fatalf("interruption at %s: %v", at, err)
	}
}

func (fixture legacyMigrationFixture) simulateFailedRename(t *testing.T) {
	t.Helper()
	migration := fixture.migration()
	state, err := migration.readState()
	if err != nil {
		t.Fatal(err)
	}
	// The old Windows migration could move the backup out and mark the live
	// source before renaming that source failed. Leave exactly that layout.
	if err := os.Rename(filepath.Join(migration.directory, "preparation", "original"), migration.backup()); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(fixture.legacy, legacyProfileMarker), []byte(state.Attempt), 0o600); err != nil {
		t.Fatal(err)
	}
}

func withLegacyMigrationProfile(t *testing.T, path string, check func(*Profile)) {
	t.Helper()
	opened, err := OpenProfile(t.Context(), path)
	if err != nil {
		t.Fatalf("open profile %q: %v", path, err)
	}
	defer func() {
		if err := opened.Close(); err != nil {
			t.Errorf("close profile %q: %v", path, err)
		}
	}()
	check(opened)
}

func writeLegacyMigrationSettings(t *testing.T, opened *Profile) protocol.TerminalSettings {
	t.Helper()
	input := protocol.DefaultTerminalSettings()
	input.Shell = filepath.Join(opened.Ownership.Path, "bin", "fixture-shell")
	var saved protocol.TerminalSettings
	if err := opened.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		var err error
		saved, _, err = store.SaveTerminalSettings(t.Context(), db, input)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return saved
}

func checkLegacyMigrationSettings(t *testing.T, opened *Profile, want protocol.TerminalSettings) {
	t.Helper()
	var got protocol.TerminalSettings
	if err := opened.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		var err error
		got, err = store.ReadTerminalSettings(t.Context(), db)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if got.Shell != want.Shell || got.Revision != want.Revision {
		t.Fatalf("terminal shell/revision = %q/%d, want %q/%d", got.Shell, got.Revision, want.Shell, want.Revision)
	}
}

func TestMigrateLegacyProfileRefreshesPreparedSource(t *testing.T) {
	t.Parallel()
	for _, tc := range []struct {
		name                      string
		backup, reopen, interrupt bool
	}{
		{name: "prepared", reopen: true},
		{name: "failed rename without writes", backup: true},
		{name: "failed rename with writes", backup: true, reopen: true},
		{name: "interrupted refresh", backup: true, reopen: true, interrupt: true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			fixture := newLegacyMigrationFixture(t)
			fixture.interrupt(t, "prepared")
			if tc.backup {
				fixture.simulateFailedRename(t)
			}
			var want protocol.TerminalSettings
			if tc.reopen {
				withLegacyMigrationProfile(t, fixture.legacy, func(opened *Profile) {
					want = writeLegacyMigrationSettings(t, opened)
					if tc.backup {
						var inUse *core.ProfileInUseError
						err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target)
						if !errors.As(err, &inUse) || inUse.Path != fixture.legacy {
							t.Fatalf("resuming migration while the legacy Node is open: %v", err)
						}
					}
					checkLegacyMigrationSettings(t, opened, want)
				})
			}
			storage := filepath.Join(fixture.legacy, "storage")
			original, err := legacyProfileDatabaseHashes(t.Context(), storage)
			if err != nil {
				t.Fatal(err)
			}
			if tc.interrupt {
				fixture.interrupt(t, "refreshing")
				if exists, err := legacyProfileExists(fixture.target); err != nil || exists {
					t.Fatalf("interrupted refresh published a target: %v", err)
				}
				if err := checkLegacyProfileHashes(t.Context(), storage, original); err != nil {
					t.Fatalf("interrupted refresh changed the live source: %v", err)
				}
			}
			if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err != nil {
				t.Fatal(err)
			}
			if err := checkLegacyProfileHashes(t.Context(), filepath.Join(fixture.migration().backup(), "storage"), original); err != nil {
				t.Fatalf("named backup does not contain the last closed source: %v", err)
			}
			if !tc.reopen {
				fixture.assertMigrated(t)
				return
			}
			// Reopening the old Node sweeps scratch files, but the new row and
			// its relocated path must survive both starts of the target Node.
			want.Shell = filepath.Join(fixture.target, "bin", "fixture-shell")
			check := func(opened *Profile) { checkLegacyMigrationSettings(t, opened, want) }
			fixture.assertMigratedProfile(t, check)
			if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err != nil {
				t.Fatalf("repeat migration on startup: %v", err)
			}
			withLegacyMigrationProfile(t, fixture.target, check)
		})
	}
}

func TestMigrateLegacyProfileRefusesPreparedSourceConflicts(t *testing.T) {
	t.Parallel()
	for _, kind := range []string{"foreign backup", "installing", "changed identity"} {
		t.Run(kind, func(t *testing.T) {
			t.Parallel()
			fixture := newLegacyMigrationFixture(t)
			fixture.interrupt(t, "prepared")
			migration := fixture.migration()
			state, err := migration.readState()
			if err != nil {
				t.Fatal(err)
			}
			statePath := filepath.Join(migration.directory, "state.json")
			files := []string{statePath, filepath.Join(fixture.legacy, "link.key")}
			backupMarker := filepath.Join(migration.backup(), legacyProfileMarker)
			if kind != "changed identity" {
				fixture.simulateFailedRename(t)
				files = append(files, backupMarker, filepath.Join(fixture.legacy, legacyProfileMarker))
			}
			switch kind {
			case "foreign backup":
				// Even a backup with the expected hashes is not owned when the
				// attempt marker belongs to someone else.
				if err := os.WriteFile(backupMarker, []byte("attempt-foreign"), 0o600); err != nil {
					t.Fatal(err)
				}
			case "installing":
				// A source still under the legacy name must not be refreshed
				// once the journal has recorded a partial installation.
				marker := filepath.Join(migration.directory, "installing")
				if err := os.WriteFile(marker, []byte(state.Attempt), 0o600); err != nil {
					t.Fatal(err)
				}
				files = append(files, marker)
			case "changed identity":
				if err := os.Rename(fixture.legacy, filepath.Join(fixture.outside, "previous")); err != nil {
					t.Fatal(err)
				}
			}
			withLegacyMigrationProfile(t, fixture.legacy, func(opened *Profile) {
				if replaced := opened.Identity.ID() != fixture.node; replaced != (kind == "changed identity") {
					t.Fatalf("unexpected source identity %v", opened.Identity.ID())
				}
				writeLegacyMigrationSettings(t, opened)
			})
			before := make(map[string][]byte, len(files))
			for _, path := range files {
				body, err := os.ReadFile(path)
				if err != nil {
					t.Fatal(err)
				}
				before[path] = body
			}
			storage := filepath.Join(fixture.legacy, "storage")
			original, err := legacyProfileDatabaseHashes(t.Context(), storage)
			if err != nil {
				t.Fatal(err)
			}
			if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err == nil {
				t.Fatal("conflicting source was migrated")
			}
			if exists, err := legacyProfileExists(fixture.target); err != nil || exists {
				t.Fatalf("refused migration published a target: %v", err)
			}
			if err := checkLegacyProfileHashes(t.Context(), storage, original); err != nil {
				t.Fatalf("refused migration changed the live source: %v", err)
			}
			if kind != "changed identity" {
				if err := checkLegacyProfileHashes(t.Context(), filepath.Join(migration.backup(), "storage"), state.Original); err != nil {
					t.Fatalf("refused migration changed the named backup: %v", err)
				}
			}
			for path, want := range before {
				if got, err := os.ReadFile(path); err != nil || !bytes.Equal(got, want) {
					t.Fatalf("refused migration changed %q: %v", path, err)
				}
			}
		})
	}
}
