package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/turso"
)

func schemaFixture(t *testing.T) (string, protocol.NodeID, *Store) {
	t.Helper()
	profile := t.TempDir()
	var node protocol.NodeID
	node[0] = 1
	storage, err := Open(context.Background(), profile, node)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := storage.Close(); err != nil {
			t.Error(err)
		}
	})
	return profile, node, storage
}

func schemaExecute(t *testing.T, handle *Handle, query string, arguments ...any) {
	t.Helper()
	if err := handle.Write(context.Background(), func(connection *sql.Conn) error {
		_, err := connection.ExecContext(context.Background(), query, arguments...)
		return err
	}); err != nil {
		t.Fatal(err)
	}
}

func schemaInteger(t *testing.T, handle *Handle, query string) int64 {
	t.Helper()
	var value int64
	if err := handle.Read(context.Background(), func(connection *sql.Conn) error {
		return connection.QueryRowContext(context.Background(), query).Scan(&value)
	}); err != nil {
		t.Fatal(err)
	}
	return value
}

func reopenSchemaFixture(t *testing.T, profile string, node protocol.NodeID) *Store {
	t.Helper()
	storage, err := Open(context.Background(), profile, node)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := storage.Close(); err != nil {
			t.Error(err)
		}
	})
	return storage
}

