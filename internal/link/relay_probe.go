package link

import (
	"context"
	"errors"
	"time"

	"github.com/libp2p/go-libp2p"
	"github.com/libp2p/go-libp2p/core/peer"
	"github.com/libp2p/go-libp2p/p2p/protocol/ping"
)

func ProbeRelay(ctx context.Context, address string) (time.Duration, error) {
	relay, err := peer.AddrInfoFromString(address)
	if err != nil {
		return 0, err
	}
	probe, err := libp2p.New(libp2p.NoListenAddrs, libp2p.DisableRelay(), libp2p.DisableMetrics())
	if err != nil {
		return 0, err
	}
	defer probe.Close()
	if err := probe.Connect(ctx, *relay); err != nil {
		return 0, err
	}
	scoped, cancel := context.WithCancel(ctx)
	defer cancel()
	select {
	case <-scoped.Done():
		return 0, scoped.Err()
	case result, ok := <-ping.Ping(scoped, probe, relay.ID):
		if !ok {
			return 0, errors.New("relay ping ended without a result")
		}
		return result.RTT, result.Error
	}
}
