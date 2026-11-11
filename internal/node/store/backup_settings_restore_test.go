package store

import (
	"context"
	"database/sql"
	"encoding/binary"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// A database that was damaged when it was backed up has a correct checksum
// in its archive. The restore refuses it instead of publishing the profile.
func TestRestoredIntegrity(t *testing.T) {
	t.Parallel()
	ctx := context.Background()
	node := protocol.NodeID{7}
	staging := t.TempDir()
	storage, err := Open(ctx, staging, node)
	if err != nil {
		t.Fatal(err)
	}
	// An index with entries, emptied below: its table still reads, so only
	// the integrity check sees that the two disagree.
	var index string
	var root, size int64
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := db.ExecContext(ctx, "CREATE TABLE restored_fixture(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE)"); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO restored_fixture(name) VALUES('one'),('two'),('three')"); err != nil {
			return err
		}
		if err := db.QueryRowContext(ctx, "SELECT name,rootpage FROM sqlite_schema WHERE type='index' AND tbl_name='restored_fixture'").Scan(&index, &root); err != nil {
			return err
		}
		return db.QueryRowContext(ctx, "PRAGMA page_size").Scan(&size)
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Close(); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(staging, "storage", "node.sqlite")
	target := filepath.Join(t.TempDir(), "target")

	// The fixture table is not part of the schema, so a sound copy is
	// refused for that and not for its pages.
	if _, err := PrepareRestored(ctx, staging, node, staging, target); err == nil || strings.Contains(err.Error(), "corrupt") {
		t.Fatalf("restore of a sound database with a foreign table: %v", err)
	}

	// Fold the write-ahead log, so the main file holds every page.
	handle, err := OpenHandle(ctx, path)
	if err != nil {
		t.Fatal(err)
	}
	closed, err := handle.Pause(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if err := errors.Join(closed.Close(), handle.Close()); err != nil {
		t.Fatal(err)
	}
	// An empty leaf page in place of the index's root.
	page := make([]byte, size)
	page[0] = 0x0a
	binary.BigEndian.PutUint16(page[5:], uint16(size))
	file, err := os.OpenFile(path, os.O_WRONLY, 0)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := file.WriteAt(page, (root-1)*size); err != nil {
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
	_, err = PrepareRestored(ctx, staging, node, staging, target)
	var fault *protocol.Fault
	if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeInvalidRequest || fault.Message != "restored Node database is corrupt" {
		t.Fatalf("restore of a database with a damaged index %s: %v", index, err)
	}
}
