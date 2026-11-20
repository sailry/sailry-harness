package link

import (
	"context"
	"io"
	"testing"
	"testing/synctest"

	lpcrypto "github.com/libp2p/go-libp2p/core/crypto"
	"github.com/libp2p/go-libp2p/core/network"
	"github.com/libp2p/go-libp2p/core/peer"
	lpprotocol "github.com/libp2p/go-libp2p/core/protocol"
	ma "github.com/multiformats/go-multiaddr"

	"github.com/sailry/sailry-harness/internal/link/peertransport"
)

type livenessPath struct {
	network.Conn
	addr    ma.Multiaddr
	peer    peer.ID
	key     lpcrypto.PubKey
	streams []network.Stream
	closed  bool
}

func (p *livenessPath) RemoteMultiaddr() ma.Multiaddr    { return p.addr }
func (p *livenessPath) RemotePeer() peer.ID              { return p.peer }
func (p *livenessPath) RemotePublicKey() lpcrypto.PubKey { return p.key }
func (p *livenessPath) GetStreams() []network.Stream     { return p.streams }
func (p *livenessPath) IsClosed() bool                   { return p.closed }
func (p *livenessPath) Close() error                     { p.closed = true; return nil }

type livenessNetwork struct {
	network.Network
	paths []network.Conn
}

func (n livenessNetwork) ConnsToPeer(peer.ID) []network.Conn { return n.paths }

type livenessHost struct {
	lanTestHost
	next network.Stream
}

func (h *livenessHost) NewStream(ctx context.Context, _ peer.ID, _ ...lpprotocol.ID) (network.Stream, error) {
	if h.next == nil {
		<-ctx.Done()
		return nil, ctx.Err()
	}
	return h.next, nil
}

type livenessStream struct {
	*libp2pPipeStream
	path *livenessPath
}

func (s *livenessStream) Conn() network.Conn      { return s.path }
func (s *livenessStream) Protocol() lpprotocol.ID { return libp2pProtocol(libp2pNodeALPN) }

func TestNetworkChangedScopesLibp2pProbeTimeout(t *testing.T) {
	for _, scenario := range []string{"direct-and-relay", "shared-relay", "idle-last-path", "open-timeout"} {
		t.Run(scenario, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				link := newFixtureLink(t)
				link.timeout = context.WithTimeout
				link.endpoint = fixtureRebindingEndpoint{}
				id := libp2pTestIdentity(31).ID()
				key, err := lpcrypto.UnmarshalEd25519PublicKey(id[:])
				if err != nil {
					t.Fatal(err)
				}
				remote, err := peer.IDFromPublicKey(key)
				if err != nil {
					t.Fatal(err)
				}
				relay := &livenessPath{addr: ma.StringCast(fixtureRelayTCP + "/p2p-circuit"), peer: remote, key: key}
				direct := &livenessPath{addr: ma.StringCast("/ip4/192.168.1.4/tcp/8159"), peer: remote, key: key}
				net := &livenessNetwork{paths: []network.Conn{relay}}
				host := &livenessHost{lanTestHost: lanTestHost{network: net}}
				ep := &libp2pEndpoint{host: host, ctx: link.ctx, connections: make(map[*libp2pConnection]struct{})}
				conn := ep.addConnectionLocked(remote, id, libp2pNodeALPN, false, nil)
				slot, err := link.slot(t.Context(), id)
				if err != nil {
					t.Fatal(err)
				}
				slot.connection = conn
				link.releaseSlot(slot)

				var old peertransport.Stream
				var receiver *libp2pStream
				if scenario != "idle-last-path" {
					left, right := libp2pPipePair(t)
					host.next = &livenessStream{left, relay}
					relay.streams = []network.Stream{host.next}
					receiver = newLibp2pStream(right, nil, nil)
					defer receiver.Close()
					old, err = conn.OpenBi(t.Context())
					if err != nil {
						t.Fatal(err)
					}
					defer old.Close()
					if _, err := old.Write([]byte("before")); err != nil {
						t.Fatal(err)
					}
					var first [6]byte
					if _, err := io.ReadFull(receiver, first[:]); err != nil || string(first[:]) != "before" {
						t.Fatalf("original relay stream before probe: %q, %v", first, err)
					}
				}
				probePath := relay
				if scenario == "direct-and-relay" || scenario == "open-timeout" {
					net.paths = append(net.paths, direct)
					probePath = direct
				}
				host.next = nil
				if scenario != "open-timeout" {
					left, right := libp2pPipePair(t)
					host.next = &livenessStream{left, probePath}
					silent := newLibp2pStream(right, nil, nil)
					defer silent.Close()
					go func() { _, _ = io.Copy(io.Discard, silent) }()
				}
				if err := link.NetworkChanged(t.Context()); err != nil {
					t.Fatal(err)
				}
				if relay.closed != (scenario == "idle-last-path") || direct.closed != (scenario == "direct-and-relay") {
					t.Fatalf("closed paths: relay %v, direct %v", relay.closed, direct.closed)
				}
				if scenario == "idle-last-path" {
					if slot.connection != nil || conn.ctx.Err() == nil {
						t.Fatal("last failed path still cached")
					}
					return
				}
				if slot.connection != conn || conn.ctx.Err() != nil {
					t.Fatal("probe canceled the logical owner of the relay stream")
				}
				if _, err := old.Write([]byte("after")); err != nil {
					t.Fatal(err)
				}
				if err := old.CloseWrite(); err != nil {
					t.Fatal(err)
				}
				if data, err := io.ReadAll(receiver); err != nil || string(data) != "after" {
					t.Fatalf("original relay stream: %q, %v", data, err)
				}
			})
		})
	}
}
