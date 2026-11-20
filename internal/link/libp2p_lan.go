package link

import (
	"context"
	"log/slog"
	"net/netip"
	"slices"
	"time"

	"github.com/libp2p/go-libp2p/core/event"
	"github.com/libp2p/go-libp2p/core/network"
	ma "github.com/multiformats/go-multiaddr"
	manet "github.com/multiformats/go-multiaddr/net"

	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

const (
	libp2pLANLimit       = wire.MaxLANListeners
	libp2pLANDialTimeout = 3 * time.Second
)

func (ep *libp2pEndpoint) lanPrefixes() []netip.Prefix {
	addrs, err := ep.lanInterfaces()
	if err != nil {
		return nil
	}
	var prefixes []netip.Prefix
	for _, addr := range addrs {
		p, err := netip.ParsePrefix(addr.String())
		if err != nil || !p.Addr().IsPrivate() || !p.Masked().Addr().IsPrivate() {
			continue
		}
		prefixes = append(prefixes, p)
	}
	return prefixes
}

func lanListenerIP(addr ma.Multiaddr) netip.Addr {
	if !libp2pDirectAddr(addr, false) {
		return netip.Addr{}
	}
	ip, err := netip.ParseAddr(addr[0].Value())
	if err != nil || !ip.IsPrivate() || ip.Is4In6() {
		return netip.Addr{}
	}
	return ip
}

func (ep *libp2pEndpoint) lanListeners() []string {
	prefixes := ep.lanPrefixes()
	var local []ma.Multiaddr
	for _, p := range prefixes {
		addr, err := manet.FromIP(p.Addr().AsSlice())
		if err == nil {
			local = append(local, addr)
		}
	}
	if len(local) == 0 {
		return nil
	}
	listeners, _ := manet.ResolveUnspecifiedAddresses(ep.host.Network().ListenAddresses(), local)
	var out []string
	for _, addr := range listeners {
		ip := lanListenerIP(addr)
		if !ip.IsValid() || !slices.ContainsFunc(prefixes, func(p netip.Prefix) bool { return p.Addr() == ip }) {
			continue
		}
		value := addr.String()
		if !slices.Contains(out, value) {
			out = append(out, value)
		}
		if len(out) == libp2pLANLimit {
			break
		}
	}
	return out
}

func lanCandidates(listeners []string, prefixes []netip.Prefix) []ma.Multiaddr {
	if len(listeners) > libp2pLANLimit {
		return nil
	}
	var candidates []ma.Multiaddr
	for _, value := range listeners {
		if len(value) > libp2pMaxHintBytes {
			continue
		}
		addr, err := ma.NewMultiaddr(value)
		if err != nil || addr.String() != value {
			continue
		}
		ip := lanListenerIP(addr)
		if !ip.IsValid() || !slices.ContainsFunc(prefixes, func(p netip.Prefix) bool {
			if !p.Contains(ip) {
				return false
			}
			// Neither the network address nor directed broadcast is a peer.
			return !ip.Is4() || p.Bits() >= 31 || ip != p.Masked().Addr() && p.Contains(ip.Next())
		}) {
			continue
		}
		if !slices.ContainsFunc(candidates, addr.Equal) {
			candidates = append(candidates, addr)
		}
	}
	return candidates
}

func relayInspectStream(stream contextStream) bool {
	s, ok := stream.(*libp2pStream)
	return ok && s.raw.Protocol() == libp2pProtocol(libp2pNodeALPN) && libp2pRouteOf(s.raw.Conn()) == "relay"
}

func (conn *libp2pConnection) wakeLANLocked(listeners []string) {
	select {
	case conn.lanWake <- listeners:
	default:
	}
}

func (ep *libp2pEndpoint) wakeLANLocked(incomingOnly bool) {
	for conn := range ep.connections {
		if conn.route == "relay" && (!incomingOnly || conn.incoming) {
			conn.wakeLANLocked(nil)
		}
	}
}

func (ep *libp2pEndpoint) watchLANChanges() error {
	sub, err := ep.host.EventBus().Subscribe(new(event.EvtLocalAddressesUpdated))
	if err != nil {
		return err
	}
	ep.peerWork.Go(func() {
		defer sub.Close()
		for {
			select {
			case <-ep.ctx.Done():
				return
			case _, open := <-sub.Out():
				if !open {
					return
				}
				ep.mu.Lock()
				ep.wakeLANLocked(false)
				ep.mu.Unlock()
			}
		}
	})
	return nil
}

// Only paired Node sessions participate. The incoming side can also inspect
// back over the existing circuit when its own interfaces change; this needs
// neither an outbound address book entry nor an execution permission.
func (link *Link) watchLAN(parent context.Context, conn *libp2pConnection, initial []string) {
	ep := conn.endpoint
	ep.mu.Lock()
	defer ep.mu.Unlock()
	if conn.ctx.Err() != nil || parent.Err() != nil || conn.lanWake != nil {
		return
	}
	wake := make(chan []string, 1)
	conn.lanWake = wake
	ep.peerWork.Go(func() {
		ctx, cancel := context.WithCancel(parent)
		stop := context.AfterFunc(conn.ctx, cancel)
		defer func() { stop(); cancel() }()
		listeners := initial
		for ctx.Err() == nil {
			if len(listeners) == 0 {
				select {
				case <-ctx.Done():
					return
				case listeners = <-wake:
				}
			}
			if !link.isTrusted(conn.id) {
				return
			}
			info, err := conn.Info(ctx)
			if err != nil || info.Closed != nil {
				return
			}
			if info.Route == "relay" {
				if len(listeners) == 0 {
					bounded, expire := context.WithTimeout(ctx, livenessTimeout)
					reply, err := link.inspection(bounded, conn.openBi)
					expire()
					if err == nil && reply.Kind == wire.ReplyEndpoint && reply.Endpoint != nil {
						listeners = reply.Endpoint.LANListeners
					}
				}
				if len(listeners) != 0 && link.isTrusted(conn.id) {
					conn.dialLAN(ctx, listeners)
				}
			}
			listeners = nil
		}
	})
}

func (conn *libp2pConnection) dialLAN(ctx context.Context, listeners []string) {
	ep := conn.endpoint
	allowed := lanCandidates(listeners, ep.lanPrefixes())
	if len(allowed) == 0 || ctx.Err() != nil {
		return
	}
	if info, err := conn.Info(ctx); err != nil || info.Route != "relay" || info.Closed != nil {
		return
	}
	bounded, cancel := context.WithTimeout(ctx, libp2pLANDialTimeout)
	defer cancel()
	release, ok := ep.gater.limitLANDial(conn.peer, allowed)
	if !ok {
		return
	}
	defer release()
	dial := network.WithForceDirectDial(bounded, "Sailry LAN upgrade")
	ep.host.Peerstore().AddAddrs(conn.peer, allowed, libp2pLANDialTimeout)
	path, err := ep.host.Network().DialPeer(dial, conn.peer)
	// Route telemetry comes from live Swarm paths, never from a successful
	// dial return (which can reuse a concurrent connection).
	if err == nil && libp2pRouteOf(path) == "p2p" && !path.Stat().Limited && slices.ContainsFunc(allowed, path.RemoteMultiaddr().Equal) {
		id, ok := libp2pNodeID(path)
		if ok && id == conn.id {
			slog.Debug("libp2p LAN direct path available")
		}
	}
}
