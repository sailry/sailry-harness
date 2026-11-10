package databases

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/tls"
	"crypto/x509"
	"crypto/x509/pkix"
	"errors"
	"io"
	"math/big"
	"net"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func sqliteOperation(path string, readOnly bool, query *Query) Operation {
	return Operation{
		Profile: protocol.DatabaseProfile{
			Connection: protocol.DatabaseConnection{Kind: "sqlite", Data: protocol.DatabaseConnectionSqlitePayload{Path: path}},
			ReadOnly:   readOnly,
		},
		Query: query,
	}
}

func sqliteQuery(t *testing.T, path, sql string, limit uint32) protocol.DatabaseResultSet {
	t.Helper()
	outcome, err := Execute(context.Background(), sqliteOperation(path, false, &Query{SQL: sql, RowLimit: limit, TimeoutMS: 30_000}))
	if err != nil {
		t.Fatalf("%s: %v", sql, err)
	}
	result, ok := outcome.Data.(protocol.DatabaseResultSet)
	if outcome.Kind != "query" || !ok {
		t.Fatalf("%s: unexpected outcome %#v", sql, outcome)
	}
	return result
}

func requireFault(t *testing.T, err error, code protocol.ErrorCode) *protocol.Fault {
	t.Helper()
	var fault *protocol.Fault
	if !errors.As(err, &fault) {
		t.Fatalf("expected %s fault, got %v", code, err)
	}
	if fault.Code != code {
		t.Fatalf("expected %s fault, got %s: %s", code, fault.Code, fault.Message)
	}
	return fault
}

