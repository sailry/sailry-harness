package protocol

import (
	"encoding/hex"
	"encoding/json"
	"testing"

	"lukechampine.com/blake3"
)

func TestScopedKnowledgeCallKeepsItsStorageTargetAndRecoveryIdentity(t *testing.T) {
	scope := DefaultPluginContext()
	scope.Package = PluginReference{Name: "notes", Digest: "captured"}
	original := NewRequest(NodeID{3}, Command{Kind: "delete_knowledge_collection", Data: CommandDeleteKnowledgeCollectionPayload{ID: NewKnowledgeID()}})
	original.Plugin = &scope
	body, err := json.Marshal(original)
	if err != nil {
		t.Fatal(err)
	}
	digest := blake3.Sum256(body)
	recovery := NewRequest(original.Target, Command{Kind: "inspect_request", Data: CommandInspectRequestPayload{ID: original.ID, Digest: hex.EncodeToString(digest[:])}})
	for _, call := range []ScopedKnowledgeCall{{Request: original}, {Request: recovery, Original: &original}} {
		if err := call.Validate(); err != nil {
			t.Fatal(err)
		}
		encoded, err := json.Marshal(call)
		if err != nil {
			t.Fatal(err)
		}
		var decoded ScopedKnowledgeCall
		if err := json.Unmarshal(encoded, &decoded); err != nil || decoded.Validate() != nil || decoded.Request.Target != original.Target || decoded.Request.ID != call.Request.ID {
			t.Fatalf("scoped request changed: %+v: %v", decoded, err)
		}
	}
	for _, scenario := range []string{"unscoped", "ordinary", "host_handler", "changed_target", "changed_original"} {
		t.Run(scenario, func(t *testing.T) {
			copy := original
			call := ScopedKnowledgeCall{Request: recovery, Original: &copy}
			switch scenario {
			case "unscoped":
				copy.Plugin = nil
				call = ScopedKnowledgeCall{Request: copy}
			case "ordinary":
				copy.Command = Command{Kind: "snapshot"}
				call = ScopedKnowledgeCall{Request: copy}
			case "host_handler":
				copy.Command = Command{Kind: "call_host_handler", Data: HostHandlerCall{Handler: "save"}}
				call = ScopedKnowledgeCall{Request: copy}
			case "changed_target":
				call.Request.Target = NodeID{4}
			case "changed_original":
				copy.ID = NewRequestID()
			}
			if err := call.Validate(); err == nil {
				t.Fatal("invalid scoped knowledge call was admitted")
			}
		})
	}
}
