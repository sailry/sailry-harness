package wire

import (
	"bytes"
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestEnvelopeRetainsRoutedCommandBody(t *testing.T) {
	target := protocol.NodeID{3}
	request := protocol.NewRequest(target, protocol.Command{Kind: "snapshot"})
	body, err := json.Marshal(request)
	if err != nil {
		t.Fatal(err)
	}
	body = append(body[:len(body)-1], []byte(`,"future_field":{"retained":true}}`)...)
	hasNode := false
	input := Envelope{
		HasNode: &hasNode, Target: &target,
		Call: Call{Kind: CallCommand, Request: &request}, RawData: body,
	}
	encoded, err := json.Marshal(input)
	if err != nil {
		t.Fatal(err)
	}
	var decoded Envelope
	if err := protocol.DecodeAs(protocol.PeerCommand, encoded, &decoded); err != nil {
		t.Fatal(err)
	}
	if decoded.Target == nil || *decoded.Target != target || decoded.HasNode == nil || *decoded.HasNode {
		t.Fatalf("route metadata changed: %#v", decoded)
	}
	reencoded, err := json.Marshal(decoded)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Contains(reencoded, []byte(`"future_field":{"retained":true}`)) {
		t.Fatalf("forwarded request lost a peer field: %s", reencoded)
	}
}

func TestEnvelopeRetainsUnknownRoutedCommand(t *testing.T) {
	target := protocol.NodeID{3}
	node, err := json.Marshal(target)
	if err != nil {
		t.Fatal(err)
	}
	encoded := fmt.Sprintf(`{"protocol":%d,"has_node":false,"target":%s,"kind":"command","data":{"version":%d,"id":%q,"target":%s,"command":{"kind":"future_knowledge_read","data":{"extra":true}}}}`,
		protocol.VERSION, node, protocol.VERSION, protocol.NewRequestID(), node)
	var decoded Envelope
	if err := protocol.DecodeAs(protocol.PeerCommand, []byte(encoded), &decoded); err != nil {
		t.Fatal(err)
	}
	if decoded.Undecoded == nil || decoded.Call.Request != nil {
		t.Fatalf("unknown request identity was not retained: %#v", decoded)
	}
	forward, err := json.Marshal(decoded)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Contains(forward, []byte(`"future_knowledge_read"`)) || !bytes.Contains(forward, []byte(`"extra":true`)) {
		t.Fatalf("unknown command was changed: %s", forward)
	}
}

func TestEnvelopeSeparatesScopedExecutionFromKnowledgeStorage(t *testing.T) {
	execution, storage := protocol.NodeID{2}, protocol.NodeID{3}
	scope := protocol.DefaultPluginContext()
	request := protocol.NewRequest(storage, protocol.Command{Kind: "list_knowledge_collections"})
	request.Plugin = &scope
	call := protocol.ScopedKnowledgeCall{Request: request}
	envelope := Envelope{Target: &execution, Call: Call{Kind: CallScopedKnowledge, ScopedKnowledge: &call}}
	encoded, err := json.Marshal(envelope)
	if err != nil {
		t.Fatal(err)
	}
	var decoded Envelope
	if err := protocol.DecodeAs(protocol.PeerCommand, encoded, &decoded); err != nil {
		t.Fatal(err)
	}
	if decoded.Target == nil || *decoded.Target != execution || decoded.Call.ScopedKnowledge == nil ||
		decoded.Call.ScopedKnowledge.Request.Target != storage || decoded.Call.ScopedKnowledge.Request.ID != request.ID {
		t.Fatalf("execution route changed the storage request: %+v", decoded)
	}
}

func TestRawFramePreservesPagesAcrossCompression(t *testing.T) {
	for _, compressed := range []bool{false, true} {
		t.Run(fmt.Sprint(compressed), func(t *testing.T) {
			pages := []json.RawMessage{
				json.RawMessage(`{"kind":"snapshot_start","data":{"delivery":"completed","node":"0300000000000000000000000000000000000000000000000000000000000000","cursor":2,"bytes":6}}`),
				json.RawMessage(`{"kind":"snapshot_page","data":{"index":0,"data":"first","future":true}}`),
				json.RawMessage(`{"kind":"snapshot_page","data":{"index":1,"data":"!"}}`),
			}
			var stream bytes.Buffer
			for _, page := range pages {
				if err := Write(&stream, page, compressed); err != nil {
					t.Fatal(err)
				}
			}
			reader := NewReader(compressed)
			for _, want := range pages {
				got, err := reader.Raw(&stream)
				if err != nil || !bytes.Equal(got, want) {
					t.Fatalf("raw frame changed: %s, %v", got, err)
				}
			}
			if stream.Len() != 0 {
				t.Fatal("raw reader combined or left frames behind")
			}
		})
	}
}
