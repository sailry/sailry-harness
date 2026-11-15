package mobile

import (
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPeerMetadataPreservesRoutingAndDirectSourceIdentity(t *testing.T) {
	t.Parallel()
	address := link.EndpointAddress{ID: protocol.NodeID{2}, Addrs: []json.RawMessage{json.RawMessage(`{"Ip":"127.0.0.1:4321"}`)}}
	source := protocol.NodeID{3}
	body, err := peerAddressMetadata(address, link.PeerRelationship{ID: address.ID, Direct: true, Name: "Remembered host", Sources: []protocol.NodeID{source}})
	if err != nil {
		t.Fatal(err)
	}
	var routed link.EndpointAddress
	if err := json.Unmarshal([]byte(body), &routed); err != nil || routed.ID != address.ID || len(routed.Addrs) != 1 {
		t.Fatalf("metadata changed routing address: %+v %v", routed, err)
	}
	var decoded struct {
		Relationship struct {
			Direct  bool     `json:"direct"`
			Sources []string `json:"sources"`
			Name    string   `json:"name"`
		} `json:"relationship"`
	}
	if err := json.Unmarshal([]byte(body), &decoded); err != nil {
		t.Fatal(err)
	}
	if !decoded.Relationship.Direct || decoded.Relationship.Name != "Remembered host" || len(decoded.Relationship.Sources) != 1 || decoded.Relationship.Sources[0] != source.String() {
		t.Fatalf("native provenance lost direct priority or canonical source ID: %+v", decoded.Relationship)
	}
}
