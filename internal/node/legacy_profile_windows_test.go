package node

import (
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
)

const windowsMigrationLockAlias = "migration-source.lock"

func windowsMigrationLockInfo(t *testing.T, path string) os.FileInfo {
	t.Helper()
	file, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	info, err := file.Stat()
	if err := errors.Join(err, file.Close()); err != nil {
		t.Fatal(err)
	}
	return info
}

func TestWindowsLegacyMigrationKeepsOriginalLockThroughRenames(t *testing.T) {
	t.Parallel()
	fixture := newLegacyMigrationFixture(t)
	migration := fixture.migration()
	alias := filepath.Join(migration.directory, windowsMigrationLockAlias)
	original := windowsMigrationLockInfo(t, filepath.Join(fixture.legacy, "node.lock"))
	if err := migrateLegacyProfile(t.Context(), fixture.legacy, fixture.target, func(phase string) error {
		path := fixture.legacy
		switch phase {
		case "staged", "installed":
			path = filepath.Join(migration.directory, "profile")
		case "published":
			path = fixture.target
		}
		if _, err := os.Lstat(alias); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("%s: journal link not retired before profile changes: %v", phase, err)
		}
		if current := windowsMigrationLockInfo(t, filepath.Join(path, "node.lock")); !os.SameFile(original, current) {
			t.Fatalf("%s: migration replaced node.lock", phase)
		}
		profile, err := core.AcquireProfile(path)
		if err == nil {
			_ = profile.Close()
		}
		var inUse *core.ProfileInUseError
		if !errors.As(err, &inUse) {
			t.Fatalf("%s: original name did not contend with the migration lock: %v", phase, err)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	fixture.assertMigrated(t)
}

func TestWindowsLegacyMigrationRecoversJournalLock(t *testing.T) {
	t.Parallel()
	for _, phase := range []string{"before preparation", "prepared", "staged", "published"} {
		t.Run(phase, func(t *testing.T) {
			t.Parallel()
			fixture := newLegacyMigrationFixture(t)
			migration := fixture.migration()
			path := fixture.legacy
			if phase == "before preparation" {
				journal, err := core.AcquireProfile(migration.directory)
				if err != nil {
					t.Fatal(err)
				}
				if err := journal.Close(); err != nil {
					t.Fatal(err)
				}
			} else {
				interrupted := errors.New("interrupted")
				if err := migrateLegacyProfile(t.Context(), fixture.legacy, fixture.target, func(step string) error {
					if step == phase {
						return interrupted
					}
					return nil
				}); !errors.Is(err, interrupted) {
					t.Fatal(err)
				}
				if phase == "staged" {
					path = filepath.Join(migration.directory, "profile")
				} else if phase == "published" {
					path = fixture.target
					profile, err := core.AcquireProfile(path)
					if err != nil {
						t.Fatal(err)
					}
					blocked := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target)
					if err := profile.Close(); err != nil {
						t.Fatal(err)
					}
					var inUse *core.ProfileInUseError
					if !errors.As(blocked, &inUse) || inUse.Path != path {
						t.Fatalf("unfinished publication ignored the target lock: %v", blocked)
					}
				}
			}
			// Simulate process termination between the atomic link creation and unlink.
			primary := filepath.Join(path, "node.lock")
			alias := filepath.Join(migration.directory, windowsMigrationLockAlias)
			if err := os.Link(primary, alias); err != nil {
				t.Fatal(err)
			}
			profile, err := core.AcquireProfile(path)
			if err == nil {
				_ = profile.Close()
			}
			var invalid *core.InvalidProfileError
			if !errors.As(err, &invalid) || invalid.Path != primary {
				t.Fatalf("normal acquisition accepted a hard-linked node.lock: %v", err)
			}
			if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err != nil {
				t.Fatal(err)
			}
			if _, err := os.Lstat(alias); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("resumed migration kept the journal link: %v", err)
			}
			fixture.assertMigrated(t)
		})
	}
}

func TestWindowsLegacyMigrationRejectsUnexpectedLockLinks(t *testing.T) {
	t.Parallel()
	for _, kind := range []string{"unrelated journal entry", "unrelated hard link", "third link"} {
		t.Run(kind, func(t *testing.T) {
			t.Parallel()
			fixture := newLegacyMigrationFixture(t)
			migration := fixture.migration()
			journal, err := core.AcquireProfile(migration.directory)
			if err != nil {
				t.Fatal(err)
			}
			if err := journal.Close(); err != nil {
				t.Fatal(err)
			}
			primary := filepath.Join(fixture.legacy, "node.lock")
			alias := filepath.Join(migration.directory, windowsMigrationLockAlias)
			outside := filepath.Join(fixture.outside, "unexpected.lock")
			paths := []string{primary}
			if kind == "unrelated journal entry" {
				if err := os.WriteFile(alias, []byte("leave alone"), 0o600); err != nil {
					t.Fatal(err)
				}
				paths = append(paths, alias)
			} else {
				if err := os.Link(primary, outside); err != nil {
					t.Fatal(err)
				}
				paths = append(paths, outside)
				if kind == "third link" {
					if err := os.Link(primary, alias); err != nil {
						t.Fatal(err)
					}
					paths = append(paths, alias)
				}
			}
			before := make(map[string]os.FileInfo)
			for _, path := range paths {
				before[path] = windowsMigrationLockInfo(t, path)
			}
			var invalid *core.InvalidProfileError
			if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); !errors.As(err, &invalid) {
				t.Fatalf("unexpected lock links accepted: %v", err)
			}
			for path, info := range before {
				if !os.SameFile(info, windowsMigrationLockInfo(t, path)) {
					t.Fatalf("refused migration replaced %q", path)
				}
			}
			if _, err := os.Lstat(fixture.target); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("refused migration changed target: %v", err)
			}
			if kind == "unrelated hard link" {
				if _, err := os.Lstat(alias); !errors.Is(err, os.ErrNotExist) {
					t.Fatalf("refused migration created its own extra link: %v", err)
				}
			}
		})
	}
}

func TestWindowsLegacyMigrationLockContentionDoesNotLeaveLink(t *testing.T) {
	t.Parallel()
	fixture := newLegacyMigrationFixture(t)
	profile, err := core.AcquireProfile(fixture.legacy)
	if err != nil {
		t.Fatal(err)
	}
	defer profile.Close()
	var inUse *core.ProfileInUseError
	if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); !errors.As(err, &inUse) || inUse.Path != fixture.legacy {
		t.Fatalf("migration missed existing Node ownership: %v", err)
	}
	if _, err := os.Lstat(filepath.Join(fixture.migration().directory, windowsMigrationLockAlias)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("contending migration left a hard link: %v", err)
	}
}
