package wire

import (
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderDirectoryPaging(t *testing.T) {
	provider := protocol.DefaultConversationProvider()
	provider.Api, provider.Authentication = protocol.ConversationModelApiResponses, protocol.AuthenticationApiKey
	provider.ID, provider.Name = protocol.ProviderID{3}, strings.Repeat("p", protocol.MAX_FRAME_BYTES)
	catalog := protocol.ProviderDirectory{Node: protocol.NodeID{1}, Cursor: 3, Providers: []protocol.ConversationProvider{provider}}
	output := protocol.Output{Kind: "provider_directory", Data: catalog}
	reply := Reply{Kind: ReplyCompleted, Completed: &Response{Output: &output}}
	var parts []Reply
	if err := WriteReply(reply, func(part Reply) error { parts = append(parts, part); return nil }); err != nil {
		t.Fatal(err)
	}
	if len(parts) < 2 || parts[0].SnapshotStart == nil || parts[0].SnapshotStart.Cursor != catalog.Cursor {
		t.Fatal("provider directory did not carry its cursor across frames")
	}
	var assembly Assembly
	var result *Reply
	for _, part := range parts {
		var err error
		result, err = assembly.Accept(part)
		if err != nil {
			t.Fatal(err)
		}
	}
	if result == nil || result.Completed == nil || result.Completed.Output.Kind != output.Kind {
		t.Fatal("paged provider directory did not complete")
	}
	got := result.Completed.Output.Data.(*protocol.ProviderDirectory)
	if got.Node != catalog.Node || got.Cursor != catalog.Cursor || len(got.Providers) != 1 || got.Providers[0].Name != provider.Name {
		t.Fatal("paged provider directory changed in transit")
	}
}
