//go:build integration

package databases

import (
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"io"
	"net"
	"os"
	"reflect"
	"strconv"
	"sync/atomic"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
	cryptossh "golang.org/x/crypto/ssh"
)

// startTunnelFixture serves an in-process SSH host that forwards direct-tcpip
// channels and records the last requested destination.
func startTunnelFixture(t *testing.T) (protocol.SshProfile, protocol.SshCredential, *atomic.Pointer[string]) {
	t.Helper()
	_, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	signer, err := cryptossh.NewSignerFromKey(private)
	if err != nil {
		t.Fatal(err)
	}
	config := &cryptossh.ServerConfig{PasswordCallback: func(cryptossh.ConnMetadata, []byte) (*cryptossh.Permissions, error) { return nil, nil }}
	config.AddHostKey(signer)
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	forwarded := &atomic.Pointer[string]{}
	go func() {
		for {
			connection, err := listener.Accept()
			if err != nil {
				return
			}
			go func() {
				defer connection.Close()
				server, channels, requests, err := cryptossh.NewServerConn(connection, config)
				if err != nil {
					return
				}
				defer server.Close()
				go cryptossh.DiscardRequests(requests)
				for offered := range channels {
					var destination struct {
						Host       string
						Port       uint32
						Origin     string
						OriginPort uint32
					}
					if offered.ChannelType() != "direct-tcpip" || cryptossh.Unmarshal(offered.ExtraData(), &destination) != nil {
						_ = offered.Reject(cryptossh.UnknownChannelType, "direct-tcpip expected")
						continue
					}
					target := net.JoinHostPort(destination.Host, strconv.FormatUint(uint64(destination.Port), 10))
					upstream, err := net.Dial("tcp", target)
					if err != nil {
						_ = offered.Reject(cryptossh.ConnectionFailed, "destination refused")
						continue
					}
					channel, requests, err := offered.Accept()
					if err != nil {
						_ = upstream.Close()
						return
					}
					forwarded.Store(&target)
					go cryptossh.DiscardRequests(requests)
					go func() { _, _ = io.Copy(upstream, channel); _ = upstream.Close() }()
					go func() { _, _ = io.Copy(channel, upstream); _ = channel.Close() }()
				}
			}()
		}
	}()
	profile := protocol.SshProfile{
		ID: protocol.NewSshID(), Revision: 1, Name: "tunnel", Host: "127.0.0.1", Port: uint16(listener.Addr().(*net.TCPAddr).Port),
		Username: "fixture", Authentication: protocol.SshAuthenticationPassword,
		HostKey: &protocol.SshHostKey{Algorithm: signer.PublicKey().Type(), Fingerprint: cryptossh.FingerprintSHA256(signer.PublicKey())},
	}
	credential := protocol.SshCredential{Kind: "password", Data: protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret("fixture")}}
	return profile, credential, forwarded
}

