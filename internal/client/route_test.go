package client

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type routedClientTransport struct {
	*clientTransport
	entry protocol.NodeID
}

func (transport routedClientTransport) Route() protocol.RouteKey {
	return protocol.RouteKey{Entry: transport.entry, Target: transport.Target()}
}

func TestRouteSeparatesEntriesWithSameResourceTarget(t *testing.T) {
	direct := New(&clientTransport{})
	via := New(routedClientTransport{clientTransport: &clientTransport{}, entry: protocol.NodeID{2}})
	t.Cleanup(func() { _ = direct.Close(); _ = via.Close() })
	if direct.Target() != via.Target() || direct.Route() == via.Route() {
		t.Fatal("two entry identities were merged by their common target")
	}
	if direct.Route() != (protocol.RouteKey{Entry: direct.Target(), Target: direct.Target()}) {
		t.Fatal("direct route did not retain its target")
	}
	local := New(link.NewLocal(protocol.NodeID{4}, protocol.NodeID{4}, nil))
	t.Cleanup(func() { _ = local.Close() })
	if local.Route() != (protocol.RouteKey{Entry: protocol.NodeID{4}, Target: protocol.NodeID{4}}) {
		t.Fatal("local route identity changed")
	}
}
