//go:build integration

package link

import (
	"context"
	"io"
	"net"
	"net/netip"
	"strconv"
	"testing"
	"time"

	"github.com/libp2p/go-libp2p/core/network"
	ma "github.com/multiformats/go-multiaddr"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func localPrivateLAN() (net.Addr, string) {
	addrs, err := net.InterfaceAddrs()
	if err != nil {
		return nil, ""
	}
	for _, addr := range addrs {
		prefix, err := netip.ParsePrefix(addr.String())
		if err != nil || !prefix.Addr().Is4() || !prefix.Addr().IsPrivate() || !prefix.Masked().Addr().IsPrivate() {
			continue
		}
		ip := prefix.Addr().String()
		listener, err := net.Listen("tcp4", net.JoinHostPort(ip, "0"))
		if err == nil {
			_ = listener.Close()
			return addr, ip
		}
	}
	return nil, ""
}

func openLANTestStream(t *testing.T, ctx context.Context, remote *Remote, id protocol.NodeID, direct bool) *libp2pStream {
	t.Helper()
	stream, err := remote.Open(ctx, protocol.NewStreamID())
	if err != nil {
		t.Fatal(err)
	}
	raw, ok := stream.(*libp2pStream)
	if !ok {
		_ = stream.Close()
		t.Fatal("stream is not a libp2p stream")
	}
	t.Cleanup(func() { _ = raw.Close() })
	path := raw.raw.Conn()
	peerID, valid := libp2pNodeID(path)
	if !valid || peerID != id || path.RemotePeer() != libp2pTestPeerID(t, id) || path.IsClosed() ||
		raw.raw.Protocol() != libp2pProtocol(libp2pNodeALPN) {
		t.Fatal("stream is not on an authenticated Node connection")
	}
	if direct && (libp2pRouteOf(path) != "p2p" || path.Stat().Limited) ||
		!direct && (libp2pRouteOf(path) != "relay" || !path.Stat().Limited) {
		t.Fatalf("unexpected stream path: %s", path.RemoteMultiaddr())
	}
	return raw
}

func finishLANTestStream(t *testing.T, ctx context.Context, stream *libp2pStream, data, expected string) {
	t.Helper()
	stop := context.AfterFunc(ctx, func() { _ = stream.Close() })
	defer stop()
	if _, err := stream.WriteContext(ctx, []byte(data)); err != nil {
		t.Fatal(err)
	}
	if err := stream.CloseWrite(); err != nil {
		t.Fatal(err)
	}
	got, err := io.ReadAll(&scopedReader{ctx: ctx, stream: stream})
	if err != nil || string(got) != "stored:"+expected {
		t.Fatalf("stream response = %q, %v", got, err)
	}
}

func TestLibp2pLANUpgradePreservesRelayStream(t *testing.T) {
	private, ip := localPrivateLAN()
	for _, mode := range []string{"tcp", "quic", "failure"} {
		t.Run(mode, func(t *testing.T) {
			if ip == "" && mode != "failure" {
				t.Skip("no bindable private IPv4 interface")
			}
			ctx, cancel := context.WithTimeout(t.Context(), 40*time.Second)
			defer cancel()
			relay, _ := startLibp2pTestRelay(t, 1)
			scope := NetworkScope{Bind: "127.0.0.1:0", Relays: []string{relay}}
			nodeID, controllerID := libp2pTestIdentity(41), libp2pTestIdentity(81)
			handler := &libp2pTestHandler{node: nodeID.ID(), controller: controllerID.ID()}
			node := bindLibp2pTestNode(t, ctx, scope, nodeID, handler)
			controller := bindLibp2pTestNode(t, ctx, scope, controllerID, nil)
			nodeEP := node.endpoint.(*libp2pEndpoint)
			controllerEP := controller.endpoint.(*libp2pEndpoint)
			interfaces := func() ([]net.Addr, error) {
				if private != nil {
					return []net.Addr{private}, nil
				}
				return nil, nil
			}
			nodeEP.lanInterfaces = interfaces
			controllerEP.lanInterfaces = interfaces
			pairLibp2pTest(t, ctx, node, controller, "relay", relay)
			stored, err := controller.Peers(ctx)
			if err != nil || len(stored) != 1 || stored[0].ID != nodeID.ID() || len(stored[0].Addrs) != 1 ||
				len(libp2pAddressHints(t, stored[0], "relay")) != 1 || node.Direction(controllerID.ID()) != DirectionController {
				t.Fatalf("pairing did not save the authenticated circuit-only address: %v, %v", stored, err)
			}
			remote := controller.Remote(stored[0])
			old := openLANTestStream(t, ctx, remote, nodeID.ID(), false)
			if _, err := old.WriteContext(ctx, []byte("before:")); err != nil {
				t.Fatal(err)
			}

			paths := make(chan struct{}, 1)
			targetPeer := libp2pTestPeerID(t, nodeID.ID())
			watcher := &network.NotifyBundle{ConnectedF: func(_ network.Network, path network.Conn) {
				if path.RemotePeer() == targetPeer && libp2pRouteOf(path) == "p2p" {
					select {
					case paths <- struct{}{}:
					default:
					}
				}
			}}
			controllerEP.host.Network().Notify(watcher)
			t.Cleanup(func() { controllerEP.host.Network().StopNotify(watcher) })

			if mode == "failure" && ip != "" {
				listener, err := net.Listen("tcp4", net.JoinHostPort(ip, "0"))
				if err != nil {
					t.Fatal(err)
				}
				accepted := make(chan error, 1)
				go func() {
					conn, err := listener.Accept()
					if err == nil {
						_ = conn.Close()
					}
					accepted <- err
				}()
				t.Cleanup(func() { _ = listener.Close() })
				candidate := "/ip4/" + ip + "/tcp/" + strconv.Itoa(listener.Addr().(*net.TCPAddr).Port)
				logical, _, err := controller.connection(ctx, stored[0], true)
				if err != nil {
					t.Fatal(err)
				}
				logical.(*libp2pConnection).dialLAN(ctx, []string{candidate})
				select {
				case err := <-accepted:
					if err != nil {
						t.Fatalf("accept LAN dial: %v", err)
					}
				case <-ctx.Done():
					t.Fatal("LAN dial did not reach the local rejecting listener")
				}
			} else {
				if mode == "failure" {
					t.Log("no bindable private IPv4; checking relay fallback without a dial candidate")
				} else {
					listen := "/ip4/" + ip + "/tcp/0"
					if mode == "quic" {
						listen = "/ip4/" + ip + "/udp/0/quic-v1"
					}
					if err := nodeEP.host.Network().Listen(ma.StringCast(listen)); err != nil {
						t.Fatal(err)
					}
				}
				if err := controller.NetworkChanged(ctx); err != nil {
					t.Fatal(err)
				}
				if mode != "failure" {
					select {
					case <-paths:
					case <-ctx.Done():
						t.Fatalf("no direct LAN path: %v", ctx.Err())
					}
				}
			}

			if mode == "failure" {
				assertLibp2pTestPhysicalPath(t, controller, nodeID.ID(), "relay")
			}
			newStream := openLANTestStream(t, ctx, remote, nodeID.ID(), mode != "failure")
			if mode != "failure" {
				address := newStream.raw.Conn().RemoteMultiaddr()
				if host, err := address.ValueForProtocol(ma.P_IP4); err != nil || host != ip {
					t.Fatalf("direct stream did not reach the Node's private interface: %s", address)
				}
				protocol := ma.P_TCP
				if mode == "quic" {
					protocol = ma.P_QUIC_V1
				}
				if _, err := address.ValueForProtocol(protocol); err != nil {
					t.Fatalf("wrong direct transport: %s", address)
				}
			}
			finishLANTestStream(t, ctx, newStream, "new", "new")
			if old.raw.Conn().IsClosed() || libp2pRouteOf(old.raw.Conn()) != "relay" {
				t.Fatal("original relay path was closed")
			}
			finishLANTestStream(t, ctx, old, "after", "before:after")
		})
	}
}
