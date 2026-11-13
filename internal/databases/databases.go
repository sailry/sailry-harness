// Package databases executes operations against external PostgreSQL, MySQL and
// SQLite databases. Execution is adapted from sailry-code 67ae9fa0
// sailry-connections.
package databases

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	maxSQLBytes    = 64 * 1024
	maxRowLimit    = 10_000
	maxTimeoutMS   = 3_600_000
	maxResultBytes = 512 * 1024
)

// Query is one SQL statement with its result and time limits.
type Query struct {
	SQL       string
	RowLimit  uint32
	TimeoutMS uint64
}

// SSH is the saved SSH connection that tunnels a PostgreSQL or MySQL operation.
type SSH struct {
	Profile    protocol.SshProfile
	Credential protocol.SshCredential
}

// Browse selects a catalog read. A nil Database lists databases; otherwise the
// tables of the database already selected in the profile connection are listed.
type Browse struct {
	Database *string
}

// Operation is one database request. Browse reads the catalog, Query runs one
// statement, and neither checks the connection. Storage is the Node profile
// directory that SQLite paths may not enter.
type Operation struct {
	Profile  protocol.DatabaseProfile
	Password protocol.Secret
	SSH      *SSH
	Query    *Query
	Browse   *Browse
	Storage  string
}

// Execute runs the operation until it completes, its query timeout elapses or
// ctx is cancelled. Every returned error is a *protocol.Fault.
func Execute(ctx context.Context, operation Operation) (protocol.DatabaseOutcome, error) {
	if operation.Browse != nil {
		return browse(ctx, operation)
	}
	query := Query{SQL: "SELECT 1", RowLimit: 1, TimeoutMS: 30_000}
	if operation.Query != nil {
		query = *operation.Query
		if strings.TrimSpace(query.SQL) == "" || len(query.SQL) > maxSQLBytes ||
			query.RowLimit < 1 || query.RowLimit > maxRowLimit ||
			query.TimeoutMS < 1 || query.TimeoutMS > maxTimeoutMS {
			return protocol.DatabaseOutcome{}, invalid("invalid database query or limit")
		}
	}
	if ctx.Err() != nil {
		return protocol.DatabaseOutcome{}, fault(protocol.ErrorCodeCancelled, "Database operation cancelled before execution")
	}
	ctx, cancel := context.WithTimeout(ctx, time.Duration(query.TimeoutMS)*time.Millisecond)
	defer cancel()
	result, err := run(ctx, operation, query)
	if err != nil {
		// Drivers know whether SQL was sent. Connection and preparation
		// failures stay definite even when the operation deadline has expired.
		return protocol.DatabaseOutcome{}, err
	}
	if operation.Query == nil {
		return protocol.DatabaseOutcome{Kind: "connected"}, nil
	}
	return protocol.DatabaseOutcome{Kind: "query", Data: result}, nil
}

func run(ctx context.Context, operation Operation, query Query) (protocol.DatabaseResultSet, error) {
	target, err := resolve(operation.Profile.Connection)
	if err != nil {
		return protocol.DatabaseResultSet{}, err
	}
	if target.engine == protocol.DatabaseEngineSqlite && target.path != "" {
		return runSQLite(ctx, operation, target.path, query)
	}
	dial, release, err := openTransport(ctx, operation.SSH, target.tunnel)
	if err != nil {
		return protocol.DatabaseResultSet{}, err
	}
	defer release()
	switch target.engine {
	case protocol.DatabaseEngineMysql:
		return runMySQL(ctx, operation, target, query, dial)
	case protocol.DatabaseEnginePostgres:
		return runPostgres(ctx, operation, target, query, dial)
	default:
		return protocol.DatabaseResultSet{}, invalid("invalid database engine")
	}
}

// endpoint is the driver view of a saved connection.
type endpoint struct {
	engine protocol.DatabaseEngine
	server protocol.DatabaseServer
	path   string
	tunnel bool
}

func resolve(connection protocol.DatabaseConnection) (endpoint, error) {
	switch data := connection.Data.(type) {
	case protocol.DatabaseConnectionSqlitePayload:
		return sqliteEndpoint(data)
	case *protocol.DatabaseConnectionSqlitePayload:
		return sqliteEndpoint(*data)
	case protocol.DatabaseConnectionSocketPayload:
		return socketEndpoint(data), nil
	case *protocol.DatabaseConnectionSocketPayload:
		return socketEndpoint(*data), nil
	case protocol.DatabaseConnectionSshPayload:
		return sshEndpoint(data), nil
	case *protocol.DatabaseConnectionSshPayload:
		return sshEndpoint(*data), nil
	case protocol.DatabaseConnectionNetworkPayload:
		return networkEndpoint(data), nil
	case *protocol.DatabaseConnectionNetworkPayload:
		return networkEndpoint(*data), nil
	default:
		return endpoint{}, invalid("invalid database connection")
	}
}

