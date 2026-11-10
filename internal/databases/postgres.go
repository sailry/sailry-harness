package databases

import (
	"context"
	"crypto/tls"
	"errors"
	"fmt"
	"io"
	"net"
	"unicode/utf8"

	"github.com/jackc/pgx/v5/pgconn"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// pgconn requires ParseConfig. These explicit settings shadow the PG*
// environment; every connection field is then assigned from the profile.
const postgresDefaults = "host=localhost port=5432 user=postgres password=unused dbname=postgres sslmode=disable " +
	"connect_timeout=0 target_session_attrs=any channel_binding=prefer min_protocol_version=3.0 max_protocol_version=3.0"

func postgresConfig(server protocol.DatabaseServer, password string, readOnly bool, dial dialFunc, tunnelled bool) (*pgconn.Config, error) {
	config, err := pgconn.ParseConfig(postgresDefaults)
	if err != nil {
		return nil, err
	}
	config.Host = server.Host
	if server.Socket != nil {
		config.Host = *server.Socket
	}
	config.Port = server.Port
	config.Database = server.Database
	if config.Database == "" {
		config.Database = "postgres"
	}
	config.User = server.Username
	config.Password = password
	config.TLSConfig = nil
	config.Fallbacks = nil
	config.RequireAuth = ""
	config.RuntimeParams = map[string]string{
		"DateStyle":          "ISO, MDY",
		"client_encoding":    "UTF8",
		"TimeZone":           "UTC",
		"extra_float_digits": "2",
	}
	if readOnly {
		config.RuntimeParams["options"] = "-c default_transaction_read_only=on"
	}
	config.DialFunc = postgresDial(dial, server.Tls, server.Host)
	if tunnelled {
		// The SSH server resolves the database host.
		config.LookupFunc = func(_ context.Context, host string) ([]string, error) { return []string{host}, nil }
	}
	return config, nil
}

// postgresDial negotiates TLS on the dialled connection itself. pgconn's own
// prefer mode retries in plaintext after a certificate failure; the source
// never does, so the connection handed to pgconn is already final.
func postgresDial(dial dialFunc, mode protocol.DatabaseTls, host string) pgconn.DialFunc {
	return func(ctx context.Context, network, address string) (net.Conn, error) {
		connection, err := dial(ctx, network, address)
		if err != nil || network == "unix" || mode == protocol.DatabaseTlsDisable {
			return connection, err
		}
		secured, err := postgresTLS(ctx, connection, mode, host)
		if err != nil {
			_ = connection.Close()
			return nil, err
		}
		return secured, nil
	}
}

func postgresTLS(ctx context.Context, connection net.Conn, mode protocol.DatabaseTls, host string) (net.Conn, error) {
	stop := context.AfterFunc(ctx, func() { _ = connection.Close() })
	defer stop()
	// SSLRequest: length 8, code 80877103.
	if _, err := connection.Write([]byte{0, 0, 0, 8, 4, 210, 22, 47}); err != nil {
		return nil, err
	}
	var reply [1]byte
	if _, err := io.ReadFull(connection, reply[:]); err != nil {
		return nil, err
	}
	switch reply[0] {
	case 'S':
		secured := tls.Client(connection, verifiedTLS(host))
		if err := secured.HandshakeContext(ctx); err != nil {
			return nil, err
		}
		return secured, nil
	case 'N':
		if mode == protocol.DatabaseTlsPrefer {
			return connection, nil
		}
		return nil, errors.New("server does not support TLS")
	default:
		return nil, errors.New("server sent an invalid TLS negotiation response")
	}
}

func runPostgres(ctx context.Context, operation Operation, target endpoint, query Query, dial dialFunc) (protocol.DatabaseResultSet, error) {
	var empty protocol.DatabaseResultSet
	config, err := postgresConfig(target.server, operation.Password.Expose(), operation.Profile.ReadOnly, dial, target.tunnel)
	if err != nil {
		return empty, connectionError(err, operation.Password)
	}
	connection, err := pgconn.ConnectConfig(ctx, config)
	if err != nil {
		return empty, connectionError(err, operation.Password)
	}
	defer connection.Close(ctx)
	// Preparation rejects multiple statements; the simple protocol then keeps
	// PostgreSQL's text representation for arbitrary and extension types.
	description, err := connection.Prepare(ctx, "", query.SQL, nil)
	if err != nil {
		return empty, queryError(postgresMessage(err))
	}
	columns := make([]string, len(description.Fields))
	for index, field := range description.Fields {
		columns[index] = field.Name
	}
	result, err := newRows(columns, query.RowLimit)
	if err != nil {
		return empty, err
	}
	reader := connection.Exec(ctx, query.SQL)
	for reader.NextResult() {
		current := reader.ResultReader()
		for current.NextRow() {
			if !result.retain() {
				continue
			}
			row, err := postgresRow(current.Values())
			if err != nil {
				return empty, err
			}
			if err := result.push(row); err != nil {
				return empty, err
			}
		}
		if tag, err := current.Close(); err == nil && len(columns) == 0 {
			result.result.AffectedRows = uint64(tag.RowsAffected())
		}
	}
	if err := reader.Close(); err != nil {
		var database *pgconn.PgError
		if errors.As(err, &database) {
			return empty, queryError(postgresMessage(err))
		}
		return empty, unknown()
	}
	return result.result, nil
}

func postgresRow(values [][]byte) ([]protocol.DatabaseValue, error) {
	row := make([]protocol.DatabaseValue, len(values))
	for index, value := range values {
		switch {
		case value == nil:
			row[index] = nullValue()
		case !utf8.Valid(value):
			return nil, queryError("PostgreSQL returned text that is not valid UTF-8")
		default:
			row[index] = textValue(string(value))
		}
	}
	return row, nil
}

func postgresMessage(err error) string {
	var database *pgconn.PgError
	if !errors.As(err, &database) {
		return err.Error()
	}
	message := "error returned from database: " + database.Message
	if database.Line > 0 {
		message += fmt.Sprintf(" at line %d", database.Line)
	}
	return message
}
