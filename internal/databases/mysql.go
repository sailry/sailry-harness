package databases

import (
	"context"
	"database/sql/driver"
	"errors"
	"fmt"
	"io"
	"log"
	"math"
	"net"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/go-sql-driver/mysql"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// mysqlSession is the part of the driver connection one operation uses.
type mysqlSession interface {
	driver.ConnPrepareContext
	driver.ExecerContext
	driver.QueryerContext
}

func mysqlConfig(server protocol.DatabaseServer, password string, dial dialFunc, tunnelled bool) *mysql.Config {
	config := mysql.NewConfig()
	config.User = server.Username
	config.Passwd = password
	config.DBName = server.Database
	config.ClientFoundRows = true
	config.Logger = log.New(io.Discard, "", 0)
	config.DialFunc = dial
	host := server.Host
	if !tunnelled && strings.EqualFold(host, "localhost") {
		// Route to IPv4 loopback while TLS keeps verifying the profile host.
		host = "127.0.0.1"
	}
	config.Net, config.Addr = "tcp", net.JoinHostPort(host, strconv.Itoa(int(server.Port)))
	if server.Socket != nil {
		config.Net, config.Addr = "unix", *server.Socket
	}
	switch server.Tls {
	case protocol.DatabaseTlsPrefer:
		config.TLS = verifiedTLS(server.Host)
		config.AllowFallbackToPlaintext = true
	case protocol.DatabaseTlsRequire:
		config.TLS = verifiedTLS(server.Host)
	}
	return config
}

func runMySQL(ctx context.Context, operation Operation, target endpoint, query Query, dial dialFunc) (protocol.DatabaseResultSet, error) {
	var empty protocol.DatabaseResultSet
	connector, err := mysql.NewConnector(mysqlConfig(target.server, operation.Password.Expose(), dial, target.tunnel))
	if err != nil {
		return empty, connectionError(err, operation.Password)
	}
	connection, err := connector.Connect(ctx)
	if err != nil {
		return empty, connectionError(err, operation.Password)
	}
	defer connection.Close()
	session, ok := connection.(mysqlSession)
	if !ok {
		return empty, fault(protocol.ErrorCodeInternal, "MySQL driver connection lacks context execution")
	}
	return queryMySQL(ctx, session, query, operation.Profile.ReadOnly)
}

func queryMySQL(ctx context.Context, session mysqlSession, query Query, readOnly bool) (protocol.DatabaseResultSet, error) {
	var empty protocol.DatabaseResultSet
	if readOnly {
		if _, err := session.ExecContext(ctx, "SET SESSION TRANSACTION READ ONLY", nil); err != nil {
			return empty, queryError(mysqlMessage(err))
		}
	}
	// Server-side preparation rejects multiple statements before execution.
	statement, err := session.PrepareContext(ctx, query.SQL)
	if err != nil {
		return empty, queryError(mysqlMessage(err))
	}
	_ = statement.Close()
	stream, err := session.QueryContext(ctx, query.SQL, nil)
	if err != nil {
		if ctx.Err() != nil {
			return empty, unknown()
		}
		return empty, mysqlExecutionError(err)
	}
	result, err := mysqlCollect(stream, query.RowLimit)
	if err != nil {
		if ctx.Err() != nil {
			return empty, unknown()
		}
		return empty, err
	}
	if len(result.Columns) == 0 {
		// The driver discards the OK packet of a statement read through the
		// query path. ROW_COUNT() is the server's copy of that packet's count.
		if result.AffectedRows, err = mysqlAffectedRows(ctx, session); err != nil {
			return empty, unknown()
		}
	}
	return result, nil
}

// mysqlCollect keeps the first result set. CALL can return several sets; the
// remainder is drained without changing that result.
func mysqlCollect(stream driver.Rows, limit uint32) (protocol.DatabaseResultSet, error) {
	var empty protocol.DatabaseResultSet
	columns := stream.Columns()
	result, err := newRows(columns, limit)
	if err != nil {
		return empty, err
	}
	values := make([]driver.Value, len(columns))
	for {
		err := stream.Next(values)
		if err == io.EOF {
			break
		}
		if err != nil {
			return empty, mysqlExecutionError(err)
		}
		if !result.retain() {
			continue
		}
		row := make([]protocol.DatabaseValue, len(values))
		for index, value := range values {
			row[index] = mysqlValue(value)
		}
		if err := result.push(row); err != nil {
			return empty, err
		}
	}
	if sets, ok := stream.(driver.RowsNextResultSet); ok {
		for {
			err := sets.NextResultSet()
			if err == io.EOF {
				break
			}
			if err != nil {
				return empty, mysqlExecutionError(err)
			}
		}
	}
	if err := stream.Close(); err != nil {
		return empty, mysqlExecutionError(err)
	}
	return result.result, nil
}

func mysqlAffectedRows(ctx context.Context, session mysqlSession) (uint64, error) {
	stream, err := session.QueryContext(ctx, "SELECT ROW_COUNT()", nil)
	if err != nil {
		return 0, err
	}
	defer stream.Close()
	values := make([]driver.Value, 1)
	if err := stream.Next(values); err != nil {
		return 0, err
	}
	count, _ := values[0].(int64)
	return uint64(max(count, 0)), nil
}

// mysqlValue maps the driver's text-protocol values. Integers stay exact; an
// unsigned value beyond int64 is retained as text.
func mysqlValue(value driver.Value) protocol.DatabaseValue {
	switch value := value.(type) {
	case nil:
		return nullValue()
	case int64:
		return integerValue(value)
	case uint64:
		if value > math.MaxInt64 {
			return textValue(strconv.FormatUint(value, 10))
		}
		return integerValue(int64(value))
	case float32:
		return realValue(mysqlReal(float64(value), 32))
	case float64:
		return realValue(mysqlReal(value, 64))
	case []byte:
		if utf8.Valid(value) {
			return textValue(string(value))
		}
		return blobValue(append([]byte(nil), value...))
	default:
		return textValue(fmt.Sprint(value))
	}
}

// mysqlReal renders the shortest decimal that restores the parsed value. The
// driver parses FLOAT and DOUBLE text itself, so the server's own digits are
// not available.
func mysqlReal(value float64, bits int) string {
	if magnitude := math.Abs(value); magnitude == 0 || magnitude >= 1e-5 && magnitude < 1e15 {
		return strconv.FormatFloat(value, 'f', -1, bits)
	}
	mantissa, exponent, _ := strings.Cut(strconv.FormatFloat(value, 'e', -1, bits), "e")
	number, err := strconv.Atoi(exponent)
	if err != nil {
		return mantissa
	}
	return mantissa + "e" + strconv.Itoa(number)
}

func mysqlMessage(err error) string {
	var database *mysql.MySQLError
	if !errors.As(err, &database) {
		return err.Error()
	}
	if database.SQLState != [5]byte{} {
		return fmt.Sprintf("error returned from database: %d (%s): %s", database.Number, database.SQLState[:], database.Message)
	}
	return fmt.Sprintf("error returned from database: %d: %s", database.Number, database.Message)
}

func mysqlExecutionError(err error) error {
	var database *mysql.MySQLError
	if errors.As(err, &database) {
		return queryError(mysqlMessage(err))
	}
	return unknown()
}