// TestServerDatabases runs against real servers named by the environment:
//
//	SAILRY_TEST_POSTGRES_HOST, _PORT, _DATABASE, _USERNAME, _PASSWORD, _TLS
//	SAILRY_TEST_MYSQL_HOST, _PORT, _DATABASE, _USERNAME, _PASSWORD, _TLS
//
// _TLS is disable, prefer or require and defaults to disable. An engine whose
// host is unset is skipped. The account needs CREATE TEMPORARY TABLE.
func TestServerDatabases(t *testing.T) {
	engines := map[string]protocol.DatabaseEngine{
		"POSTGRES": protocol.DatabaseEnginePostgres,
		"MYSQL":    protocol.DatabaseEngineMysql,
	}
	for name, engine := range engines {
		t.Run(name, func(t *testing.T) {
			prefix := "SAILRY_TEST_" + name + "_"
			host := os.Getenv(prefix + "HOST")
			if host == "" {
				t.Skip(prefix + "HOST is not set")
			}
			port, err := strconv.ParseUint(os.Getenv(prefix+"PORT"), 10, 16)
			if err != nil {
				t.Fatalf("%sPORT: %v", prefix, err)
			}
			mode := protocol.DatabaseTls(os.Getenv(prefix + "TLS"))
			if mode == "" {
				mode = protocol.DatabaseTlsDisable
			}
			operation := func(readOnly bool, sql string, limit uint32) Operation {
				operation := Operation{
					Profile: protocol.DatabaseProfile{
						Connection: protocol.DatabaseConnection{Kind: "network", Data: protocol.DatabaseConnectionNetworkPayload{
							Engine: engine, Host: host, Port: uint16(port), Tls: mode,
							Database: os.Getenv(prefix + "DATABASE"), Username: os.Getenv(prefix + "USERNAME"),
						}},
						ReadOnly: readOnly,
					},
					Password: protocol.NewSecret(os.Getenv(prefix + "PASSWORD")),
				}
				if sql != "" {
					operation.Query = &Query{SQL: sql, RowLimit: limit, TimeoutMS: 30_000}
				}
				return operation
			}
			run := func(readOnly bool, sql string, limit uint32) (protocol.DatabaseResultSet, error) {
				outcome, err := Execute(context.Background(), operation(readOnly, sql, limit))
				if err != nil {
					return protocol.DatabaseResultSet{}, err
				}
				return outcome.Data.(protocol.DatabaseResultSet), nil
			}

			if outcome, err := Execute(context.Background(), operation(false, "", 0)); err != nil || outcome.Kind != "connected" {
				t.Fatalf("check: %#v %v", outcome, err)
			}
			series := "SELECT 1 AS n UNION ALL SELECT 2 UNION ALL SELECT 3"
			result, err := run(false, series, 2)
			if err != nil {
				t.Fatal(err)
			}
			if !result.Truncated || len(result.Rows) != 2 || !reflect.DeepEqual(result.Columns, []string{"n"}) {
				t.Fatalf("unexpected page %#v", result)
			}
			if _, err := run(false, "SELECT 1; SELECT 2", 1); err == nil {
				t.Fatal("multiple statements were accepted")
			} else {
				requireFault(t, err, protocol.ErrorCodeInvalidRequest)
			}
			if _, err := run(false, "not SQL", 1); err == nil {
				t.Fatal("invalid SQL was accepted")
			} else {
				requireFault(t, err, protocol.ErrorCodeInvalidRequest)
			}
			if _, err := run(true, "CREATE TABLE sailry_read_only_probe (id INT)", 1); err == nil {
				t.Fatal("read-only profile created a table")
			} else {
				requireFault(t, err, protocol.ErrorCodeInvalidRequest)
			}
			browse := operation(false, "", 0)
			browse.Browse = &Browse{}
			if outcome, err := Execute(context.Background(), browse); err != nil || outcome.Kind != "catalog" {
				t.Fatalf("catalog: %#v %v", outcome, err)
			}
			wrong := operation(false, "", 0)
			wrong.Password = protocol.NewSecret(os.Getenv(prefix+"PASSWORD") + "-wrong")
			if _, err := Execute(context.Background(), wrong); err == nil {
				t.Fatal("wrong password connected")
			} else {
				requireFault(t, err, protocol.ErrorCodeUnavailable)
			}
			t.Run("ssh tunnel", func(t *testing.T) {
				profile, credential, forwarded := startTunnelFixture(t)
				tunneled := operation(false, series, 3)
				tunneled.Profile.Connection = protocol.DatabaseConnection{Kind: "ssh", Data: protocol.DatabaseConnectionSshPayload{
					Engine: engine, Host: host, Port: uint16(port), Tls: mode, Ssh: profile.ID,
					Database: os.Getenv(prefix + "DATABASE"), Username: os.Getenv(prefix + "USERNAME"),
				}}
				tunneled.SSH = &SSH{Profile: profile, Credential: credential}
				outcome, err := Execute(context.Background(), tunneled)
				if err != nil {
					t.Fatal(err)
				}
				result := outcome.Data.(protocol.DatabaseResultSet)
				target := net.JoinHostPort(host, strconv.FormatUint(port, 10))
				if len(result.Rows) != 3 || result.Truncated || forwarded.Load() == nil || *forwarded.Load() != target {
					t.Fatalf("tunnel returned %#v through %v, expected %s", result, forwarded.Load(), target)
				}
			})
		})
	}
}
