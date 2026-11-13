package databases

import (
	"context"
	"database/sql/driver"
	"errors"
	"io"
	"net"
	"sync/atomic"
	"testing"

	"github.com/go-sql-driver/mysql"
	"github.com/jackc/pgx/v5/pgproto3"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDatabaseFailureStages(t *testing.T) {
	t.Parallel()
	query := Query{SQL: "DELETE FROM items", RowLimit: 1, TimeoutMS: 30_000}
	for _, engine := range []protocol.DatabaseEngine{protocol.DatabaseEnginePostgres, protocol.DatabaseEngineMysql} {
		t.Run(string(engine)+" connection", func(t *testing.T) {
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			calls := 0
			dial := func(context.Context, string, string) (net.Conn, error) {
				calls++
				cancel()
				return nil, context.Canceled
			}
			target := endpoint{engine: engine, tunnel: true, server: protocol.DatabaseServer{Host: "database.invalid", Port: 1234, Username: "fixture", Tls: protocol.DatabaseTlsDisable}}
			var err error
			if engine == protocol.DatabaseEnginePostgres {
				_, err = runPostgres(ctx, Operation{}, target, query, dial)
			} else {
				_, err = runMySQL(ctx, Operation{}, target, query, dial)
			}
			requireFault(t, err, protocol.ErrorCodeUnavailable)
			if calls != 1 {
				t.Fatalf("connection attempts = %d", calls)
			}
		})
	}
	for _, stage := range []string{"prepare", "execute", "results"} {
		t.Run("mysql "+stage, func(t *testing.T) {
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			session := &phaseMySQL{stage: stage, cancel: cancel}
			_, err := queryMySQL(ctx, session, query, false)
			want, queries := protocol.ErrorCodeOutcomeUnknown, 1
			if stage == "prepare" {
				want, queries = protocol.ErrorCodeInvalidRequest, 0
			}
			requireFault(t, err, want)
			if session.queries != queries {
				t.Fatalf("SQL dispatches = %d, want %d", session.queries, queries)
			}
		})
	}
	for _, stage := range []string{"prepare", "execute"} {
		t.Run("postgres "+stage, func(t *testing.T) {
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			client, remote := net.Pipe()
			t.Cleanup(func() { _ = client.Close(); _ = remote.Close() })
			var queries atomic.Int32
			finished := make(chan error, 1)
			go func() {
				defer remote.Close()
				backend := pgproto3.NewBackend(remote, remote)
				if _, err := backend.ReceiveStartupMessage(); err != nil {
					finished <- err
					return
				}
				backend.Send(&pgproto3.AuthenticationOk{})
				backend.Send(&pgproto3.ReadyForQuery{TxStatus: 'I'})
				if err := backend.Flush(); err != nil {
					finished <- err
					return
				}
				for {
					message, err := backend.Receive()
					if err != nil {
						finished <- err
						return
					}
					switch value := message.(type) {
					case *pgproto3.Sync:
						if stage == "prepare" {
							cancel()
							finished <- nil
							return
						}
						backend.Send(&pgproto3.ParseComplete{})
						backend.Send(&pgproto3.ParameterDescription{})
						backend.Send(&pgproto3.NoData{})
						backend.Send(&pgproto3.ReadyForQuery{TxStatus: 'I'})
						if err := backend.Flush(); err != nil {
							finished <- err
							return
						}
					case *pgproto3.Query:
						queries.Add(1)
						if value.String != query.SQL {
							finished <- errors.New("unexpected SQL dispatched")
							return
						}
						cancel()
						finished <- nil
						return
					}
				}
			}()
			target := endpoint{engine: protocol.DatabaseEnginePostgres, tunnel: true, server: protocol.DatabaseServer{Host: "database.invalid", Port: 5432, Username: "fixture", Tls: protocol.DatabaseTlsDisable}}
			_, err := runPostgres(ctx, Operation{}, target, query, func(context.Context, string, string) (net.Conn, error) { return client, nil })
			want, sent := protocol.ErrorCodeOutcomeUnknown, int32(1)
			if stage == "prepare" {
				want, sent = protocol.ErrorCodeInvalidRequest, 0
			}
			requireFault(t, err, want)
			if err := <-finished; err != nil {
				t.Fatal(err)
			}
			if queries.Load() != sent {
				t.Fatalf("SQL dispatches = %d, want %d", queries.Load(), sent)
			}
		})
	}
}

type phaseMySQL struct {
	stage   string
	cancel  context.CancelFunc
	queries int
}

func (session *phaseMySQL) PrepareContext(context.Context, string) (driver.Stmt, error) {
	if session.stage == "prepare" {
		session.cancel()
		return nil, context.Canceled
	}
	return phaseStatement{}, nil
}

func (*phaseMySQL) ExecContext(context.Context, string, []driver.NamedValue) (driver.Result, error) {
	return driver.RowsAffected(0), nil
}

func (session *phaseMySQL) QueryContext(context.Context, string, []driver.NamedValue) (driver.Rows, error) {
	session.queries++
	if session.stage == "execute" {
		session.cancel()
		return nil, &mysql.MySQLError{Number: 1317, Message: "Query execution was interrupted"}
	}
	return phaseRows{cancel: session.cancel}, nil
}

type phaseStatement struct{}

func (phaseStatement) Close() error                               { return nil }
func (phaseStatement) NumInput() int                              { return 0 }
func (phaseStatement) Exec([]driver.Value) (driver.Result, error) { return nil, driver.ErrSkip }
func (phaseStatement) Query([]driver.Value) (driver.Rows, error)  { return nil, driver.ErrSkip }

type phaseRows struct{ cancel context.CancelFunc }

func (phaseRows) Columns() []string { return []string{"value"} }
func (phaseRows) Close() error      { return nil }
func (rows phaseRows) Next([]driver.Value) error {
	rows.cancel()
	return io.ErrUnexpectedEOF
}
