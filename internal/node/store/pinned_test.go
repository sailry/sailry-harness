package store

import (
	"context"
	"database/sql"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
)

func TestPinnedCopy(t *testing.T) {
	t.Parallel()
	ctx := context.Background()
	count := func(t *testing.T, handle *Handle) (rows int) {
		t.Helper()
		if err := handle.Read(ctx, func(connection *sql.Conn) error {
			return connection.QueryRowContext(ctx, "SELECT count(*) FROM fixture").Scan(&rows)
		}); err != nil {
			t.Fatal(err)
		}
		return rows
	}
	insert := func(handle *Handle, from, to int) error {
		return handle.Write(ctx, func(connection *sql.Conn) error {
			for id := from; id < to; id++ {
				if _, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(?,?)", id, "row"); err != nil {
					return err
				}
			}
			return nil
		})
	}
	directory := databaseDirectory(t)
	handle, err := OpenHandle(ctx, filepath.Join(directory, "node.sqlite"))
	if err != nil {
		t.Fatal(err)
	}
	defer handle.Close()
	if err := handle.Write(ctx, func(connection *sql.Conn) error {
		_, err := connection.ExecContext(ctx, "CREATE TABLE fixture(id INTEGER PRIMARY KEY, body TEXT)")
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := insert(handle, 0, 100); err != nil {
		t.Fatal(err)
	}
	exclusive, err := handle.Exclusive(ctx)
	if err != nil {
		t.Fatal(err)
	}
	pinned, err := exclusive.Pin(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if err := exclusive.Close(); err != nil {
		t.Fatal(err)
	}

	t.Run("the database is used while a copy is pending", func(t *testing.T) {
		// Under the earlier closed-file copy these waited for the backup.
		finished := make(chan error, 1)
		go func() { finished <- insert(handle, 100, 150) }()
		select {
		case err := <-finished:
			if err != nil {
				t.Fatal(err)
			}
		case <-time.After(10 * time.Second):
			t.Fatal("a write waited for the pinned snapshot")
		}
		if rows := count(t, handle); rows != 150 {
			t.Fatalf("live rows = %d", rows)
		}
	})

	target := filepath.Join(directory, "copy", "node.sqlite")
	if err := core.PrivateDirectory(filepath.Dir(target)); err != nil {
		t.Fatal(err)
	}
	t.Run("the copy is the pinned state", func(t *testing.T) {
		var rows int
		if err := pinned.Connection().QueryRowContext(ctx, "SELECT count(*) FROM fixture").Scan(&rows); err != nil || rows != 100 {
			t.Fatalf("pinned rows = %d, %v", rows, err)
		}
		if err := pinned.Copy(ctx, target); err != nil {
			t.Fatal(err)
		}
		entries, err := os.ReadDir(filepath.Dir(target))
		if err != nil || len(entries) != 1 {
			t.Fatalf("the copy is not one complete file: %v, %v", entries, err)
		}
		assertPrivateDatabase(t, target)
		// It opens as any Node database does, which requires WAL mode.
		copied, err := OpenHandle(ctx, target)
		if err != nil {
			t.Fatal(err)
		}
		defer copied.Close()
		if rows := count(t, copied); rows != 100 {
			t.Fatalf("copied rows = %d", rows)
		}
	})

	t.Run("a cancelled copy stops", func(t *testing.T) {
		cancelled, cancel := context.WithCancel(ctx)
		cancel()
		if err := pinned.Copy(cancelled, filepath.Join(directory, "cancelled.sqlite")); err == nil {
			t.Fatal("a cancelled copy finished")
		}
	})

	t.Run("closing waits for the pinned reader only", func(t *testing.T) {
		if err := pinned.Close(); err != nil {
			t.Fatal(err)
		}
		if err := pinned.Close(); err != nil {
			t.Fatal(err)
		}
		again, err := handle.Exclusive(ctx)
		if err != nil {
			t.Fatal(err)
		}
		if err := again.Close(); err != nil {
			t.Fatal(err)
		}
	})
}