func sqliteEndpoint(data protocol.DatabaseConnectionSqlitePayload) (endpoint, error) {
	if data.Path == "" {
		return endpoint{}, invalid("Database filename is missing")
	}
	return endpoint{engine: protocol.DatabaseEngineSqlite, path: data.Path}, nil
}

func socketEndpoint(data protocol.DatabaseConnectionSocketPayload) endpoint {
	return endpoint{engine: data.Engine, server: protocol.DatabaseServer{
		Host: "localhost", Port: data.Port, Database: data.Database, Username: data.Username,
		Tls: protocol.DatabaseTlsDisable, Socket: &data.Path,
	}}
}

func sshEndpoint(data protocol.DatabaseConnectionSshPayload) endpoint {
	return endpoint{engine: data.Engine, tunnel: true, server: protocol.DatabaseServer{
		Host: data.Host, Port: data.Port, Database: data.Database, Username: data.Username, Tls: data.Tls,
	}}
}

func networkEndpoint(data protocol.DatabaseConnectionNetworkPayload) endpoint {
	return endpoint{engine: data.Engine, server: protocol.DatabaseServer{
		Host: data.Host, Port: data.Port, Database: data.Database, Username: data.Username, Tls: data.Tls,
	}}
}

// rows retains result rows within the row and encoded-size limits. Drivers keep
// reading after truncation so the response is drained.
type rows struct {
	result protocol.DatabaseResultSet
	limit  int
	bytes  int
}

func newRows(columns []string, limit uint32) (*rows, error) {
	if columns == nil {
		columns = []string{}
	}
	encoded, err := json.Marshal(columns)
	if err != nil {
		return nil, queryError(err)
	}
	if len(encoded) > maxResultBytes {
		return nil, invalid("database column metadata exceeds the result limit")
	}
	return &rows{
		result: protocol.DatabaseResultSet{Columns: columns, Rows: [][]protocol.DatabaseValue{}},
		limit:  int(limit),
		bytes:  len(encoded),
	}, nil
}

// retain reports whether another row can still be kept; a refused row marks the
// result truncated without being converted.
func (set *rows) retain() bool {
	if set.result.Truncated || len(set.result.Rows) >= set.limit {
		set.result.Truncated = true
		return false
	}
	return true
}

func (set *rows) push(row []protocol.DatabaseValue) error {
	encoded, err := json.Marshal(row)
	if err != nil {
		return queryError(err)
	}
	if set.bytes+len(encoded) > maxResultBytes {
		set.result.Truncated = true
		return nil
	}
	set.bytes += len(encoded)
	set.result.Rows = append(set.result.Rows, row)
	return nil
}

func nullValue() protocol.DatabaseValue { return protocol.DatabaseValue{Kind: "null"} }

func integerValue(value int64) protocol.DatabaseValue {
	return protocol.DatabaseValue{Kind: "integer", Data: value}
}

func realValue(value string) protocol.DatabaseValue {
	return protocol.DatabaseValue{Kind: "real", Data: value}
}

func textValue(value string) protocol.DatabaseValue {
	return protocol.DatabaseValue{Kind: "text", Data: value}
}

func blobValue(value []byte) protocol.DatabaseValue {
	return protocol.DatabaseValue{Kind: "blob", Data: protocol.Bytes(value)}
}

func fault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}

func invalid(message string) error {
	return fault(protocol.ErrorCodeInvalidRequest, message)
}

func queryError(detail any) error {
	return invalid(fmt.Sprintf("Database query failed: %v", detail))
}

func unavailable() error {
	return fault(protocol.ErrorCodeUnavailable, "Database connection failed")
}

func unknown() error {
	return fault(protocol.ErrorCodeOutcomeUnknown, "Database completion is uncertain; inspect the database before retrying")
}

func connectionError(err error, password protocol.Secret) error {
	message := fmt.Sprintf("Database connection failed: %v", err)
	if password.Expose() != "" {
		message = strings.ReplaceAll(message, password.Expose(), "[redacted]")
	}
	return fault(protocol.ErrorCodeUnavailable, message)
}
