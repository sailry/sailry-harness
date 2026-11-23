package node

import (
	"database/sql"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRetiredPersonalStorageTransferRejectsNewRequests(t *testing.T) {
	t.Parallel()
	node := nodeFixture(t)
	source := protocol.NodeID{97}
	if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		return store.PairPeerIdentity(t.Context(), db, node.ID(), source, link.DirectionController, true)
	}); err != nil {
		t.Fatal(err)
	}
	for _, command := range []protocol.ConfigurationFollowCommand{
		{Action: "kv_pending"},
		{Action: "kv_snapshot", KVSnapshot: &protocol.PersonalKVSnapshot{Generation: 1, Records: []protocol.PersonalKVRecord{}}},
		{Action: "kv_ack", KVReceipt: &protocol.PersonalKVReceipt{Executor: node.ID(), Request: protocol.NewRequestID(), Sequence: 1}},
	} {
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "configuration_follow", Data: &command})
		result := dispatchFixture(t, node, source, request)
		if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeNotConfigured {
			t.Fatalf("%s was not retired: %+v", command.Action, result.Fault)
		}
	}
}
