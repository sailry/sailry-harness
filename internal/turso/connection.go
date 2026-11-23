package turso

import (
	"context"
	"database/sql/driver"
	"errors"
	"io"
	"sync"
	"time"

	sdk "turso.tech/database/tursogo"
)

type sdkConnection interface {
	driver.Conn
	driver.ConnPrepareContext
	driver.ConnBeginTx
	driver.ExecerContext
	driver.QueryerContext
}

type connection struct {
	mu         sync.Mutex
	base       sdkConnection
	database   uintptr
	pointer    sdk.TursoConnection
	statements map[*statement]struct{}
	rows       map[*rows]struct{}
	closed     bool
}

// Preserve database/sql's distinction between an absent byte slice and a
// non-nil empty BLOB. SDK 0.8.3 otherwise binds both slices as an empty BLOB.
func (connection *connection) CheckNamedValue(argument *driver.NamedValue) error {
	value, err := driver.DefaultParameterConverter.ConvertValue(argument.Value)
	if err != nil {
		return err
	}
	if bytes, ok := value.([]byte); ok && bytes == nil {
		value = nil
	}
	argument.Value = value
	return nil
}

// A watcher remains alive for a cursor, but interrupts only while that cursor
// is executing. Leaving an operation joins any in-flight interrupt before the
// connection is reused. Close joins the watcher before releasing native state.
type operation struct {
	ctx     context.Context
	pointer sdk.TursoConnection
	mu      sync.Mutex
	active  bool
	stop    chan struct{}
	done    chan struct{}
}

func newOperation(ctx context.Context, pointer sdk.TursoConnection) *operation {
	op := &operation{ctx: ctx, pointer: pointer}
	if ctx.Done() != nil {
		op.stop, op.done = make(chan struct{}), make(chan struct{})
		go op.watch()
	}
	return op
}

func (op *operation) watch() {
	defer close(op.done)
	select {
	case <-op.stop:
		return
	case <-op.ctx.Done():
	}
	// The native interrupt ignores an idle connection. Reissue while the
	// cancelled call is active to cover cancellation just before C enters.
	ticker := time.NewTicker(time.Millisecond)
	defer ticker.Stop()
	for {
		op.mu.Lock()
		if op.active {
			bridge.interrupt(op.pointer)
		}
		op.mu.Unlock()
		select {
		case <-op.stop:
			return
		case <-ticker.C:
		}
	}
}

func (op *operation) enter() error {
	if err := op.ctx.Err(); err != nil {
		return err
	}
	op.mu.Lock()
	op.active = true
	op.mu.Unlock()
	return nil
}

func (op *operation) leave() {
	op.mu.Lock()
	op.active = false
	op.mu.Unlock()
}

func (op *operation) close() {
	op.leave()
	if op.stop != nil {
		close(op.stop)
		<-op.done
		op.stop = nil
	}
}

func (op *operation) result(err error) error {
	if err != nil && op.ctx.Err() != nil {
		return errors.Join(op.ctx.Err(), err)
	}
	return err
}