func TestQueryPaging(t *testing.T) {
	path := filepath.Join(t.TempDir(), "query.sqlite")
	if result := sqliteQuery(t, path, "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL)", 10); len(result.Columns) != 0 || len(result.Rows) != 0 {
		t.Fatalf("unexpected schema result %#v", result)
	}
	if result := sqliteQuery(t, path, "INSERT INTO items (name) VALUES ('a'), ('b;c'), ('d')", 10); result.AffectedRows != 3 {
		t.Fatalf("affected rows = %d, want 3", result.AffectedRows)
	}

	t.Run("row limit truncates", func(t *testing.T) {
		result := sqliteQuery(t, path, "SELECT id, name FROM items ORDER BY id", 2)
		want := [][]protocol.DatabaseValue{
			{integerValue(1), textValue("a")},
			{integerValue(2), textValue("b;c")},
		}
		if !result.Truncated || !reflect.DeepEqual(result.Rows, want) || !reflect.DeepEqual(result.Columns, []string{"id", "name"}) {
			t.Fatalf("unexpected result %#v", result)
		}
	})
	t.Run("exact limit is complete", func(t *testing.T) {
		result := sqliteQuery(t, path, "SELECT id FROM items", 3)
		if result.Truncated || len(result.Rows) != 3 || result.AffectedRows != 0 {
			t.Fatalf("unexpected result %#v", result)
		}
	})
	t.Run("typed values", func(t *testing.T) {
		result := sqliteQuery(t, path, "SELECT 9223372036854775807, 1.25, CAST(X'ff' AS TEXT), X'00ff', NULL", 1)
		want := []protocol.DatabaseValue{
			integerValue(9223372036854775807), realValue("1.25"), textValue("\uFFFD"), blobValue([]byte{0, 255}), nullValue(),
		}
		if len(result.Rows) != 1 || !reflect.DeepEqual(result.Rows[0], want) {
			t.Fatalf("unexpected values %#v", result.Rows)
		}
	})
	t.Run("empty result keeps columns", func(t *testing.T) {
		result := sqliteQuery(t, path, "SELECT name FROM items WHERE id < 0", 5)
		if !reflect.DeepEqual(result.Columns, []string{"name"}) || result.Rows == nil || len(result.Rows) != 0 || result.Truncated {
			t.Fatalf("unexpected result %#v", result)
		}
	})
	t.Run("separator inside a literal and trailing comment", func(t *testing.T) {
		result := sqliteQuery(t, path, "SELECT count(*) FROM items WHERE name = 'b;c'; -- done\n", 1)
		if len(result.Rows) != 1 || !reflect.DeepEqual(result.Rows[0][0], integerValue(1)) {
			t.Fatalf("unexpected result %#v", result)
		}
	})
	t.Run("byte budget truncates", func(t *testing.T) {
		result := sqliteQuery(t, path, "SELECT hex(zeroblob(150000)) FROM items", 10)
		if !result.Truncated || len(result.Rows) != 1 {
			t.Fatalf("rows = %d truncated = %t", len(result.Rows), result.Truncated)
		}
	})
	t.Run("update reports changes", func(t *testing.T) {
		if result := sqliteQuery(t, path, "UPDATE items SET name = name WHERE id <= 2", 1); result.AffectedRows != 2 {
			t.Fatalf("affected rows = %d, want 2", result.AffectedRows)
		}
	})

	rejected := map[string]string{
		"multiple statements": "DELETE FROM items; DELETE FROM items",
		"read then write":     "SELECT 1; DELETE FROM items",
		"invalid SQL":         "not SQL",
		"comment only":        "-- nothing",
		"embedded NUL":        "SELECT 1\x00; DELETE FROM items",
	}
	for name, sql := range rejected {
		t.Run(name, func(t *testing.T) {
			_, err := Execute(context.Background(), sqliteOperation(path, false, &Query{SQL: sql, RowLimit: 1, TimeoutMS: 30_000}))
			fault := requireFault(t, err, protocol.ErrorCodeInvalidRequest)
			if !strings.HasPrefix(fault.Message, "Database query failed: ") {
				t.Fatalf("unexpected message %q", fault.Message)
			}
			if result := sqliteQuery(t, path, "SELECT count(*) FROM items", 1); !reflect.DeepEqual(result.Rows[0][0], integerValue(3)) {
				t.Fatalf("rejected SQL changed the table: %#v", result.Rows)
			}
		})
	}

	t.Run("cancelled before start", func(t *testing.T) {
		ctx, cancel := context.WithCancel(context.Background())
		cancel()
		_, err := Execute(ctx, sqliteOperation(path, false, &Query{SQL: "DELETE FROM items", RowLimit: 1, TimeoutMS: 30_000}))
		requireFault(t, err, protocol.ErrorCodeOutcomeUnknown)
		if result := sqliteQuery(t, path, "SELECT count(*) FROM items", 1); !reflect.DeepEqual(result.Rows[0][0], integerValue(3)) {
			t.Fatalf("cancelled statement ran: %#v", result.Rows)
		}
	})
	t.Run("timeout interrupts", func(t *testing.T) {
		endless := "WITH RECURSIVE counter(value) AS (SELECT 1 UNION ALL SELECT value + 1 FROM counter) SELECT count(*) FROM counter"
		_, err := Execute(context.Background(), sqliteOperation(path, false, &Query{SQL: endless, RowLimit: 1, TimeoutMS: 50}))
		requireFault(t, err, protocol.ErrorCodeOutcomeUnknown)
		if result := sqliteQuery(t, path, "SELECT count(*) FROM items", 1); !reflect.DeepEqual(result.Rows[0][0], integerValue(3)) {
			t.Fatalf("unexpected rows after timeout: %#v", result.Rows)
		}
	})
	t.Run("check connects", func(t *testing.T) {
		outcome, err := Execute(context.Background(), sqliteOperation(path, true, nil))
		if err != nil || outcome.Kind != "connected" || outcome.Data != nil {
			t.Fatalf("outcome %#v error %v", outcome, err)
		}
	})
	t.Run("catalog databases", func(t *testing.T) {
		operation := sqliteOperation(path, false, nil)
		operation.Browse = &Browse{}
		outcome, err := Execute(context.Background(), operation)
		if err != nil {
			t.Fatal(err)
		}
		want := protocol.DatabaseOutcome{Kind: "catalog", Data: protocol.DatabaseCatalog{Kind: "databases", Data: []string{"main"}}}
		if !reflect.DeepEqual(outcome, want) {
			t.Fatalf("unexpected catalog %#v", outcome)
		}
	})
	t.Run("catalog tables", func(t *testing.T) {
		sqliteQuery(t, path, "CREATE VIEW item_names AS SELECT name FROM items", 1)
		database := "main"
		operation := sqliteOperation(path, false, nil)
		operation.Browse = &Browse{Database: &database}
		outcome, err := Execute(context.Background(), operation)
		if err != nil {
			t.Fatal(err)
		}
		want := protocol.DatabaseOutcome{Kind: "catalog", Data: protocol.DatabaseCatalog{Kind: "tables", Data: protocol.DatabaseCatalogTablesPayload{
			Database: "main",
			Tables:   []protocol.DatabaseTable{{Schema: "main", Name: "item_names"}, {Schema: "main", Name: "items"}},
		}}}
		if !reflect.DeepEqual(outcome, want) {
			t.Fatalf("unexpected catalog %#v", outcome)
		}
	})
}

