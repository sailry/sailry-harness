package store

import (
	"context"
	"database/sql"
	"database/sql/driver"
	"errors"
	"sync"

	"github.com/sailry/sailry-harness/internal/core"
)

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
// is read.
func (exclusive *Exclusive) Pin(ctx context.Context) (*Pinned, error) {
	exclusive.mu.Lock()
	defer exclusive.mu.Unlock()
	if exclusive.finished {
		return nil, core.ErrStopped
	}
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
