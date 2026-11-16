package store

import (
	"bytes"
	"context"
	"database/sql"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
)

func TestClosedCopy(t *testing.T) {
	t.Parallel()
	handle, ctx := driverFixture(t)
	if err := handle.Write(ctx, func(connection *sql.Conn) error {
		_, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(1,2)")
		return err
	}); err != nil {
		t.Fatal(err)
	}
	exclusive, err := handle.Exclusive(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer exclusive.Close()
	closed, err := exclusive.Pause(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer closed.Close()
	if err := exclusive.Close(); err != nil {
		t.Fatal(err)
	}
	// The old exclusive owner must not release permits now owned by Closed.
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	called := false
	if err := handle.Write(cancelled, func(*sql.Conn) error {
		called = true
		return nil
	}); !errors.Is(err, context.Canceled) || called {
		t.Fatalf("closed database admitted a write: called=%v, error=%v", called, err)
	}
	target := filepath.Join(databaseDirectory(t), "node.turso")
	if err := closed.Copy(ctx, target); err != nil {
		t.Fatal(err)
	}
	for index, source := range core.DatabaseFiles(closed.Path()) {
		original, err := os.ReadFile(source)
		copyPath := core.DatabaseFiles(target)[index]
		if errors.Is(err, os.ErrNotExist) && index > 0 {
			if _, err := os.Lstat(copyPath); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("copy contains an absent source member %s: %v", copyPath, err)
			}
			continue
		}
		if err != nil {
			t.Fatal(err)
		}
		copied, err := os.ReadFile(copyPath)
		if err != nil || !bytes.Equal(copied, original) {
			t.Fatalf("copied database member %s differs: %v", copyPath, err)
		}
		assertPrivateDatabase(t, copyPath)
	}
	if err := closed.Copy(cancelled, filepath.Join(databaseDirectory(t), "cancelled.turso")); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled copy = %v", err)
	}
	if err := closed.Reopen(ctx); err != nil {
		t.Fatal(err)
	}
	if err := closed.Copy(ctx, filepath.Join(databaseDirectory(t), "reopened.turso")); !errors.Is(err, core.ErrStopped) {
		t.Fatalf("copy after reopening = %v", err)
	}
	if err := handle.Write(ctx, func(connection *sql.Conn) error {
		_, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(2,3)")
		return err
	}); err != nil {
		t.Fatal(err)
	}
	// The copied family reopens independently, at the state before later writes.
	copied, err := OpenHandle(ctx, target)
	if err != nil {
		t.Fatal(err)
	}
	defer copied.Close()
	var count int
	if err := copied.Read(ctx, func(connection *sql.Conn) error {
		return connection.QueryRowContext(ctx, "SELECT count(*) FROM fixture").Scan(&count)
	}); err != nil || count != 1 {
		t.Fatalf("copied rows = %d, %v", count, err)
	}
}

func TestCopyDatabaseFamily(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	source := filepath.Join(databaseDirectory(t), "node.turso")
	target := filepath.Join(databaseDirectory(t), "node.turso")
	family := core.DatabaseFiles(source)
	for index, path := range family {
		if err := os.WriteFile(path, []byte{byte(index), 7, 8}, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	if err := copyDatabase(ctx, source, target); err != nil {
		t.Fatal(err)
	}
	for index, path := range core.DatabaseFiles(target) {
		body, err := os.ReadFile(path)
		if err != nil || !bytes.Equal(body, []byte{byte(index), 7, 8}) {
			t.Fatalf("database family member %s = %v, %v", path, body, err)
		}
	}
	if err := copyDatabase(ctx, source, target); !errors.Is(err, os.ErrExist) {
		t.Fatalf("existing copy was overwritten: %v", err)
	}
	if err := os.Remove(family[2]); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(family[2], 0o700); err != nil {
		t.Fatal(err)
	}
	if err := copyDatabase(ctx, source, filepath.Join(databaseDirectory(t), "node.turso")); err == nil {
		t.Fatal("a non-regular database family member was copied")
	}
}
