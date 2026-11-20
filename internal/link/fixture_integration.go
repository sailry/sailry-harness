//go:build integration

package link

import (
	"context"

	"github.com/sailry/sailry-harness/internal/link/peertransport"
)

func OpenRelayFixture(ctx context.Context, path, relay string, handler Handler) (*Link, error) {
	identity, store, err := openControllerStore(path)
	if err != nil {
		return nil, err
	}
	endpoint, err := Bind(ctx, EndpointOptions{
		Identity: identity, Scope: NetworkScope{Bind: "127.0.0.1:0", Relays: []string{relay}}, Peers: store, Handler: handler,
	})
	if err != nil {
		_ = store.Close()
		return nil, err
	}
	endpoint.controller = store
	return endpoint, nil
}

func (remote *Remote) FixtureConnectionInfo(ctx context.Context) (peertransport.ConnectionInfo, error) {
	connection, _, err := remote.link.connection(ctx, remote.address, true)
	if err != nil {
		return peertransport.ConnectionInfo{}, err
	}
	return connection.Info(ctx)
}
