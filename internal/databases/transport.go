package databases

import (
	"context"
	"crypto/tls"
	"errors"
	"net"

	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/ssh"
)

type dialFunc = func(ctx context.Context, network, address string) (net.Conn, error)

// openTransport returns the dialer for one operation. A tunnelled operation
// owns one verified SSH session and reaches the database through direct-tcpip
// channels, so the database address is resolved by the SSH server while TLS
// still verifies the database's own hostname. release closes the session and
// every channel with it.
func openTransport(ctx context.Context, tunnel *SSH, tunnelled bool) (dialFunc, func(), error) {
	if !tunnelled {
		return (&net.Dialer{}).DialContext, func() {}, nil
	}
	if tunnel == nil {
		return nil, nil, unavailable()
	}
	client, err := ssh.Connect(ctx, tunnel.Profile, tunnel.Credential)
	if err != nil {
		var required *ssh.HostKeyRequired
		if errors.As(err, &required) {
			return nil, nil, fault(protocol.ErrorCodePermissionDenied, "Verify the SSH connection host key before connecting to the database")
		}
		return nil, nil, err
	}
	return client.DialContext, func() { _ = client.Close() }, nil
}

// verifiedTLS authenticates the server certificate against the execution
// Node's system roots and the database hostname. Both Prefer and Require use
// it; Prefer differs only in accepting a server that offers no TLS at all.
func verifiedTLS(host string) *tls.Config {
	return &tls.Config{ServerName: host, MinVersion: tls.VersionTLS12}
}
