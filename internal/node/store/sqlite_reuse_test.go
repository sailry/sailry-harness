package store

import (
	"bytes"
	"context"
	"database/sql"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
	"modernc.org/sqlite"
)

func sqliteProfileFixture(t *testing.T, seed func(context.Context, *sql.Conn, string, protocol.NodeID) error) (string, protocol.NodeID, *sql.DB) {
	t.Helper()
	ctx := t.Context()
	profile := t.TempDir()
	directory := filepath.Join(profile, "storage")
	if err := core.PrivateDirectory(directory); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(directory, "node.sqlite")
	if err := os.WriteFile(path, nil, 0o600); err != nil {
		t.Fatal(err)
	}
	connector, err := sqlite.NewConnector(path)
	if err != nil {
		t.Fatal(err)
	}
	database := sql.OpenDB(connector)
	t.Cleanup(func() { _ = database.Close() })
	database.SetMaxOpenConns(1)
	connection, err := database.Conn(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer connection.Close()
	var mode string
	if err := connection.QueryRowContext(ctx, "PRAGMA journal_mode=WAL").Scan(&mode); err != nil || mode != "wal" {
		t.Fatalf("SQLite fixture journal: %q, %v", mode, err)
	}
	if _, err := connection.ExecContext(ctx, "PRAGMA foreign_keys=1; PRAGMA wal_autocheckpoint=0; BEGIN IMMEDIATE"); err != nil {
		t.Fatal(err)
	}
	expected, err := referenceSchema(ctx, directory)
	if err != nil {
		t.Fatal(err)
	}
	node := protocol.NodeID{31}
	definition := currentSchema{state: schemaFresh, path: path, expected: expected}
	if err := definition.initialize(ctx, connection, node); err != nil {
		t.Fatal(err)
	}
	if seed != nil {
		if err := seed(ctx, connection, profile, node); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := connection.ExecContext(ctx, "COMMIT"); err != nil {
		t.Fatal(err)
	}
	return profile, node, database
}

func TestSQLiteProfileReusesNativeSameFile(t *testing.T) {
	t.Parallel()
	peer := protocol.NodeID{32}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = protocol.NewProviderID(), "fixture"
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
	var session protocol.SessionID
	profile, node, fixture := sqliteProfileFixture(t, func(ctx context.Context, db *sql.Conn, profile string, node protocol.NodeID) error {
		provider := protocol.DefaultConversationProvider()
		provider.ID, provider.Revision, provider.Name, provider.Enabled = config.Provider, 1, "SQLite fixture", true
		provider.Api, provider.Authentication, provider.Endpoint = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, "http://fixture.invalid/v1"
		provider.DefaultModel = config.Model
		model := protocol.DefaultConversationModel()
		model.ID, model.Context, model.Output, model.Tools = config.Model, 32000, 8000, true
		provider.Models = []protocol.ConversationModel{model}
		body, err := encode(providerConfiguration{ID: provider.ID, Provider: &provider, Listed: true, Authorizations: map[protocol.CredentialID]providerAuthorization{}})
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO providers(id,body) VALUES(?1,?2)", provider.ID.String(), body); err != nil {
			return err
		}
		if _, err := SaveDefaults(ctx, db, node, protocol.Defaults{Config: &config}); err != nil {
			return err
		}
		if err := PairLinkPeer(ctx, db, peer, link.DirectionHost); err != nil {
			return err
		}
		output, _, err := createSession(ctx, db, node, profile, protocol.CommandCreateSessionPayload{Config: &config})
		if err != nil {
			return err
		}
		created := output.Data.(protocol.Session)
		session = created.ID
		request := protocol.NewRequest(node, protocol.Command{Kind: "submit_turn", Data: &protocol.CommandSubmitTurnPayload{
			Session: session, ExpectedRevision: created.Revision, Message: protocol.ConversationInput{Text: "SQLite history remains"},
		}})
		body, err = encode(request)
		if err != nil {
			return err
		}
		if _, err := admission.Admit(ctx, db, node, request, body); err != nil {
			return err
		}
		capture, err := admitAgentTurn(ctx, db, node, node, request, created, protocol.ConversationRunKindTask, true)
		if err != nil {
			return err
		}
		now := time.Date(2026, 11, 10, 0, 0, 0, 0, time.UTC)
		invocation, _, err := ClaimNext(ctx, db, now)
		if err != nil {
			return err
		}
		if invocation == nil || invocation.Run.Turn != capture.ID {
			return errors.New("SQLite fixture turn was not claimed")
		}
		record := canonical.NewRecord(capture.ID, "message", ptr(canonical.NewMessage(canonical.User, canonical.Text(0, "SQLite history remains"))), now)
		record.Provenance.Author = "user"
		if _, _, err := AppendCanonical(ctx, db, session, record); err != nil {
			return err
		}
		_, _, err = FinishTurn(ctx, db, capture.ID, protocol.ConversationStatusCompleted, nil, now)
		return err
	})
	if err := fixture.Close(); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(profile, "storage", "node.sqlite")
	before, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	derived := filepath.Join(profile, "storage", "knowledge.sqlite")
	if err := os.WriteFile(derived, []byte("preserved SQLite derived index"), 0o600); err != nil {
		t.Fatal(err)
	}
	var reference protocol.PluginReference
	for round := range 2 {
		storage, err := Open(t.Context(), profile, node)
		if err != nil {
			t.Fatal(err)
		}
		if storage.Node.Path() != path || storage.Knowledge.Path() != filepath.Join(profile, "storage", "knowledge.turso") {
			t.Fatalf("storage selected a replacement: %s, %s", storage.Node.Path(), storage.Knowledge.Path())
		}
		if err := storage.Node.Read(t.Context(), func(db *sql.Conn) error {
			var mode string
			if err := db.QueryRowContext(t.Context(), "PRAGMA journal_mode").Scan(&mode); err != nil || mode != "wal" {
				return fmt.Errorf("existing journal changed: %q, %v", mode, err)
			}
			peers, err := LinkDirections(t.Context(), db)
			if err != nil || len(peers) != 1 || peers[0].ID != peer || peers[0].Direction != link.DirectionHost {
				return fmt.Errorf("pairing changed: %#v, %v", peers, err)
			}
			defaults, err := ReadDefaults(t.Context(), db)
			if err != nil || defaults.Revision != uint64(round+1) || !reflect.DeepEqual(defaults.Config, &config) {
				return fmt.Errorf("defaults changed: %#v, %v", defaults, err)
			}
			history, err := ReadConversation(t.Context(), db, session, nil, 20)
			if err != nil {
				return err
			}
			body, err := encode(history)
			if err == nil && !bytes.Contains(body, []byte("SQLite history remains")) {
				err = errors.New("canonical history was lost")
			}
			return err
		}); err != nil {
			_ = storage.Close()
			t.Fatal(err)
		}
		if round == 0 {
			if err := storage.Node.Write(t.Context(), func(db *sql.Conn) error {
				if _, err := SaveDefaults(t.Context(), db, node, protocol.Defaults{Revision: 1, Config: &config}); err != nil {
					return err
				}
				info, err := InstallPlugin(t.Context(), db, pluginTestInfo("wal-native-search", 'a'), 0, true)
				if err != nil {
					return err
				}
				reference = plugins.PackageReference(info)
				_, err = WritePluginValue(t.Context(), db, reference, "record", []byte(`"retained"`), 0, &protocol.PluginStorageIndex{Fields: [2]string{"native search", ""}})
				return err
			}); err != nil {
				_ = storage.Close()
				t.Fatal(err)
			}
		} else if err := storage.Node.Read(t.Context(), func(db *sql.Conn) error {
			query := protocol.DefaultPluginStorageSearch()
			query.Terms, query.Weights, query.Limit = []string{"native"}, [2]uint16{2, 1}, 10
			page, err := SearchPluginValues(t.Context(), db, reference, query)
			if err == nil && len(page.Entries) != 1 {
				err = fmt.Errorf("native FTS restart returned %d entries", len(page.Entries))
			}
			return err
		}); err != nil {
			_ = storage.Close()
			t.Fatal(err)
		}
		if err := storage.Close(); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := PrepareRestored(t.Context(), profile, node, profile, profile); err != nil {
		t.Fatalf("restore validation did not reuse the existing basename: %v", err)
	}
	after, err := os.Stat(path)
	if err != nil || !os.SameFile(before, after) {
		t.Fatalf("canonical database was replaced: %v", err)
	}
	body, err := os.ReadFile(path)
	if err != nil || len(body) < 20 || body[18] != 2 || body[19] != 2 {
		t.Fatalf("WAL file header changed: %v", err)
	}
	if body, err := os.ReadFile(derived); err != nil || string(body) != "preserved SQLite derived index" {
		t.Fatalf("old derived index changed: %v", err)
	}
	for _, name := range []string{"node.turso", "rust-archive", "backups"} {
		if _, err := os.Lstat(filepath.Join(profile, "storage", name)); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("same-file reuse created %s: %v", name, err)
		}
	}
}

func TestSQLiteVirtualTableRefusedBeforeNativeOpen(t *testing.T) {
	t.Parallel()
	profile, node, database := sqliteProfileFixture(t, nil)
	ctx := t.Context()
	connection, err := database.Conn(ctx)
	if err != nil {
		t.Fatal(err)
	}
	// Keep the SQLite owner alive so this committed virtual table exists only
	// in its WAL. A metadata guard that ignores the journal must not pass.
	if _, err := connection.ExecContext(ctx, "CREATE VIRTUAL TABLE plugin_search_fixture USING fts5(text); INSERT INTO plugin_search_fixture VALUES('preserved')"); err != nil {
		t.Fatal(err)
	}
	connection.Close()
	path := filepath.Join(profile, "storage", "node.sqlite")
	before := map[string][]byte{}
	for _, member := range []string{path, path + "-wal"} {
		before[member], err = os.ReadFile(member)
		if err != nil {
			t.Fatal(err)
		}
	}
	storage, err := Open(ctx, profile, node)
	if storage != nil {
		_ = storage.Close()
	}
	if err == nil || !strings.Contains(err.Error(), "virtual table") {
		t.Fatalf("unsupported SQLite index: %v", err)
	}
	for member, original := range before {
		if body, err := os.ReadFile(member); err != nil || !bytes.Equal(body, original) {
			t.Fatalf("preflight changed %s: %v", member, err)
		}
	}
	for _, name := range []string{"node.turso", "knowledge.turso", "node.sqlite-tshm", "node.db-log", "backups"} {
		if _, err := os.Lstat(filepath.Join(profile, "storage", name)); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("refused index created %s: %v", name, err)
		}
	}
}

func TestNodeDatabasePathPreservesIncompleteStorage(t *testing.T) {
	t.Parallel()
	for _, name := range []string{"node.turso", "node.sqlite", "node.turso-wal", "node.db-log", "node.sqlite3"} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			directory := t.TempDir()
			path := filepath.Join(directory, name)
			if err := os.WriteFile(path, nil, 0o600); err != nil {
				t.Fatal(err)
			}
			if selected, err := nodeDatabasePath(t.Context(), directory); err == nil {
				t.Fatalf("incomplete storage selected %s", selected)
			}
			if info, err := os.Lstat(path); err != nil || info.Size() != 0 {
				t.Fatalf("incomplete storage changed: %v", err)
			}
		})
	}
}
