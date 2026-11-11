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
	close  sync.Once
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
	exclusive.close.Do(func() { exclusive.handle.leases.Release(allLeases) })
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
	var busy, log, checkpointed int
	if err := exclusive.Connection().QueryRowContext(ctx, "PRAGMA wal_checkpoint(TRUNCATE)").Scan(&busy, &log, &checkpointed); err != nil {
		_ = exclusive.Close()
		return nil, err
	}
	if busy != 0 {
		_ = exclusive.Close()
		return nil, errors.New("database checkpoint is busy")
	}
	handle.mu.Lock()
	state := handle.state
	handle.state = nil
	handle.mu.Unlock()
	if err := state.close(); err != nil {
		handle.mu.Lock()
		handle.stopped = true
		handle.mu.Unlock()
		_ = exclusive.Close()
		return nil, err
	}
	// The Closed owner takes the existing permit without releasing it in between.
	return &Closed{handle: handle}, nil
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
