//go:build integration

package link

import (
	"context"

	"github.com/sailry/sailry-harness/internal/link/iroh"
)

func OpenRelayFixture(ctx context.Context, path, relay string, handler Handler, roots [][]byte) (*Link, error) {
	identity, store, err := openControllerStore(path)
	if err != nil {
		return nil, err
	}
	endpoint, err := bindConfigured(ctx, EndpointOptions{
		Identity: identity, Scope: NetworkScope{Relays: []string{relay}}, Peers: store, Handler: handler,
	}, iroh.Config{
		SecretKey: identity.SecretKey(), Mode: "custom",
		Relays: []string{relay}, RootsDER: roots, RelayOnly: true,
	})
	if err != nil {
		_ = store.Close()
		return nil, err
	}
	endpoint.controller = store
	return endpoint, nil
}

func (remote *Remote) FixtureConnectionInfo(ctx context.Context) (iroh.ConnectionInfo, error) {
	connection, _, err := remote.link.connection(ctx, remote.address, true)
	if err != nil {
		return iroh.ConnectionInfo{}, err
	}
	return connection.Info(ctx)
}
