package store

import (
	"context"
	"database/sql"
	"database/sql/driver"
	"errors"
	"strings"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	turso "turso.tech/database/tursogo"
)

const conflictRetries = 8

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
	if err := beginRead(ctx, connection, state.writer); err != nil {
		return err
	}
	err = callback(connection)
	if err == nil {
		_, err = connection.ExecContext(ctx, "COMMIT")
		if err == nil {
			return nil
		}
	}
	return errors.Join(err, rollbackRead(ctx, connection))
}

func beginRead(ctx context.Context, connection *sql.Conn, writer *writer) error {
	for attempt := 0; ; attempt++ {
		if err := ctx.Err(); err != nil {
			return err
		}
		err := pinRead(ctx, connection)
		if err == nil {
			return nil
		}
		if cleanup := rollbackRead(ctx, connection); cleanup != nil {
			return errors.Join(err, cleanup)
		}
		if !transactionConflict(err) || attempt >= conflictRetries {
			return err
		}
		// Turso's blocking MVCC checkpoint runs inside COMMIT and can reject
		// new snapshots until its I/O completes. Wait for the local writer
		// only after the reader has ended its failed admission.
		if done := writer.commitDone.Load(); done != nil {
			select {
			case <-*done:
			case <-ctx.Done():
				return ctx.Err()
			}
		} else if err := waitForConflict(ctx, attempt); err != nil {
			return err
		}
	}
}

func pinRead(ctx context.Context, connection *sql.Conn) error {
	if _, err := connection.ExecContext(ctx, "BEGIN"); err != nil {
		return err
	}
	rows, err := connection.QueryContext(ctx, "SELECT rootpage FROM sqlite_schema LIMIT 1")
	if err != nil {
		return err
	}
	for rows.Next() {
		var rootpage int64
		if err = rows.Scan(&rootpage); err != nil {
			break
		}
	}
	return errors.Join(err, rows.Err(), rows.Close())
}

func transactionConflict(err error) bool {
	if joined, ok := err.(interface{ Unwrap() []error }); ok {
		causes := joined.Unwrap()
		for _, cause := range causes {
			if !transactionConflict(cause) {
				return false
			}
		}
		return len(causes) != 0
	}
	if errors.Is(err, turso.ErrTursoBusy) {
		// The SDK also uses Busy for an unfinished statement on this same
		// connection. Waiting cannot finish that statement.
		return !strings.Contains(err.Error(), "SQL statements in progress")
	}
	// SDK 0.8.3 does not recognize native TURSO_BUSY_SNAPSHOT (6). Match
	// its two exact messages without making other generic errors retryable.
	if errors.Is(err, turso.ErrTursoGeneric) {
		message := strings.TrimPrefix(err.Error(), turso.ErrTursoGeneric.Error()+": ")
		return message == "database snapshot is stale, rollback and retry the transaction" ||
			message == "Commit dependency aborted, rollback and retry the whole transaction"
	}
	return false
}

func waitForConflict(ctx context.Context, attempt int) error {
	delay := time.NewTimer(time.Millisecond << min(attempt, 6))
	defer delay.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-delay.C:
		return nil
	}
}

func rollbackRead(ctx context.Context, connection *sql.Conn) error {
	_, err := connection.ExecContext(context.WithoutCancel(ctx), "ROLLBACK")
	if err != nil {
		_ = connection.Raw(func(any) error { return driver.ErrBadConn })
	}
	return err
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
	if exclusive.writer.failed != nil || exclusive.Connection() == nil {
		return nil, errors.New("database writer connection is unavailable")
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