// recordingDial hands out one end of an in-memory pipe and records the target.
type recordingDial struct {
	network, address string
	client           net.Conn
}

func (record *recordingDial) dial(_ context.Context, network, address string) (net.Conn, error) {
	record.network, record.address = network, address
	return record.client, nil
}

// queuedWrites makes writes on a synchronous pipe non-blocking.
type queuedWrites struct {
	net.Conn
	pending chan []byte
}

func (queued *queuedWrites) Write(data []byte) (int, error) {
	queued.pending <- append([]byte(nil), data...)
	return len(data), nil
}

func selfSignedCertificate(t *testing.T, host string) tls.Certificate {
	t.Helper()
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	template := &x509.Certificate{
		SerialNumber: big.NewInt(1),
		Subject:      pkix.Name{CommonName: host},
		DNSNames:     []string{host},
		NotBefore:    time.Unix(0, 0),
		NotAfter:     time.Date(2999, 1, 1, 0, 0, 0, 0, time.UTC),
		KeyUsage:     x509.KeyUsageDigitalSignature,
		ExtKeyUsage:  []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth},
	}
	der, err := x509.CreateCertificate(rand.Reader, template, template, &key.PublicKey, key)
	if err != nil {
		t.Fatal(err)
	}
	return tls.Certificate{Certificate: [][]byte{der}, PrivateKey: key}
}

