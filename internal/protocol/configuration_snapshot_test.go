package protocol

import (
	"bytes"
	"encoding/json"
	"testing"
)

func TestConfigurationSnapshotPageIdentityAndFrame(t *testing.T) {
	root := NewRequestID()
	first := ConfigurationPageRequestID(root, 0)
	if first == root || first == (RequestID{}) || first != ConfigurationPageRequestID(root, 0) ||
		first == ConfigurationPageRequestID(root, 1) || first == ConfigurationPageRequestID(NewRequestID(), 0) {
		t.Fatal("configuration page identity is not stable and distinct")
	}
	page := ConfigurationSnapshotPage{
		Root: root, Source: NodeID{41}, Category: "providers", Revision: 7,
		Bytes: 2 * ConfigurationPageBytes, Digest: "fixture", Data: bytes.Repeat([]byte{0xff}, ConfigurationPageBytes),
	}
	request := NewRequest(NodeID{42}, Command{Kind: "configuration_follow", Data: &ConfigurationFollowCommand{Action: "apply_page", Page: &page}})
	request.ID = first
	encoded, err := json.Marshal(request)
	if err != nil {
		t.Fatal(err)
	}
	if len(encoded) >= MAX_FRAME_BYTES {
		t.Fatalf("one page exceeds a frame: %d", len(encoded))
	}
	var decoded Request
	if err := json.Unmarshal(encoded, &decoded); err != nil {
		t.Fatal(err)
	}
	command := decoded.Command.Data.(*ConfigurationFollowCommand)
	if decoded.ID != first || command.Page == nil || command.Page.Root != root || !bytes.Equal(command.Page.Data, page.Data) || decoded.Command.Durable() {
		t.Fatal("configuration page did not preserve exact binary data and identity")
	}
	output := Output{Kind: "configuration_snapshot", Data: ConfigurationSnapshotProgress{Root: root, NextIndex: 2, Complete: true}}
	encoded, err = json.Marshal(output)
	if err != nil {
		t.Fatal(err)
	}
	var result Output
	if err := json.Unmarshal(encoded, &result); err != nil {
		t.Fatal(err)
	}
	progress := result.Data.(*ConfigurationSnapshotProgress)
	if progress.Root != root || progress.NextIndex != 2 || !progress.Complete {
		t.Fatal("configuration completion receipt changed on the wire")
	}
}