func (connection *connection) run(ctx context.Context, callback func() error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	connection.mu.Lock()
	defer connection.mu.Unlock()
	if connection.closed {
		return sdk.ErrTursoConnClosed
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	op := newOperation(ctx, connection.pointer)
	defer op.close()
	if err := op.enter(); err != nil {
		return err
	}
	err := callback()
	op.leave()
	return op.result(err)
}

func (connection *connection) Close() error {
	connection.mu.Lock()
	defer connection.mu.Unlock()
	if connection.closed {
		return nil
	}
	for cursor := range connection.rows {
		_ = cursor.close()
	}
	for stmt := range connection.statements {
		_ = stmt.close()
	}
	err := connection.base.Close()
	bridge.close(connection.database)
	connection.closed = true
	return err
}

func (connection *connection) Prepare(query string) (driver.Stmt, error) {
	return connection.PrepareContext(context.Background(), query)
}

func (connection *connection) PrepareContext(ctx context.Context, query string) (driver.Stmt, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	connection.mu.Lock()
	defer connection.mu.Unlock()
	if connection.closed {
		return nil, sdk.ErrTursoConnClosed
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	op := newOperation(ctx, connection.pointer)
	defer op.close()
	if err := op.enter(); err != nil {
		return nil, err
	}
	prepared, err := connection.base.PrepareContext(ctx, query)
	op.leave()
	if err != nil {
		return nil, op.result(err)
	}
	stmt := &statement{Stmt: prepared, connection: connection}
	if connection.statements == nil {
		connection.statements = map[*statement]struct{}{}
	}
	connection.statements[stmt] = struct{}{}
	return stmt, nil
}

func (connection *connection) ExecContext(ctx context.Context, query string, arguments []driver.NamedValue) (driver.Result, error) {
	var result driver.Result
	err := connection.run(ctx, func() (err error) {
		result, err = connection.base.ExecContext(ctx, query, arguments)
		return err
	})
	return result, err
}

func (connection *connection) QueryContext(ctx context.Context, query string, arguments []driver.NamedValue) (driver.Rows, error) {
	return connection.query(ctx, func() (driver.Rows, error) {
		return connection.base.QueryContext(ctx, query, arguments)
	})
}

func (connection *connection) query(ctx context.Context, callback func() (driver.Rows, error)) (driver.Rows, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	connection.mu.Lock()
	defer connection.mu.Unlock()
	if connection.closed {
		return nil, sdk.ErrTursoConnClosed
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	op := newOperation(ctx, connection.pointer)
	if err := op.enter(); err != nil {
		op.close()
		return nil, err
	}
	base, err := callback()
	op.leave()
	if err != nil {
		op.close()
		return nil, op.result(err)
	}
	if err := ctx.Err(); err != nil {
		op.close()
		_ = base.Close()
		return nil, err
	}
	cursor := &rows{Rows: base, connection: connection, operation: op}
	connection.rows[cursor] = struct{}{}
	return cursor, nil
}

func (connection *connection) Ping(ctx context.Context) error {
	cursor, err := connection.QueryContext(ctx, "SELECT 1", nil)
	if err != nil {
		return err
	}
	defer cursor.Close()
	values := make([]driver.Value, len(cursor.Columns()))
	for {
		if err := cursor.Next(values); err != nil {
			if err == io.EOF {
				return nil
			}
			return err
		}
	}
}

func (connection *connection) Begin() (driver.Tx, error) {
	return connection.BeginTx(context.Background(), driver.TxOptions{})
}

func (connection *connection) BeginTx(ctx context.Context, options driver.TxOptions) (driver.Tx, error) {
	var tx driver.Tx
	err := connection.run(ctx, func() (err error) {
		tx, err = connection.base.BeginTx(ctx, options)
		return err
	})
	if err != nil {
		return nil, err
	}
	return &transaction{Tx: tx, connection: connection}, nil
}

type transaction struct {
	driver.Tx
	connection *connection
}

func (tx *transaction) Commit() error {
	return tx.connection.run(context.Background(), tx.Tx.Commit)
}

func (tx *transaction) Rollback() error {
	return tx.connection.run(context.Background(), tx.Tx.Rollback)
}

type statement struct {
	driver.Stmt
	connection *connection
	closed     bool
}

func (statement *statement) close() error {
	if statement.closed {
		return nil
	}
	statement.closed = true
	delete(statement.connection.statements, statement)
	return statement.Stmt.Close()
}

func (statement *statement) Close() error {
	statement.connection.mu.Lock()
	defer statement.connection.mu.Unlock()
	return statement.close()
}

func (statement *statement) Exec(arguments []driver.Value) (driver.Result, error) {
	var result driver.Result
	err := statement.connection.run(context.Background(), func() (err error) {
		result, err = statement.Stmt.Exec(arguments)
		return err
	})
	return result, err
}

func (statement *statement) Query(arguments []driver.Value) (driver.Rows, error) {
	return statement.connection.query(context.Background(), func() (driver.Rows, error) {
		return statement.Stmt.Query(arguments)
	})
}

func (statement *statement) ExecContext(ctx context.Context, arguments []driver.NamedValue) (driver.Result, error) {
	var result driver.Result
	err := statement.connection.run(ctx, func() (err error) {
		result, err = statement.Stmt.(driver.StmtExecContext).ExecContext(ctx, arguments)
		return err
	})
	return result, err
}

func (statement *statement) QueryContext(ctx context.Context, arguments []driver.NamedValue) (driver.Rows, error) {
	return statement.connection.query(ctx, func() (driver.Rows, error) {
		return statement.Stmt.(driver.StmtQueryContext).QueryContext(ctx, arguments)
	})
}

type rows struct {
	driver.Rows
	connection *connection
	operation  *operation
	closed     bool
}

func (rows *rows) close() error {
	if rows.closed {
		return nil
	}
	rows.operation.close()
	rows.closed = true
	delete(rows.connection.rows, rows)
	return rows.Rows.Close()
}

func (rows *rows) Close() error {
	rows.connection.mu.Lock()
	defer rows.connection.mu.Unlock()
	return rows.close()
}

func (rows *rows) Next(values []driver.Value) error {
	rows.connection.mu.Lock()
	defer rows.connection.mu.Unlock()
	if rows.closed {
		return io.EOF
	}
	if err := rows.operation.enter(); err != nil {
		_ = rows.close()
		return err
	}
	err := rows.Rows.Next(values)
	rows.operation.leave()
	if err == nil {
		err = rows.operation.ctx.Err()
	}
	if err != nil {
		_ = rows.close()
	}
	return rows.operation.result(err)
}
