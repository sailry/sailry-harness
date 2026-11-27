package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type legacyMigrationFixture struct {
	legacy, target, outside string
	node                    protocol.NodeID
}

func newLegacyMigrationFixture(t *testing.T) legacyMigrationFixture {
	t.Helper()
	base, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	fixture := legacyMigrationFixture{legacy: filepath.Join(base, "legacy"), target: filepath.Join(base, "target"), outside: filepath.Join(base, "outside")}
	opened, err := OpenProfile(t.Context(), fixture.legacy)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = opened.Close() })
	fixture.node = opened.Identity.ID()
	for _, directory := range []string{filepath.Join(fixture.legacy, "projects", "inside"), filepath.Join(fixture.legacy, "notes"), fixture.outside} {
		if err := os.MkdirAll(directory, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	if err := opened.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		owner, err := store.LocalUser(t.Context(), db)
		if err != nil {
			return err
		}
		appearance, err := json.Marshal(protocol.DefaultProjectsAppearance())
		if err != nil {
			return err
		}
		for _, item := range []struct{ name, path string }{{"Inside", filepath.Join(fixture.legacy, "projects", "inside")}, {"Outside", fixture.outside}} {
			project, worktree := protocol.NewProjectID(), protocol.NewWorktreeID()
			if _, err := db.ExecContext(t.Context(), "INSERT INTO projects(id,name,path,appearance,owner) VALUES(?1,?2,?3,?4,?5)", project.String(), item.name, item.path, appearance, owner.ID.String()); err != nil {
				return err
			}
			if _, err := db.ExecContext(t.Context(), "INSERT INTO worktrees(id,project,owner,path,main) VALUES(?1,?2,?3,?4,1)", worktree.String(), project.String(), owner.ID.String(), item.path); err != nil {
				return err
			}
		}
		definition := protocol.DefaultKnowledgeDefinition()
		definition.Name = "Notes"
		definition.Source = protocol.KnowledgeSource{Kind: "directory", Data: &protocol.KnowledgeSourceDirectoryPayload{Path: filepath.Join(fixture.legacy, "notes")}}
		request := protocol.NewRequest(fixture.node, protocol.Command{Kind: "create_knowledge_collection", Data: &protocol.CommandCreateKnowledgeCollectionPayload{Definition: definition}})
		_, _, err = store.ExecuteKnowledge(store.WithUser(t.Context(), owner), db, request)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := opened.Close(); err != nil {
		t.Fatal(err)
	}
	for name, body := range map[string]string{
		filepath.Join("tmp", "unswept.txt"):                  "keep scratch content",
		filepath.Join("desktop", "preferences.json"):         "{}",
		filepath.Join("user-data", "nested", "document.txt"): "keep user content",
	} {
		path := filepath.Join(fixture.legacy, name)
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	return fixture
}

func (fixture legacyMigrationFixture) migration() *legacyProfileMigration {
	return &legacyProfileMigration{legacy: fixture.legacy, target: fixture.target, directory: filepath.Join(filepath.Dir(fixture.target), ".target.legacy-profile")}
}

func (fixture legacyMigrationFixture) assertMigrated(t *testing.T) {
	t.Helper()
	if body, err := os.ReadFile(filepath.Join(fixture.target, "tmp", "unswept.txt")); err != nil || string(body) != "keep scratch content" {
		t.Fatalf("migration swept scratch content: %q, %v", body, err)
	}
	fixture.assertMigratedProfile(t, nil)
}

func (fixture legacyMigrationFixture) assertMigratedProfile(t *testing.T, inspect func(*Profile)) {
	t.Helper()
	if exists, err := legacyProfileExists(fixture.legacy); err != nil || exists {
		t.Fatalf("migration retained a shared legacy path or alias: %v", err)
	}
	for _, name := range []string{filepath.Join("desktop", "preferences.json"), filepath.Join("user-data", "nested", "document.txt")} {
		if _, err := os.ReadFile(filepath.Join(fixture.target, name)); err != nil {
			t.Fatalf("migration lost or swept %s: %v", name, err)
		}
	}
	identity, err := link.ReadIdentity(fixture.target)
	if err != nil {
		t.Fatal(err)
	}
	if identity.ID() != fixture.node {
		t.Fatalf("migrated identity = %v", identity.ID())
	}
	migration := fixture.migration()
	state, err := migration.readState()
	if err != nil {
		t.Fatal(err)
	}
	if err := checkLegacyProfileHashes(t.Context(), filepath.Join(migration.backup(), "storage"), state.Original); err != nil {
		t.Fatalf("original closed database family was not retained: %v", err)
	}
	original, err := link.ReadIdentity(migration.backup())
	if err != nil || original.ID() != fixture.node {
		t.Fatalf("original key was not retained: %v", err)
	}
	opened, err := OpenProfile(t.Context(), fixture.target)
	if err != nil {
		t.Fatalf("open migrated Node: %v", err)
	}
	defer opened.Close()
	if opened.Identity.ID() != fixture.node {
		t.Fatal("opening the target replaced its identity")
	}
	if err := opened.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		for _, table := range []string{"projects", "worktrees"} {
			rows, err := db.QueryContext(t.Context(), "SELECT path FROM "+table+" ORDER BY path")
			if err != nil {
				return err
			}
			paths := map[string]bool{}
			for rows.Next() {
				var path string
				if err := rows.Scan(&path); err != nil {
					_ = rows.Close()
					return err
				}
				paths[path] = true
			}
			if err := errors.Join(rows.Err(), rows.Close()); err != nil {
				return err
			}
			if len(paths) != 2 || !paths[filepath.Join(fixture.target, "projects", "inside")] || !paths[fixture.outside] {
				return fmt.Errorf("%s paths = %v", table, paths)
			}
		}
		var body []byte
		if err := db.QueryRowContext(t.Context(), "SELECT body FROM knowledge_collections").Scan(&body); err != nil {
			return err
		}
		var collection store.KnowledgeStored
		if err := json.Unmarshal(body, &collection); err != nil {
			return err
		}
		if got := store.ReadKnowledgeSource(collection.Collection.Definition.Source).Path; got != filepath.Join(fixture.target, "notes") {
			return fmt.Errorf("knowledge source = %q", got)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if inspect != nil {
		inspect(opened)
	}
	// A completed migration is a no-op even while the new Node holds its lock.
	if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err != nil {
		t.Fatalf("repeat migration with target in use: %v", err)
	}
	if exists, err := legacyProfileExists(filepath.Join(migration.directory, "preparation")); err != nil || exists {
		t.Fatalf("completed migration retained duplicate prepared databases: %v", err)
	}
}

func TestMigrateLegacyProfileRecovery(t *testing.T) {
	t.Parallel()
	for _, phase := range []string{"prepared", "staged", "installed", "published"} {
		t.Run(phase, func(t *testing.T) {
			t.Parallel()
			fixture := newLegacyMigrationFixture(t)
			fixture.interrupt(t, phase)
			if phase != "published" {
				if exists, err := legacyProfileExists(fixture.target); err != nil || exists {
					t.Fatalf("an incomplete target was published: %v", err)
				}
			}
			if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err != nil {
				t.Fatalf("resume after %s: %v", phase, err)
			}
			fixture.assertMigrated(t)
		})
	}
}

func TestMigrateLegacyProfilePartialDatabaseInstallation(t *testing.T) {
	t.Parallel()
	fixture := newLegacyMigrationFixture(t)
	interrupted := errors.New("interrupted")
	if err := migrateLegacyProfile(t.Context(), fixture.legacy, fixture.target, func(step string) error {
		if step == "staged" {
			return interrupted
		}
		return nil
	}); !errors.Is(err, interrupted) {
		t.Fatal(err)
	}
	migration := fixture.migration()
	// Simulate termination between replacing one old member and copying the new
	// family. The immutable original and prepared families must suffice to resume.
	state, err := migration.readState()
	if err != nil {
		t.Fatal(err)
	}
	if err := writeLegacyProfileFile(filepath.Join(migration.directory, "installing"), []byte(state.Attempt)); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(migration.directory, "profile", "storage", "node.turso")
	if err := os.Remove(path); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte("partial prepared database"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err != nil {
		t.Fatal(err)
	}
	fixture.assertMigrated(t)
}

func TestMigrateLegacyProfileLocks(t *testing.T) {
	t.Parallel()
	for _, scope := range []string{"source", "journal"} {
		t.Run(scope, func(t *testing.T) {
			t.Parallel()
			fixture := newLegacyMigrationFixture(t)
			path := fixture.legacy
			if scope == "journal" {
				path = fixture.migration().directory
			}
			ownership, err := core.AcquireProfile(path)
			if err != nil {
				t.Fatal(err)
			}
			defer ownership.Close()
			err = MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target)
			var inUse *core.ProfileInUseError
			if !errors.As(err, &inUse) || inUse.Path != path {
				t.Fatalf("migration while %s is locked: %v", scope, err)
			}
			if exists, err := legacyProfileExists(fixture.target); err != nil || exists {
				t.Fatalf("locked migration created target: %v", err)
			}
		})
	}
}

func TestMigrateLegacyProfileNeverReplacesTarget(t *testing.T) {
	t.Parallel()
	for _, kind := range []string{"directory", "file"} {
		t.Run(kind, func(t *testing.T) {
			t.Parallel()
			fixture := newLegacyMigrationFixture(t)
			err := migrateLegacyProfile(t.Context(), fixture.legacy, fixture.target, func(step string) error {
				if step != "installed" {
					return nil
				}
				if kind == "directory" {
					return os.Mkdir(fixture.target, 0o700)
				}
				return os.WriteFile(fixture.target, []byte("unrelated target"), 0o600)
			})
			if err == nil {
				t.Fatal("a concurrent target was replaced")
			}
			before, err := os.Lstat(fixture.target)
			if err != nil {
				t.Fatal(err)
			}
			if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err == nil {
				t.Fatal("the retained unpublished profile was silently ignored")
			}
			after, err := os.Lstat(fixture.target)
			if err != nil || !os.SameFile(before, after) {
				t.Fatalf("recovery changed the unrelated target: %v", err)
			}
			if kind == "file" {
				if body, err := os.ReadFile(fixture.target); err != nil || string(body) != "unrelated target" {
					t.Fatalf("unrelated target = %q, %v", body, err)
				}
			}
			// Only the test removes its own conflict; migration never does.
			if err := os.Remove(fixture.target); err != nil {
				t.Fatal(err)
			}
			if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err != nil {
				t.Fatal(err)
			}
			fixture.assertMigrated(t)
		})
	}
}

func TestMigrateLegacyProfileNoop(t *testing.T) {
	t.Parallel()
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if err := MigrateLegacyProfile(ctx, "", "not absolute"); err != nil {
		t.Fatalf("empty declaration: %v", err)
	}
	base := t.TempDir()
	if err := MigrateLegacyProfile(t.Context(), filepath.Join(base, "missing"), filepath.Join(base, "other", "target")); err != nil {
		t.Fatalf("missing legacy profile: %v", err)
	}
	for _, kind := range []string{"directory", "file", "dangling link"} {
		t.Run(kind, func(t *testing.T) {
			base := t.TempDir()
			legacy, target := filepath.Join(base, "incomplete"), filepath.Join(base, "target")
			if err := os.Mkdir(legacy, 0o700); err != nil {
				t.Fatal(err)
			}
			var err error
			switch kind {
			case "directory":
				err = os.Mkdir(target, 0o755)
			case "file":
				err = os.WriteFile(target, []byte("leave alone"), 0o644)
			case "dangling link":
				err = os.Symlink(filepath.Join(base, "absent"), target)
				if err != nil && runtime.GOOS == "windows" {
					t.Skipf("Windows symlink privilege unavailable: %v", err)
				}
			}
			if err != nil {
				t.Fatal(err)
			}
			before, err := os.Lstat(target)
			if err != nil {
				t.Fatal(err)
			}
			if err := MigrateLegacyProfile(t.Context(), legacy, target); err != nil {
				t.Fatal(err)
			}
			after, err := os.Lstat(target)
			if err != nil || !os.SameFile(before, after) || before.Mode() != after.Mode() || before.ModTime() != after.ModTime() {
				t.Fatalf("existing target changed: %v", err)
			}
			if exists, err := legacyProfileExists(filepath.Join(base, ".target.legacy-profile")); err != nil || exists {
				t.Fatalf("no-op created migration state: %v", err)
			}
			entries, err := os.ReadDir(legacy)
			if err != nil || len(entries) != 0 {
				t.Fatalf("no-op opened incomplete legacy profile: %v, %v", entries, err)
			}
		})
	}
}

func TestMigrateLegacyProfileRejectsIncompleteAndForeignProfiles(t *testing.T) {
	t.Parallel()
	for _, kind := range []string{"missing key", "wrong key", "missing database", "empty database", "corrupt database", "controller", "foreign application", "future generation"} {
		t.Run(kind, func(t *testing.T) {
			t.Parallel()
			fixture := newLegacyMigrationFixture(t)
			key := filepath.Join(fixture.legacy, "link.key")
			database := filepath.Join(fixture.legacy, "storage", "node.turso")
			switch kind {
			case "missing key":
				if err := os.Remove(key); err != nil {
					t.Fatal(err)
				}
			case "wrong key":
				if err := os.WriteFile(key, bytes.Repeat([]byte{9}, 32), 0o600); err != nil {
					t.Fatal(err)
				}
			case "missing database":
				if err := os.Remove(database); err != nil {
					t.Fatal(err)
				}
			case "empty database", "corrupt database":
				body := []byte(nil)
				if kind == "corrupt database" {
					body = []byte("this is not a database")
				}
				if err := os.WriteFile(database, body, 0o600); err != nil {
					t.Fatal(err)
				}
			case "controller":
				if err := os.WriteFile(filepath.Join(fixture.legacy, "peers.json"), []byte("{}"), 0o600); err != nil {
					t.Fatal(err)
				}
			case "foreign application", "future generation":
				handle, err := store.OpenHandle(t.Context(), database)
				if err != nil {
					t.Fatal(err)
				}
				statement := "PRAGMA application_id=7"
				if kind == "future generation" {
					statement = "UPDATE node SET storage_generation=2"
				}
				err = handle.Write(t.Context(), func(db *sql.Conn) error {
					_, err := db.ExecContext(t.Context(), statement)
					return err
				})
				if err := errors.Join(err, handle.Close()); err != nil {
					t.Fatal(err)
				}
			}
			before, beforeErr := os.ReadFile(database)
			keyBefore, keyErr := os.ReadFile(key)
			if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err == nil {
				t.Fatal("an incomplete or foreign profile was migrated")
			}
			after, afterErr := os.ReadFile(database)
			keyAfter, keyAfterErr := os.ReadFile(key)
			if !bytes.Equal(before, after) || errors.Is(beforeErr, os.ErrNotExist) != errors.Is(afterErr, os.ErrNotExist) {
				t.Fatal("refused migration changed the source database")
			}
			if !bytes.Equal(keyBefore, keyAfter) || errors.Is(keyErr, os.ErrNotExist) != errors.Is(keyAfterErr, os.ErrNotExist) {
				t.Fatal("refused migration replaced or created the source identity")
			}
			if exists, err := legacyProfileExists(fixture.target); err != nil || exists {
				t.Fatalf("refused migration published a target: %v", err)
			}
			for _, path := range []string{filepath.Join(fixture.migration().directory, "preparation"), fixture.migration().backup()} {
				if exists, err := legacyProfileExists(path); err != nil || exists {
					t.Fatalf("failed preflight retained a full database copy at %s: %v", path, err)
				}
			}
		})
	}
}

func TestMigrateLegacyProfileDoesNotWalkUserTrees(t *testing.T) {
	t.Parallel()
	fixture := newLegacyMigrationFixture(t)
	opaque := filepath.Join(fixture.legacy, "user-data", "opaque")
	if err := os.Mkdir(opaque, 0o700); err != nil {
		t.Fatal(err)
	}
	if runtime.GOOS != "windows" {
		if err := os.Chmod(opaque, 0); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = os.Chmod(filepath.Join(fixture.target, "user-data", "opaque"), 0o700) })
		linked := filepath.Join(fixture.legacy, "user-data", "absolute-link")
		if err := os.Symlink(filepath.Join("nested", "document.txt"), linked); err != nil {
			t.Fatal(err)
		}
	}
	if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err != nil {
		t.Fatal(err)
	}
	if runtime.GOOS != "windows" {
		body, err := os.ReadFile(filepath.Join(fixture.target, "user-data", "absolute-link"))
		if err != nil || string(body) != "keep user content" {
			t.Fatalf("relative link lost its referent: %q, %v", body, err)
		}
	}
	fixture.assertMigrated(t)
}

func TestMigrateLegacyProfileOldNameIsIndependent(t *testing.T) {
	t.Parallel()
	fixture := newLegacyMigrationFixture(t)
	if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err != nil {
		t.Fatal(err)
	}
	fixture.assertMigrated(t)
	fresh, err := OpenProfile(t.Context(), fixture.legacy)
	if err != nil {
		t.Fatal(err)
	}
	defer fresh.Close()
	if fresh.Identity.ID() == fixture.node {
		t.Fatal("the old product name reopened the migrated identity")
	}
	if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err != nil {
		t.Fatal(err)
	}
}

func TestMigrateLegacyProfilePreservesRebuildablePythonEnvironments(t *testing.T) {
	t.Parallel()
	fixture := newLegacyMigrationFixture(t)
	for _, name := range []string{filepath.Join("environments", "cached", "bin", "tool"), filepath.Join("staging", "partial", "pyvenv.cfg")} {
		path := filepath.Join(fixture.legacy, "python", name)
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte("#!"+filepath.Join(fixture.legacy, "python", "old-runtime")), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"environments", "staging"} {
		if exists, err := legacyProfileExists(filepath.Join(fixture.target, "python", name)); err != nil || exists {
			t.Fatalf("stale Python %s remained executable in the new profile: %v", name, err)
		}
		if info, err := os.Lstat(filepath.Join(fixture.migration().backup(), "python", name)); err != nil || !info.IsDir() {
			t.Fatalf("original Python %s was not preserved: %v", name, err)
		}
	}
	fixture.assertMigrated(t)
}

func TestMigrateLegacyProfileResolvesDeclaredHome(t *testing.T) {
	fixture := newLegacyMigrationFixture(t)
	variable := "HOME"
	if runtime.GOOS == "windows" {
		variable = "USERPROFILE"
	}
	t.Setenv(variable, filepath.Dir(fixture.legacy))
	if err := MigrateLegacyProfile(t.Context(), "~/legacy", fixture.target); err != nil {
		t.Fatal(err)
	}
	fixture.assertMigrated(t)
}

func TestMigrateLegacyProfileRefusesMissingRecoverySource(t *testing.T) {
	t.Parallel()
	fixture := newLegacyMigrationFixture(t)
	interrupted := errors.New("interrupted")
	if err := migrateLegacyProfile(t.Context(), fixture.legacy, fixture.target, func(step string) error {
		if step == "prepared" {
			return interrupted
		}
		return nil
	}); !errors.Is(err, interrupted) {
		t.Fatal(err)
	}
	saved := fixture.legacy + "-preserved"
	if err := os.Rename(fixture.legacy, saved); err != nil {
		t.Fatal(err)
	}
	if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err == nil || !strings.Contains(err.Error(), "are missing") {
		t.Fatalf("missing recovery source was treated as a fresh profile: %v", err)
	}
	if exists, err := legacyProfileExists(fixture.target); err != nil || exists {
		t.Fatalf("recovery created an empty target: %v", err)
	}
	if err := os.Rename(saved, fixture.legacy); err != nil {
		t.Fatal(err)
	}
	if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err != nil {
		t.Fatal(err)
	}
	fixture.assertMigrated(t)
}

func TestMigrateLegacyProfileRefusesMissingCompletedTarget(t *testing.T) {
	t.Parallel()
	fixture := newLegacyMigrationFixture(t)
	if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err != nil {
		t.Fatal(err)
	}
	saved := fixture.target + "-preserved"
	if err := os.Rename(fixture.target, saved); err != nil {
		t.Fatal(err)
	}
	if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err == nil || !strings.Contains(err.Error(), "is missing") {
		t.Fatalf("missing completed target was treated as a fresh profile: %v", err)
	}
	if exists, err := legacyProfileExists(fixture.target); err != nil || exists {
		t.Fatalf("recovery created an empty target: %v", err)
	}
	if err := os.Rename(saved, fixture.target); err != nil {
		t.Fatal(err)
	}
	fixture.assertMigrated(t)
}

func TestMigrateLegacyProfileConfigurationPaths(t *testing.T) {
	t.Parallel()
	fixture := newLegacyMigrationFixture(t)
	localPath := filepath.Join(fixture.legacy, "files", "resource")
	remote := protocol.NodeID{1, 2, 3}
	handle, err := store.OpenHandle(t.Context(), filepath.Join(fixture.legacy, "storage", "node.turso"))
	if err != nil {
		t.Fatal(err)
	}
	err = handle.Write(t.Context(), func(db *sql.Conn) error {
		owner, err := store.LocalUser(t.Context(), db)
		if err != nil {
			return err
		}
		terminal := protocol.DefaultTerminalSettings()
		terminal.Shell = localPath
		body, err := json.Marshal(terminal)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(t.Context(), "INSERT INTO terminal_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body); err != nil {
			return err
		}
		if _, err := db.ExecContext(t.Context(), "INSERT INTO user_personal_settings(user,kind,body) VALUES(?1,'terminal',?2)", owner.ID.String(), body); err != nil {
			return err
		}
		backup := protocol.DefaultBackupSettings()
		backup.Local = &protocol.BackupLocal{Directory: localPath, Enabled: true}
		backup.OnlineEnabled = false
		body, err = json.Marshal(map[string]any{"settings": backup, "auth": map[string]any{}})
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(t.Context(), "INSERT INTO backup_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body); err != nil {
			return err
		}
		for _, node := range []protocol.NodeID{fixture.node, remote} {
			name := "local"
			if node == remote {
				name = "remote"
			}
			profile := protocol.DefaultDatabaseProfile()
			profile.ID, profile.Name, profile.Revision = protocol.NewDatabaseID(), name, 1
			profile.Connection = protocol.DatabaseConnection{Kind: "sqlite", Data: &protocol.DatabaseConnectionSqlitePayload{Path: localPath}}
			profile.Location = &protocol.HostPath{Node: node, Path: localPath}
			body, err := json.Marshal(profile)
			if err != nil {
				return err
			}
			if _, err := db.ExecContext(t.Context(), "INSERT INTO database_profiles(id,user,body,password) VALUES(?1,?2,?3,'')", profile.ID.String(), owner.ID.String(), body); err != nil {
				return err
			}
			ssh := protocol.DefaultSshProfile()
			ssh.ID, ssh.Name, ssh.Revision = protocol.NewSshID(), name, 1
			ssh.Host, ssh.Port, ssh.Username = "fixture.example.invalid", 22, "fixture"
			ssh.Authentication, ssh.AuthenticationNode = protocol.SshAuthenticationKeyPath, &node
			credential := protocol.SshCredential{Kind: "key_path", Data: &protocol.SshCredentialKeyPathPayload{Path: protocol.NewSecret(localPath), Location: &protocol.HostPath{Node: node, Path: localPath}}}
			body, err = json.Marshal(ssh)
			if err != nil {
				return err
			}
			secret, err := json.Marshal(credential)
			if err != nil {
				return err
			}
			if _, err := db.ExecContext(t.Context(), "INSERT INTO ssh_profiles(id,user,body,credential) VALUES(?1,?2,?3,?4)", ssh.ID.String(), owner.ID.String(), body, string(secret)); err != nil {
				return err
			}
		}
		return nil
	})
	if err := errors.Join(err, handle.Close()); err != nil {
		t.Fatal(err)
	}
	if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err != nil {
		t.Fatal(err)
	}
	fixture.assertMigrated(t)
	handle, err = store.OpenHandle(t.Context(), filepath.Join(fixture.target, "storage", "node.turso"))
	if err != nil {
		t.Fatal(err)
	}
	defer handle.Close()
	if err := handle.Read(t.Context(), func(db *sql.Conn) error {
		moved := filepath.Join(fixture.target, "files", "resource")
		for _, query := range []string{
			"SELECT json_extract(body,'$.shell') FROM terminal_settings",
			"SELECT json_extract(body,'$.shell') FROM user_personal_settings WHERE kind='terminal'",
			"SELECT json_extract(body,'$.settings.local.directory') FROM backup_settings",
		} {
			var path string
			if err := db.QueryRowContext(t.Context(), query).Scan(&path); err != nil {
				return err
			}
			if path != moved {
				return fmt.Errorf("setting path did not move: %s", query)
			}
		}
		for _, name := range []string{"local", "remote"} {
			expected := moved
			if name == "remote" {
				expected = localPath
			}
			for _, query := range []string{
				"SELECT json_extract(body,'$.connection.path') FROM database_profiles WHERE json_extract(body,'$.name')=?1",
				"SELECT json_extract(body,'$.location.path') FROM database_profiles WHERE json_extract(body,'$.name')=?1",
				"SELECT json_extract(credential,'$.path') FROM ssh_profiles WHERE json_extract(body,'$.name')=?1",
				"SELECT json_extract(credential,'$.location.path') FROM ssh_profiles WHERE json_extract(body,'$.name')=?1",
			} {
				var path string
				if err := db.QueryRowContext(t.Context(), query, name).Scan(&path); err != nil {
					return err
				}
				if path != expected {
					return fmt.Errorf("%s host path is incorrect: %s", name, query)
				}
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
