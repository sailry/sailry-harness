package protocol

import (
	"encoding/hex"
	"encoding/json"
	"testing"

	"lukechampine.com/blake3"
)

func TestResourceCallRecoveryBindsOriginalRequest(t *testing.T) {
	original := NewRequest(NodeID{3}, Command{
		Kind: "delete_knowledge_collection",
		Data: CommandDeleteKnowledgeCollectionPayload{ID: NewKnowledgeID()},
	})
	body, err := json.Marshal(original)
	if err != nil {
		t.Fatal(err)
	}
	digest := blake3.Sum256(body)
	recovery := NewRequest(original.Target, Command{
		Kind: "inspect_request",
		Data: CommandInspectRequestPayload{ID: original.ID, Digest: hex.EncodeToString(digest[:])},
	})
	valid := ResourceCall{Request: recovery, Original: &original}
	if err := valid.Validate(); err != nil {
		t.Fatal(err)
	}
	for _, scenario := range []string{"missing", "wrong_target", "wrong_id", "wrong_digest", "ordinary_command", "original_on_read"} {
		t.Run(scenario, func(t *testing.T) {
			call := valid
			captured := original
			call.Original = &captured
			payload := recovery.Command.Data.(CommandInspectRequestPayload)
			switch scenario {
			case "missing":
				call.Original = nil
			case "wrong_target":
				call.Request.Target = NodeID{8}
			case "wrong_id":
				payload.ID = NewRequestID()
				call.Request.Command.Data = payload
			case "wrong_digest":
				payload.Digest = hex.EncodeToString(make([]byte, len(digest)))
				call.Request.Command.Data = payload
			case "ordinary_command":
				captured.Command = Command{Kind: "snapshot"}
			case "original_on_read":
				call.Request = NewRequest(original.Target, Command{Kind: "list_knowledge_collections"})
			}
			if err := call.Validate(); err == nil {
				t.Fatal("unbound resource recovery was accepted")
			}
		})
	}
}

func TestResourceCallKeepsOrdinaryCommandsOut(t *testing.T) {
	for _, kind := range []string{"snapshot", "submit_turn", "read_file", "read_provider_key", "configuration_follow", "identity", "inspect_request"} {
		call := ResourceCall{Request: NewRequest(NodeID{1}, Command{Kind: kind})}
		if err := call.Validate(); err == nil {
			t.Fatalf("ordinary command %s entered resource channel", kind)
		}
	}
	for _, kind := range []string{"list_knowledge_collections", "query_knowledge", "read_knowledge_citation", "create_knowledge_collection", "update_knowledge_collection", "delete_knowledge_collection", "rebuild_knowledge_collection", "pause_knowledge_collection"} {
		call := ResourceCall{Request: NewRequest(NodeID{1}, Command{Kind: kind})}
		if err := call.Validate(); err != nil {
			t.Fatalf("knowledge command %s was rejected: %v", kind, err)
		}
	}
}

func TestHostHandlerRequestKeepsRecoveryAndReadOnlyContract(t *testing.T) {
	target := NodeID{7}
	request := NewRequest(target, Command{Kind: "call_host_handler", Data: HostHandlerCall{Handler: "save", Input: map[string]any{"id": "kept"}}})
	if !request.Command.Durable() {
		t.Fatal("mutable host handler was not durable")
	}
	body, err := json.Marshal(request)
	if err != nil {
		t.Fatal(err)
	}
	var decoded Request
	if err := json.Unmarshal(body, &decoded); err != nil || decoded.Target != target || decoded.ID != request.ID {
		t.Fatalf("host call changed identity: %+v %v", decoded, err)
	}
	digest := blake3.Sum256(body)
	inspection := NewRequest(target, Command{Kind: "inspect_request", Data: CommandInspectRequestPayload{
		ID: request.ID, Digest: hex.EncodeToString(digest[:]),
	}})
	if err := (ResourceCall{Request: inspection, Original: &request}).Validate(); err != nil {
		t.Fatal(err)
	}
	request.Command.Data = HostHandlerCall{Handler: "history", Input: nil, ReadOnly: true}
	if request.Command.Durable() {
		t.Fatal("read-only host handler was durable")
	}
	if err := (ResourceCall{Request: inspection, Original: &request}).Validate(); err == nil {
		t.Fatal("read-only call accepted as a durable recovery")
	}
}
