package store

import (
	"context"
	"database/sql"
	"database/sql/driver"
	"errors"
	"sync"

	"modernc.org/sqlite"
)

// snapshotPages is one step of the page copy: the copy answers cancellation
// between steps of sixteen mebibytes at the default page size.
const snapshotPages = 4096

// Pinned is a read transaction that began under an exclusive lease and
// outlives it. It keeps one lease, so the database stays open until Close.
type Pinned struct {
	handle     *Handle
	connection *sql.Conn
	close      sync.Once
}

// Pin begins a read transaction on the database exactly as it is under the
// exclusive lease. The transaction keeps its share of the lease when the
// exclusive owner closes, so reads and writes continue while the pinned state
// is read or copied.
func (exclusive *Exclusive) Pin(ctx context.Context) (*Pinned, error) {
	handle := exclusive.handle
	handle.mu.Lock()
	state := handle.state
	handle.mu.Unlock()
	connection, err := state.readDB.Conn(ctx)
	if err != nil {
		return nil, err
	}
	// The first read fixes the snapshot; BEGIN alone does not.
	var objects int64
	if _, err = connection.ExecContext(ctx, "BEGIN"); err == nil {
		err = connection.QueryRowContext(ctx, "SELECT count(*) FROM sqlite_schema").Scan(&objects)
	}
	if err != nil {
		_ = connection.Raw(func(any) error { return driver.ErrBadConn })
		_ = connection.Close()
		return nil, err
	}
	exclusive.pinned++
	return &Pinned{handle: handle, connection: connection}, nil
}

// Connection reads the pinned state.
func (pinned *Pinned) Connection() *sql.Conn { return pinned.connection }

// Copy writes the pinned state as a complete database in a new private file
// with SQLite's online backup. The source is neither closed nor blocked.
func (pinned *Pinned) Copy(ctx context.Context, target string) error {
	if err := createDatabaseFile(target); err != nil {
		return err
	}
	return pinned.connection.Raw(func(connection any) error {
		source, ok := connection.(interface {
			NewBackup(string) (*sqlite.Backup, error)
		})
		if !ok {
			return errors.New("database driver has no online backup")
		}
		copying, err := source.NewBackup(databaseDSN(target, false))
		if err != nil {
			return err
		}
		for more := true; more && err == nil; {
			if err = ctx.Err(); err == nil {
				more, err = copying.Step(snapshotPages)
			}
		}
		return errors.Join(err, copying.Finish())
	})
}

func (pinned *Pinned) Close() error {
	var err error
	pinned.close.Do(func() {
		if _, err = pinned.connection.ExecContext(context.Background(), "ROLLBACK"); err != nil {
			_ = pinned.connection.Raw(func(any) error { return driver.ErrBadConn })
		}
		err = errors.Join(err, pinned.connection.Close())
		pinned.handle.leases.Release(1)
	})
	return err
}