func TestGenerationOneKeeps(t *testing.T) {
	t.Parallel()
	t.Run("persistent identity", func(t *testing.T) {
		directory := t.TempDir()
		first, err := link.OpenIdentity(directory)
		if err != nil {
			t.Fatal(err)
		}
		second, err := link.OpenIdentity(directory)
		if err != nil || first.ID() != second.ID() {
			t.Fatalf("identity changed: %v", err)
		}
		path := filepath.Join(directory, "link.key")
		if err := os.WriteFile(path, []byte("truncated"), 0o600); err != nil {
			t.Fatal(err)
		}
		if _, err := link.OpenIdentity(directory); err == nil {
			t.Fatal("corrupt identity accepted")
		}
		body, err := os.ReadFile(path)
		if err != nil || string(body) != "truncated" {
			t.Fatalf("corrupt identity was replaced: %v", err)
		}
	})
	profile, node, original := schemaFixture(t)
	schemaExecute(t, original.Node, "UPDATE node SET plugin_defaults=1")
	var initialTelemetry []byte
	if err := original.Node.Read(context.Background(), func(connection *sql.Conn) error {
		return connection.QueryRowContext(context.Background(), "SELECT body FROM telemetry_settings WHERE singleton=1").Scan(&initialTelemetry)
	}); err != nil {
		t.Fatal(err)
	}
	if err := original.Close(); err != nil {
		t.Fatal(err)
	}
	current := reopenSchemaFixture(t, profile, node)
	if got := schemaInteger(t, current.Node, "SELECT plugin_defaults FROM node"); got != 1 {
		t.Fatalf("current records were reset: %d", got)
	}
	var currentTelemetry []byte
	if err := current.Node.Read(context.Background(), func(connection *sql.Conn) error {
		return connection.QueryRowContext(context.Background(), "SELECT body FROM telemetry_settings WHERE singleton=1").Scan(&currentTelemetry)
	}); err != nil {
		t.Fatal(err)
	}
	if string(currentTelemetry) != string(initialTelemetry) {
		t.Fatal("current configuration changed on restart")
	}
	backups, err := filepath.Glob(filepath.Join(profile, "storage", "backups", "*"))
	if err != nil || len(backups) != 0 {
		t.Fatalf("current startup created a backup: %v, %v", backups, err)
	}
	t.Run("unused historical capture", func(t *testing.T) {
		profile, node, original := schemaFixture(t)
		ctx := context.Background()
		session, worktree := protocol.NewSessionID(), protocol.NewWorktreeID()
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model = protocol.NewProviderID(), "fixture"
		config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
		body, err := json.Marshal(config)
		if err != nil {
			t.Fatal(err)
		}
		roles, err := json.Marshal(protocol.DefaultRoleSnapshot())
		if err != nil {
			t.Fatal(err)
		}
		if err := original.Node.Write(ctx, func(connection *sql.Conn) error {
			row, err := NextCounter(ctx, connection, CounterSessions)
			if err != nil {
				return err
			}
			if _, err := connection.ExecContext(ctx, "INSERT INTO worktrees(id,path,main) VALUES(?1,'/fixture',1)", worktree.String()); err != nil {
				return err
			}
			if _, err := connection.ExecContext(ctx, "INSERT INTO sessions(rowid,id,worktree,revision) VALUES(?1,?2,?3,1)", row, session.String(), worktree.String()); err != nil {
				return err
			}
			if _, err := connection.ExecContext(ctx, "INSERT INTO session_revisions(session,revision,worktree,config,roles) VALUES(?1,1,?2,?3,?4)", session.String(), worktree.String(), body, roles); err != nil {
				return err
			}
			_, err = connection.ExecContext(ctx, "INSERT INTO session_revisions(session,revision,worktree,config,roles) VALUES(?1,0,?2,?3,?4)", session.String(), worktree.String(), []byte(`{"damaged":true}`), roles)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if err := original.Close(); err != nil {
			t.Fatal(err)
		}
		current := reopenSchemaFixture(t, profile, node)
		if err := current.Node.Read(ctx, func(connection *sql.Conn) error { return validateStored(ctx, connection) }); err == nil {
			t.Fatal("complete validation accepted damaged historical capture")
		}
		if err := current.Close(); err != nil {
			t.Fatal(err)
		}
	})
}

func TestGenerationUnknownFails(t *testing.T) {
	t.Parallel()
	for _, literal := range []string{"NULL", "0", "-1", "2", "'1'", "1.0", "X'01'"} {
		t.Run(literal, func(t *testing.T) {
			t.Parallel()
			profile, node, original := schemaFixture(t)
			alteration, generation := "ALTER TABLE node DROP COLUMN storage_generation; ALTER TABLE node ADD COLUMN storage_generation; UPDATE node SET plugin_defaults=1,storage_generation="+literal, "SELECT quote(storage_generation) FROM node"
			schemaExecute(t, original.Node, alteration)
			schemaExecute(t, original.Knowledge, "UPDATE metadata SET version=7")
			if err := original.Close(); err != nil {
				t.Fatal(err)
			}
			opened, err := Open(context.Background(), profile, node)
			if opened != nil {
				_ = opened.Close()
			}
			if err == nil || !strings.Contains(err.Error(), "node.storage_generation") || !strings.Contains(err.Error(), "existing data was not changed") {
				t.Fatalf("unknown generation error: %v", err)
			}
			raw, err := OpenHandle(context.Background(), filepath.Join(profile, "storage", "node.turso"))
			if err != nil {
				t.Fatal(err)
			}
			defer raw.Close()
			if got := schemaInteger(t, raw, "SELECT plugin_defaults FROM node"); got != 1 {
				t.Fatal("unsupported generation reset existing records")
			}
			var actual string
			if err := raw.Read(context.Background(), func(connection *sql.Conn) error {
				return connection.QueryRowContext(context.Background(), generation).Scan(&actual)
			}); err != nil {
				t.Fatal(err)
			}
			if actual != literal {
				t.Fatalf("generation changed: got %q, want %q", actual, literal)
			}
			if got := readOnlySchemaInteger(t, filepath.Join(profile, "storage", "knowledge.turso"), "SELECT version FROM metadata"); got != 7 {
				t.Fatalf("refused storage changed its derived database: %d", got)
			}
			if backups, err := filepath.Glob(filepath.Join(profile, "storage", "backups", "*")); err != nil || len(backups) != 0 {
				t.Fatalf("refused storage created a backup: %v, %v", backups, err)
			}
		})
	}
	t.Run("orphaned reference", func(t *testing.T) {
		t.Parallel()
		profile, node, original := schemaFixture(t)
		if err := original.Close(); err != nil {
			t.Fatal(err)
		}
		path := filepath.Join(profile, "storage", "node.turso")
		connector, err := turso.Connector(path, false)
		if err != nil {
			t.Fatal(err)
		}
		unchecked := sql.OpenDB(connector)
		unchecked.SetMaxOpenConns(1)
		_, err = unchecked.ExecContext(context.Background(), "PRAGMA foreign_keys=0; INSERT INTO session_attention(session,revision,unread) VALUES('orphan',1,0)")
		if err = errors.Join(err, unchecked.Close()); err != nil {
			t.Fatal(err)
		}
		// A current database is not scanned at each start: foreign keys are
		// enforced on every connection of ours, and one row written behind
		// them must not lock its user out.
		opened, err := Open(context.Background(), profile, node)
		if err != nil {
			t.Fatalf("start with an orphaned reference: %v", err)
		}
		if err := opened.Close(); err != nil {
			t.Fatal(err)
		}
		if got := readOnlySchemaInteger(t, path, "SELECT count(*) FROM session_attention"); got != 1 {
			t.Fatalf("the start changed its records: %d", got)
		}
		// A restore audits what it publishes.
		_, err = PrepareRestored(context.Background(), profile, node, profile, filepath.Join(t.TempDir(), "target"))
		if err == nil || !strings.Contains(err.Error(), "invalid stored reference in session_attention; existing data was not changed") {
			t.Fatalf("orphaned reference error: %v", err)
		}
		if got := readOnlySchemaInteger(t, path, "SELECT count(*) FROM session_attention"); got != 1 {
			t.Fatalf("refused storage changed its records: %d", got)
		}
	})
	t.Run("malformed native storage is untouched", func(t *testing.T) {
		t.Parallel()
		profile := t.TempDir()
		storage := filepath.Join(profile, "storage")
		if err := core.PrivateDirectory(storage); err != nil {
			t.Fatal(err)
		}
		path := filepath.Join(storage, "node.turso")
		body := []byte("malformed native storage")
		if err := os.WriteFile(path, body, 0o600); err != nil {
			t.Fatal(err)
		}
		preview := filepath.Join(storage, "node.sqlite")
		if err := os.WriteFile(preview, []byte("preview database"), 0o600); err != nil {
			t.Fatal(err)
		}
		opened, err := Open(context.Background(), profile, protocol.NodeID{})
		if opened != nil {
			_ = opened.Close()
		}
		if err == nil {
			t.Fatal("malformed native database was accepted")
		}
		got, err := os.ReadFile(path)
		if err != nil || string(got) != string(body) {
			t.Fatalf("existing database changed: %q, %v", got, err)
		}
		if body, err := os.ReadFile(preview); err != nil || string(body) != "preview database" {
			t.Fatalf("preview database changed: %v", err)
		}
		for _, name := range []string{"knowledge.turso", "rust-archive", "backups"} {
			if _, err := os.Stat(filepath.Join(storage, name)); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("refused native storage created %s: %v", name, err)
			}
		}
	})
}

func TestStartupAddsMissingObjects(t *testing.T) {
	t.Parallel()
	for _, item := range []struct {
		name, alteration, query string
		want                    int64
	}{
		{"table", "DROP TABLE hook_settings", "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name='hook_settings'", 1},
		{"index", "DROP INDEX session_association_identity", "SELECT count(*) FROM sqlite_schema WHERE type='index' AND name='session_association_identity'", 1},
		{"legacy generation", "ALTER TABLE node DROP COLUMN storage_generation", "SELECT storage_generation FROM node WHERE singleton=1", 1},
		{"column", "ALTER TABLE sessions DROP COLUMN history_revision", "SELECT count(*) FROM pragma_table_info('sessions') WHERE name='history_revision'", 1},
		{"current empty roles", "DELETE FROM roles", "SELECT count(*) FROM roles", 0},
	} {
		t.Run(item.name, func(t *testing.T) {
			t.Parallel()
			profile, node, original := schemaFixture(t)
			schemaExecute(t, original.Node, item.alteration)
			if err := original.Close(); err != nil {
				t.Fatal(err)
			}
			current := reopenSchemaFixture(t, profile, node)
			if got := schemaInteger(t, current.Node, item.query); got != item.want {
				t.Fatalf("startup result: got %d, want %d", got, item.want)
			}
		})
	}
	t.Run("owned definitions", func(t *testing.T) {
		for _, item := range []struct{ actual, wanted string }{
			{"create table records(id integer primary key, /* value */ value text default 'Kept')", "CREATE TABLE records(id INTEGER PRIMARY KEY,value TEXT DEFAULT 'Kept')"},
			{"CREATE TABLE records(id INTEGER PRIMARY KEY,parent INTEGER,FOREIGN KEY(parent) REFERENCES parents(id))", "CREATE TABLE records(id INTEGER PRIMARY KEY,parent INTEGER REFERENCES parents(id))"},
			{`CREATE TABLE "agent_state" (owner TEXT NOT NULL, "key" TEXT NOT NULL, body BLOB NOT NULL, PRIMARY KEY(owner,"key"))`, `CREATE TABLE agent_state(owner TEXT NOT NULL,key TEXT NOT NULL,body BLOB NOT NULL,PRIMARY KEY(owner,key))`},
		} {
			actual, err := parseTable(item.actual)
			if err != nil {
				t.Fatal(err)
			}
			wanted, err := parseTable(item.wanted)
			if err != nil {
				t.Fatal(err)
			}
			if !sameTableShape(actual, wanted) {
				t.Fatal("equivalent controlled table clauses changed shape")
			}
			for name, declaration := range wanted.columns {
				if normalizeSQL(actual.columns[name]) != normalizeSQL(declaration) {
					t.Fatalf("equivalent column %q changed", name)
				}
			}
		}
		for _, item := range []struct{ actual, wanted string }{
			{`"key" TEXT`, `key TEXT NOT NULL`},
			{`"key" TEXT DEFAULT 'kept'`, `key TEXT DEFAULT 'Kept'`},
		} {
			actual, err := parseTable("CREATE TABLE records(" + item.actual + ")")
			if err != nil {
				t.Fatal(err)
			}
			wanted, err := parseTable("CREATE TABLE records(" + item.wanted + ")")
			if err != nil {
				t.Fatal(err)
			}
			if schemaColumn("records", "key", actual.columns["key"]) == schemaColumn("records", "key", wanted.columns["key"]) {
				t.Fatal("identifier quoting hid a changed column constraint or literal")
			}
		}
	})
}

func TestStartupPreservesLegacyCredentialRequests(t *testing.T) {
	t.Parallel()
	profile, node, storage := schemaFixture(t)
	request := protocol.NewRequestID()
	body := []byte(`{"command":{"kind":"put_credential","value":"fixture-secret"}}`)
	schemaExecute(t, storage.Node, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'completed')", node[:], request.String(), body)
	if err := storage.Close(); err != nil {
		t.Fatal(err)
	}
	opened, err := Open(t.Context(), profile, node)
	if opened != nil {
		_ = opened.Close()
	}
	if err == nil || !strings.Contains(err.Error(), "cannot be securely rewritten") {
		t.Fatalf("legacy credential request: %v", err)
	}
	handle, err := OpenHandle(t.Context(), filepath.Join(profile, "storage", "node.turso"))
	if err != nil {
		t.Fatal(err)
	}
	defer handle.Close()
	var retained []byte
	if err := handle.Read(t.Context(), func(db *sql.Conn) error {
		return db.QueryRowContext(t.Context(), "SELECT body FROM requests WHERE caller=?1 AND id=?2", node[:], request.String()).Scan(&retained)
	}); err != nil || string(retained) != string(body) {
		t.Fatalf("legacy request changed: %v", err)
	}
	if backups, err := filepath.Glob(filepath.Join(profile, "storage", "backups", "*")); err != nil || len(backups) != 0 {
		t.Fatalf("refused request created backups: %v, %v", backups, err)
	}
}

func TestUpdateBackupBeforeApply(t *testing.T) {
	t.Parallel()
	for _, invalid := range []bool{false, true} {
		t.Run(fmt.Sprintf("invalid=%t", invalid), func(t *testing.T) {
			t.Parallel()
			profile, node, original := schemaFixture(t)
			schemaExecute(t, original.Node, "DROP INDEX session_association_identity")
			if invalid {
				schemaExecute(t, original.Node, "UPDATE telemetry_settings SET body=X'7B' WHERE singleton=1")
			}
			if err := original.Close(); err != nil {
				t.Fatal(err)
			}
			current, err := Open(context.Background(), profile, node)
			if current != nil {
				defer current.Close()
			}
			if (err != nil) != invalid {
				t.Fatalf("update error: %v", err)
			}
			backups, globErr := filepath.Glob(filepath.Join(profile, "storage", "backups", "node-before-upgrade-*", "node.turso"))
			if globErr != nil {
				t.Fatal(globErr)
			}
			if invalid {
				if len(backups) != 0 {
					t.Fatalf("failed rehearsal published a backup: %v", backups)
				}
				if got := readOnlySchemaInteger(t, filepath.Join(profile, "storage", "node.turso"), "SELECT count(*) FROM sqlite_schema WHERE name='session_association_identity'"); got != 0 {
					t.Fatal("failed rehearsal applied the live update")
				}
				return
			}
			if len(backups) != 1 {
				t.Fatalf("private backups: %v", backups)
			}
			if err := core.PrivateDirectory(filepath.Dir(backups[0])); err != nil {
				t.Fatalf("backup directory is not private: %v", err)
			}
			assertPrivateDatabase(t, backups[0])
			if got := readOnlySchemaInteger(t, backups[0], "SELECT count(*) FROM sqlite_schema WHERE name='session_association_identity'"); got != 0 {
				t.Fatal("backup was captured after the update")
			}
			if got := schemaInteger(t, current.Node, "SELECT count(*) FROM sqlite_schema WHERE name='session_association_identity'"); got != 1 {
				t.Fatal("validated update was not applied")
			}
		})
	}
	t.Run("metadata scope", func(t *testing.T) {
		for _, item := range []struct {
			body     string
			conflict bool
		}{
			{`{"kind":"message","content":[{"type":"text","text":"goose.executable"},{"type":"tool_request","arguments":{"goose.executable":false},"tool_meta":{"goose.executable":true}}]}`, false},
			{`{"kind":"compaction","summary":{"content":[{"type":"tool_request","tool_meta":{"goose.external_dispatch":true}}]}}`, false},
			{`{"kind":"operation","operation":"tool_meta","patch":{"goose.executable":false,"sailry.executable":false}}`, false},
			{`{"kind":"operation","operation":"tool_meta","patch":{"goose.executable":true,"sailry.executable":false}}`, true},
		} {
			var body any
			if err := json.Unmarshal([]byte(item.body), &body); err != nil {
				t.Fatal(err)
			}
			changed, err := upgradeRecordMetadata(body)
			if (err != nil) != item.conflict {
				t.Fatalf("metadata upgrade: %v", err)
			}
			if !item.conflict {
				if !changed {
					t.Fatal("declared metadata was not updated")
				}
				if again, err := upgradeRecordMetadata(body); err != nil || again {
					t.Fatalf("metadata update did not converge: %t, %v", again, err)
				}
			}
		}
	})
}

func readOnlySchemaInteger(t *testing.T, path, query string) int64 {
	t.Helper()
	connector, err := turso.Connector(path, true)
	if err != nil {
		t.Fatal(err)
	}
	database := sql.OpenDB(connector)
	defer database.Close()
	var value int64
	if err := database.QueryRowContext(context.Background(), query).Scan(&value); err != nil {
		t.Fatal(err)
	}
	return value
}
