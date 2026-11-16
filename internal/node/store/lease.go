package store

import (
	"context"
	"database/sql"
	"database/sql/driver"
	"errors"
	"sync"

	"github.com/sailry/sailry-harness/internal/core"
)

// Read pins the snapshot before invoking its callback exactly once.
func (handle *Handle) Read(ctx context.Context, callback func(*sql.Conn) error) error {
	state, err := handle.acquire(ctx)
	if err != nil {
		return err
	}
	defer handle.leases.Release(1)
	connection, err := state.readDB.Conn(ctx)
	if err != nil {
		return err
	}
	defer connection.Close()
	if _, err := connection.ExecContext(ctx, "BEGIN"); err != nil {
		return err
	}
	rows, err := connection.QueryContext(ctx, "SELECT rootpage FROM sqlite_schema LIMIT 1")
	if err == nil {
		for rows.Next() {
			var rootpage int64
			if err = rows.Scan(&rootpage); err != nil {
				break
			}
		}
		err = errors.Join(err, rows.Err(), rows.Close())
	}
	if err == nil {
		err = callback(connection)
	}
	if err == nil {
		_, err = connection.ExecContext(ctx, "COMMIT")
		if err == nil {
			return nil
		}
	}
	_, cleanup := connection.ExecContext(context.WithoutCancel(ctx), "ROLLBACK")
	if cleanup != nil {
		_ = connection.Raw(func(any) error { return driver.ErrBadConn })
	}
	return errors.Join(err, cleanup)
}

// Exclusive drains existing reads and admitted writes and blocks new leases.
type Exclusive struct {
	handle *Handle
	writer *writer
	mu     sync.Mutex
	// pinned counts the read transactions that outlive this lease.
	pinned   int64
	finished bool
}

func (handle *Handle) Exclusive(ctx context.Context) (*Exclusive, error) {
	if err := handle.leases.Acquire(ctx, allLeases); err != nil {
		return nil, err
	}
	handle.mu.Lock()
	state, stopped := handle.state, handle.stopped
	handle.mu.Unlock()
	if stopped || state == nil {
		handle.leases.Release(allLeases)
		return nil, core.ErrStopped
	}
	return &Exclusive{handle: handle, writer: state.writer}, nil
}

// Connection is the writer's connection, which is idle while every lease is held.
func (exclusive *Exclusive) Connection() *sql.Conn { return exclusive.writer.connection }

func (exclusive *Exclusive) Close() error {
	exclusive.mu.Lock()
	defer exclusive.mu.Unlock()
	if !exclusive.finished {
		exclusive.finished = true
		exclusive.handle.leases.Release(allLeases - exclusive.pinned)
	}
	return nil
}

// Closed retains exclusive maintenance ownership until Reopen or Close.
type Closed struct {
	handle   *Handle
	mu       sync.Mutex
	finished bool
}

func (handle *Handle) Pause(ctx context.Context) (*Closed, error) {
	exclusive, err := handle.Exclusive(ctx)
	if err != nil {
		return nil, err
	}
	defer exclusive.Close()
	return exclusive.Pause(ctx)
}

// Pause checkpoints and closes the database without releasing ownership
// between a read of its metadata and a copy of its files.
func (exclusive *Exclusive) Pause(ctx context.Context) (*Closed, error) {
	exclusive.mu.Lock()
	defer exclusive.mu.Unlock()
	if exclusive.finished {
		return nil, core.ErrStopped
	}
	if exclusive.pinned != 0 {
		return nil, errors.New("database has pinned read transactions")
	}
	rows, err := exclusive.Connection().QueryContext(ctx, "PRAGMA wal_checkpoint(TRUNCATE)")
	if err != nil {
		return nil, err
	}
	for rows.Next() {
	}
	if err := errors.Join(rows.Err(), rows.Close()); err != nil {
		return nil, err
	}
	handle := exclusive.handle
	handle.mu.Lock()
	state := handle.state
	handle.state = nil
	handle.mu.Unlock()
	// Closing this lease later must not release the Closed owner's permits.
	exclusive.finished = true
	closed := &Closed{handle: handle}
	if err := state.close(); err != nil {
		return nil, errors.Join(err, closed.Reopen(context.WithoutCancel(ctx)))
	}
	return closed, nil
}

func (closed *Closed) Path() string { return closed.handle.path }

func (closed *Closed) Reopen(ctx context.Context) error {
	closed.mu.Lock()
	defer closed.mu.Unlock()
	if closed.finished {
		return core.ErrStopped
	}
	state, err := openConnections(ctx, closed.handle.path)
	closed.handle.mu.Lock()
	closed.handle.state = state
	closed.handle.stopped = err != nil
	closed.handle.mu.Unlock()
	closed.finished = true
	closed.handle.leases.Release(allLeases)
	return err
}

func (closed *Closed) Close() error {
	closed.mu.Lock()
	defer closed.mu.Unlock()
	if closed.finished {
		return nil
	}
	closed.handle.mu.Lock()
	closed.handle.stopped = true
	closed.handle.mu.Unlock()
	closed.finished = true
	closed.handle.leases.Release(allLeases)
	return nil
}