func TestTunnelDialer(t *testing.T) {
	server := func(mode protocol.DatabaseTls) protocol.DatabaseServer {
		return protocol.DatabaseServer{Host: "db.internal", Port: 5432, Database: "app", Username: "sailry", Tls: mode}
	}
	// negotiate dials through a tunnelled PostgreSQL configuration against a
	// fake server that answers the SSLRequest with reply.
	negotiate := func(t *testing.T, mode protocol.DatabaseTls, reply byte) (net.Conn, net.Conn, error) {
		t.Helper()
		client, remote := net.Pipe()
		t.Cleanup(func() { _ = client.Close(); _ = remote.Close() })
		record := &recordingDial{client: client}
		config, err := postgresConfig(server(mode), "secret", false, record.dial, true)
		if err != nil {
			t.Fatal(err)
		}
		if config.TLSConfig != nil || len(config.Fallbacks) != 0 {
			t.Fatal("pgconn must not negotiate or retry TLS itself")
		}
		hosts, err := config.LookupFunc(context.Background(), "db.internal")
		if err != nil || !reflect.DeepEqual(hosts, []string{"db.internal"}) {
			t.Fatalf("tunnelled lookup resolved locally: %v %v", hosts, err)
		}
		certificate := selfSignedCertificate(t, "db.internal")
		done := make(chan struct{})
		go func() {
			defer close(done)
			request := make([]byte, 8)
			if _, err := io.ReadFull(remote, request); err != nil {
				return
			}
			if _, err := remote.Write([]byte{reply}); err != nil || reply != 'S' {
				return
			}
			// net.Pipe is synchronous; queue the server's writes so a client
			// record sent in the middle of the server flight cannot deadlock.
			queued := &queuedWrites{Conn: remote, pending: make(chan []byte, 64)}
			drained := make(chan struct{})
			go func() {
				defer close(drained)
				for data := range queued.pending {
					_, _ = remote.Write(data)
				}
			}()
			secured := tls.Server(queued, &tls.Config{Certificates: []tls.Certificate{certificate}})
			_ = secured.Handshake()
			close(queued.pending)
			<-drained
		}()
		connection, err := config.DialFunc(context.Background(), "tcp", "db.internal:5432")
		if err != nil {
			_ = remote.Close()
		}
		<-done
		if record.network != "tcp" || record.address != "db.internal:5432" {
			t.Fatalf("dialled %s %s", record.network, record.address)
		}
		return connection, client, err
	}

	t.Run("postgres require rejects a server without TLS", func(t *testing.T) {
		if _, _, err := negotiate(t, protocol.DatabaseTlsRequire, 'N'); err == nil || !strings.Contains(err.Error(), "does not support TLS") {
			t.Fatalf("unexpected error %v", err)
		}
	})
	t.Run("postgres prefer keeps plaintext when TLS is declined", func(t *testing.T) {
		connection, client, err := negotiate(t, protocol.DatabaseTlsPrefer, 'N')
		if err != nil || connection != client {
			t.Fatalf("connection %v error %v", connection, err)
		}
	})
	for _, mode := range []protocol.DatabaseTls{protocol.DatabaseTlsRequire, protocol.DatabaseTlsPrefer} {
		t.Run("postgres "+string(mode)+" verifies the certificate through the tunnel", func(t *testing.T) {
			_, _, err := negotiate(t, mode, 'S')
			var verification *tls.CertificateVerificationError
			if !errors.As(err, &verification) {
				t.Fatalf("expected a certificate verification error, got %v", err)
			}
		})
	}
	t.Run("postgres disable dials without negotiation", func(t *testing.T) {
		client, remote := net.Pipe()
		defer client.Close()
		defer remote.Close()
		record := &recordingDial{client: client}
		config, err := postgresConfig(server(protocol.DatabaseTlsDisable), "secret", true, record.dial, false)
		if err != nil {
			t.Fatal(err)
		}
		connection, err := config.DialFunc(context.Background(), "tcp", "db.internal:5432")
		if err != nil || connection != client {
			t.Fatalf("connection %v error %v", connection, err)
		}
		if config.RuntimeParams["options"] != "-c default_transaction_read_only=on" || config.Database != "app" || config.Password != "secret" {
			t.Fatalf("unexpected configuration %#v", config.RuntimeParams)
		}
	})

	socket := "/run/mysqld/mysqld.sock"
	mysqlCases := []struct {
		name       string
		server     protocol.DatabaseServer
		tunnelled  bool
		network    string
		address    string
		serverName string
		fallback   bool
	}{
		{"mysql require through tunnel", protocol.DatabaseServer{Host: "db.internal", Port: 3306, Tls: protocol.DatabaseTlsRequire}, true, "tcp", "db.internal:3306", "db.internal", false},
		{"mysql prefer through tunnel", protocol.DatabaseServer{Host: "db.internal", Port: 3306, Tls: protocol.DatabaseTlsPrefer}, true, "tcp", "db.internal:3306", "db.internal", true},
		{"mysql disable", protocol.DatabaseServer{Host: "db.internal", Port: 3307, Tls: protocol.DatabaseTlsDisable}, false, "tcp", "db.internal:3307", "", false},
		{"mysql direct localhost", protocol.DatabaseServer{Host: "localhost", Port: 3306, Tls: protocol.DatabaseTlsRequire}, false, "tcp", "127.0.0.1:3306", "localhost", false},
		{"mysql tunnelled localhost", protocol.DatabaseServer{Host: "localhost", Port: 3306, Tls: protocol.DatabaseTlsDisable}, true, "tcp", "localhost:3306", "", false},
		{"mysql socket", protocol.DatabaseServer{Host: "localhost", Port: 3306, Tls: protocol.DatabaseTlsDisable, Socket: &socket}, false, "unix", socket, "", false},
	}
	for _, test := range mysqlCases {
		t.Run(test.name, func(t *testing.T) {
			record := &recordingDial{}
			config := mysqlConfig(test.server, "secret", record.dial, test.tunnelled)
			if config.Net != test.network || config.Addr != test.address || config.DialFunc == nil || !config.ClientFoundRows {
				t.Fatalf("unexpected target %s %s", config.Net, config.Addr)
			}
			if config.AllowFallbackToPlaintext != test.fallback {
				t.Fatalf("plaintext fallback = %t", config.AllowFallbackToPlaintext)
			}
			if test.serverName == "" {
				if config.TLS != nil {
					t.Fatal("unexpected TLS configuration")
				}
				return
			}
			if config.TLS == nil || config.TLS.ServerName != test.serverName || config.TLS.InsecureSkipVerify || config.TLS.RootCAs != nil {
				t.Fatalf("TLS does not verify %s against system roots: %#v", test.serverName, config.TLS)
			}
		})
	}

	t.Run("tunnel without an SSH connection is unavailable", func(t *testing.T) {
		_, err := Execute(context.Background(), Operation{Profile: protocol.DatabaseProfile{Connection: protocol.DatabaseConnection{
			Kind: "ssh",
			Data: &protocol.DatabaseConnectionSshPayload{Engine: protocol.DatabaseEnginePostgres, Host: "db.internal", Port: 5432, Tls: protocol.DatabaseTlsRequire},
		}}})
		requireFault(t, err, protocol.ErrorCodeUnavailable)
	})
	t.Run("sqlite cannot use a server connection", func(t *testing.T) {
		_, err := Execute(context.Background(), Operation{Profile: protocol.DatabaseProfile{Connection: protocol.DatabaseConnection{
			Kind: "network",
			Data: protocol.DatabaseConnectionNetworkPayload{Engine: protocol.DatabaseEngineSqlite, Host: "db.internal", Port: 1},
		}}})
		requireFault(t, err, protocol.ErrorCodeInvalidRequest)
	})
}

