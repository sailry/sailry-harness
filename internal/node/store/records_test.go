package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"
	"os"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCountersNeverReused(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := context.Background()
	var first, second uint64
	if err := storage.Node.Write(ctx, func(connection *sql.Conn) error {
		var err error
		first, err = NextCounter(ctx, connection, CounterEvents)
		if err != nil {
			return err
		}
		_, err = connection.ExecContext(ctx, "INSERT INTO events(cursor,body) VALUES(?1,?2)", first, []byte(`{"kind":"power_settings_changed","data":{"revision":0}}`))
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Write(ctx, func(connection *sql.Conn) error {
		if _, err := connection.ExecContext(ctx, "DELETE FROM events"); err != nil {
			return err
		}
		var err error
		second, err = NextCounter(ctx, connection, CounterEvents)
		return err
	}); err != nil || first != 1 || second != 2 {
		t.Fatalf("watermarks %d, %d: %v", first, second, err)
	}
	sentinel := errors.New("cancel this transaction")
	if err := storage.Node.Write(ctx, func(connection *sql.Conn) error {
		if _, err := NextCounter(ctx, connection, CounterEvents); err != nil {
			return err
		}
		return sentinel
	}); !errors.Is(err, sentinel) {
		t.Fatal(err)
	}
	if got := schemaInteger(t, storage.Node, "SELECT value FROM counters WHERE namespace='events'"); got != 2 {
		t.Fatalf("rolled-back watermark changed: %d", got)
	}
	schemaExecute(t, storage.Node, "UPDATE counters SET value=?1 WHERE namespace='events'", int64(math.MaxInt64))
	if err := storage.Node.Write(ctx, func(connection *sql.Conn) error {
		_, err := NextCounter(ctx, connection, CounterEvents)
		return err
	}); err == nil {
		t.Fatal("exhausted watermark was reused")
	}
}

func TestBlobOncePerDigest(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := context.Background()
	data := []byte("one shared binary fixture")
	var first, second string
	if err := storage.Node.Write(ctx, func(connection *sql.Conn) error {
		var err error
		first, err = PutBlob(ctx, connection, data)
		if err != nil {
			return err
		}
		second, err = PutBlob(ctx, connection, data)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if first != second || schemaInteger(t, storage.Node, "SELECT count(*) FROM agent_blobs") != 1 {
		t.Fatal("identical binary content was duplicated")
	}
	if err := storage.Node.Read(ctx, func(connection *sql.Conn) error {
		loaded, err := GetBlob(ctx, connection, first)
		if err != nil {
			return err
		}
		if !bytes.Equal(loaded, data) {
			t.Fatal("binary content changed")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Write(ctx, func(connection *sql.Conn) error {
		_, err := PutBlob(ctx, connection, nil)
		return err
	}); err != nil {
		t.Fatalf("empty binary content: %v", err)
	}
}

func TestStoredJSONFixtures(t *testing.T) {
	t.Parallel()
	providerStoreFixtures(t)
	data, err := os.ReadFile("testdata/agent_serde.json")
	if err != nil {
		t.Fatal(err)
	}
	var generated []struct {
		Name string `json:"name"`
		Body string `json:"body"`
	}
	if err = json.Unmarshal(data, &generated); err != nil {
		t.Fatal(err)
	}
	rustBody := map[string]string{}
	for _, fixture := range generated {
		rustBody[fixture.Name] = fixture.Body
	}
	for _, fixture := range []struct {
		name, body string
		factory    func() any
	}{
		{"canonical message", rustBody["canonical message"], func() any { return new(canonical.Record) }},
		{"canonical signed call", rustBody["canonical signed call"], func() any { return new(canonical.Block) }},
		{"canonical response", rustBody["canonical response"], func() any { return new(canonical.Block) }},
		{"canonical runtime", rustBody["canonical runtime"], func() any { return new(canonical.Runtime) }},
		{"canonical compaction", rustBody["canonical compaction"], func() any { return new(canonical.Compaction) }},
		{"canonical input identity", rustBody["canonical input identity"], func() any { return new(canonical.Input) }},
		{"question pending", rustBody["question pending"], func() any { return new(protocol.ConversationQuestion) }},
		{"network", `{"revision":7,"selection":{"kind":"iroh"}}`, func() any { return new(networkStored) }},
		{"custom network", `{"revision":3,"selection":{"kind":"custom","relays":["https://relay.example/"]}}`, func() any { return new(networkStored) }},
		{"retained background", `{"revision":3,"resume":false,"child_concurrency":8}`, func() any { return new(backgroundStored) }},
		{"current background", `{"revision":0,"resume":true}`, func() any { return new(backgroundStored) }},
		{"search credentials", `{"key":"private-fixture-key","basic":null}`, func() any { return new(searchAuth) }},
		{"backup credentials", `{"identity":null,"secret":null,"session_token":null,"passphrase":"fixture-passphrase"}`, func() any { return new(backupAuth) }},
		{"database credential target", `{"connection":{"kind":"sqlite","path":"/fixture.sqlite"},"tunnel":null}`, func() any { return new(databaseCredentialBinding) }},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			t.Parallel()
			value := fixture.factory()
			if err := json.Unmarshal([]byte(fixture.body), value); err != nil {
				t.Fatal(err)
			}
			encoded, err := json.Marshal(value)
			if err != nil || string(encoded) != fixture.body {
				t.Fatalf("stored JSON changed: %s, %v", encoded, err)
			}
		})
	}
	for _, body := range []string{`{"connection":`, `{"connection":{"kind":"sqlite","path":"/fixture.sqlite"},"tunnel":null,"unknown":true}`} {
		t.Run("invalid binding "+body, func(t *testing.T) {
			t.Parallel()
			var binding databaseCredentialBinding
			if err := json.Unmarshal([]byte(body), &binding); err == nil {
				t.Fatal("invalid credential binding accepted")
			}
			_, _, storage := schemaFixture(t)
			profile := protocol.DatabaseProfile{
				ID: protocol.NewDatabaseID(), Revision: 1, Name: "Fixture",
				Connection: protocol.DatabaseConnection{Kind: "sqlite", Data: &protocol.DatabaseConnectionSqlitePayload{Path: "/fixture.sqlite"}},
			}
			encoded, err := json.Marshal(profile)
			if err != nil {
				t.Fatal(err)
			}
			schemaExecute(t, storage.Node, "INSERT INTO database_profiles(id,body,password,password_target) VALUES(?1,?2,?3,?4)", profile.ID.String(), encoded, "", []byte(body))
			if err := storage.Node.Read(context.Background(), func(connection *sql.Conn) error {
				return validateStartup(context.Background(), connection)
			}); err == nil {
				t.Fatal("startup accepted invalid credential binding")
			}
		})
	}
}