func TestDatabaseProfileValidation(t *testing.T) {
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	execute := func(operation Operation) error {
		_, err := Execute(context.Background(), operation)
		return err
	}
	query := func(sql string) *Query { return &Query{SQL: sql, RowLimit: 1, TimeoutMS: 30_000} }
	absent := func(t *testing.T, path string) {
		t.Helper()
		if _, err := os.Lstat(path); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("%s exists: %v", path, err)
		}
	}

	limits := map[string]Query{
		"blank SQL":         {SQL: " \n", RowLimit: 1, TimeoutMS: 1},
		"oversized SQL":     {SQL: "SELECT '" + strings.Repeat("x", maxSQLBytes) + "'", RowLimit: 1, TimeoutMS: 1},
		"zero row limit":    {SQL: "SELECT 1", RowLimit: 0, TimeoutMS: 1},
		"row limit too big": {SQL: "SELECT 1", RowLimit: maxRowLimit + 1, TimeoutMS: 1},
		"zero timeout":      {SQL: "SELECT 1", RowLimit: 1, TimeoutMS: 0},
		"timeout too long":  {SQL: "SELECT 1", RowLimit: 1, TimeoutMS: maxTimeoutMS + 1},
	}
	for name, limit := range limits {
		t.Run(name, func(t *testing.T) {
			path := filepath.Join(root, "limits.sqlite")
			fault := requireFault(t, execute(sqliteOperation(path, false, &limit)), protocol.ErrorCodeInvalidRequest)
			if fault.Message != "invalid database query or limit" {
				t.Fatalf("unexpected message %q", fault.Message)
			}
			absent(t, path)
		})
	}
	t.Run("maximum limits are accepted", func(t *testing.T) {
		path := filepath.Join(root, "maximum.sqlite")
		if err := execute(sqliteOperation(path, false, nil)); err != nil {
			t.Fatal(err)
		}
		sql := "SELECT 1 --" + strings.Repeat("x", maxSQLBytes-len("SELECT 1 --"))
		if err := execute(sqliteOperation(path, false, &Query{SQL: sql, RowLimit: maxRowLimit, TimeoutMS: maxTimeoutMS})); err != nil {
			t.Fatal(err)
		}
	})

	missing := []struct {
		name     string
		readOnly bool
		query    *Query
	}{
		{"missing database is not created by a read", false, query("SELECT 1")},
		{"missing database is not created by PRAGMA", false, query("PRAGMA journal_mode = WAL")},
		{"missing database is not created by EXPLAIN", false, query("EXPLAIN CREATE TABLE items (id INTEGER)")},
		{"missing database is not created by a read-only check", true, nil},
		{"missing database is not created by a read-only write", true, query("CREATE TABLE items (id INTEGER)")},
	}
	for _, test := range missing {
		t.Run(test.name, func(t *testing.T) {
			path := filepath.Join(root, "absent", "missing.sqlite")
			fault := requireFault(t, execute(sqliteOperation(path, test.readOnly, test.query)), protocol.ErrorCodeNotFound)
			if fault.Message != "Database file does not exist" {
				t.Fatalf("unexpected message %q", fault.Message)
			}
			absent(t, filepath.Join(root, "absent"))
		})
	}
	t.Run("missing database browse does not create it", func(t *testing.T) {
		path := filepath.Join(root, "browse.sqlite")
		operation := sqliteOperation(path, false, nil)
		operation.Browse = &Browse{}
		requireFault(t, execute(operation), protocol.ErrorCodeNotFound)
		absent(t, path)
	})
	t.Run("invalid SQL does not create a missing database", func(t *testing.T) {
		path := filepath.Join(root, "invalid.sqlite")
		requireFault(t, execute(sqliteOperation(path, false, query("CREATE TABLE a (id INTEGER); CREATE TABLE b (id INTEGER)"))), protocol.ErrorCodeInvalidRequest)
		absent(t, path)
	})
	t.Run("check creates a missing database", func(t *testing.T) {
		path := filepath.Join(root, "created.sqlite")
		if err := execute(sqliteOperation(path, false, nil)); err != nil {
			t.Fatal(err)
		}
		if _, err := os.Stat(path); err != nil {
			t.Fatal(err)
		}
	})
	t.Run("write creates missing parents", func(t *testing.T) {
		path := filepath.Join(root, "nested", "deeper", "created.sqlite")
		if err := execute(sqliteOperation(path, false, query("CREATE TABLE items (id INTEGER)"))); err != nil {
			t.Fatal(err)
		}
		if result := sqliteQuery(t, path, "SELECT count(*) FROM items", 1); !reflect.DeepEqual(result.Rows[0][0], integerValue(0)) {
			t.Fatalf("unexpected rows %#v", result.Rows)
		}
	})
	for name, path := range map[string]string{
		"relative path":                 "relative.sqlite",
		"missing path with parent step": root + string(filepath.Separator) + "skip" + string(filepath.Separator) + ".." + string(filepath.Separator) + "dotted.sqlite",
	} {
		t.Run(name, func(t *testing.T) {
			requireFault(t, execute(sqliteOperation(path, false, nil)), protocol.ErrorCodeInvalidRequest)
			absent(t, filepath.Join(root, "dotted.sqlite"))
			absent(t, filepath.Join(root, "skip"))
		})
	}

	storage := filepath.Join(root, "profile")
	if err := os.MkdirAll(filepath.Join(storage, "data"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := execute(sqliteOperation(filepath.Join(storage, "data", "node.sqlite"), false, nil)); err != nil {
		t.Fatal(err)
	}
	confined := map[string]string{
		"existing database in Node storage": filepath.Join(storage, "data", "node.sqlite"),
		"missing database in Node storage":  filepath.Join(storage, "new", "other.sqlite"),
		"Node storage itself":               storage,
	}
	if runtime.GOOS != "windows" {
		link := filepath.Join(root, "storage-link")
		if err := os.Symlink(storage, link); err != nil {
			t.Fatal(err)
		}
		confined["symbolic link into Node storage"] = filepath.Join(link, "data", "node.sqlite")
		confined["missing database behind a link into Node storage"] = filepath.Join(link, "new", "other.sqlite")
	}
	for name, path := range confined {
		t.Run(name, func(t *testing.T) {
			operation := sqliteOperation(path, false, nil)
			operation.Storage = storage
			fault := requireFault(t, execute(operation), protocol.ErrorCodePermissionDenied)
			if fault.Message != "Node storage cannot be opened as a query database" {
				t.Fatalf("unexpected message %q", fault.Message)
			}
			absent(t, filepath.Join(storage, "new"))
		})
	}
	t.Run("sibling of Node storage is allowed", func(t *testing.T) {
		operation := sqliteOperation(filepath.Join(root, "profile-other.sqlite"), false, nil)
		operation.Storage = storage
		if err := execute(operation); err != nil {
			t.Fatal(err)
		}
	})
	t.Run("dangling link is not followed on creation", func(t *testing.T) {
		if runtime.GOOS == "windows" {
			t.Skip("symbolic links need privileges on Windows")
		}
		target := filepath.Join(root, "link-target.sqlite")
		link := filepath.Join(root, "dangling.sqlite")
		if err := os.Symlink(target, link); err != nil {
			t.Fatal(err)
		}
		if err := execute(sqliteOperation(link, false, nil)); err == nil {
			t.Fatal("created a database through a dangling link")
		}
		absent(t, target)
	})
	t.Run("read-only profile rejects writes and allows reads", func(t *testing.T) {
		path := filepath.Join(root, "readonly.sqlite")
		sqliteQuery(t, path, "CREATE TABLE items (id INTEGER)", 1)
		sqliteQuery(t, path, "INSERT INTO items VALUES (1)", 1)
		fault := requireFault(t, execute(sqliteOperation(path, true, query("DELETE FROM items"))), protocol.ErrorCodePermissionDenied)
		if fault.Message != "Database profile is read-only" {
			t.Fatalf("unexpected message %q", fault.Message)
		}
		if err := execute(sqliteOperation(path, true, query("SELECT id FROM items"))); err != nil {
			t.Fatal(err)
		}
		// Outside initialization, EXPLAIN is classified by the statement it explains.
		requireFault(t, execute(sqliteOperation(path, true, query("EXPLAIN DELETE FROM items"))), protocol.ErrorCodePermissionDenied)
		if result := sqliteQuery(t, path, "SELECT count(*) FROM items", 1); !reflect.DeepEqual(result.Rows[0][0], integerValue(1)) {
			t.Fatalf("read-only profile changed rows: %#v", result.Rows)
		}
	})
}
