package model

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginToolImageCapabilities(t *testing.T) {
	pixels := []byte("\x89PNG\r\n\x1a\nfixture")
	encoded := base64.StdEncoding.EncodeToString(pixels)
	resource := base64.StdEncoding.EncodeToString([]byte("non-image resource"))
	for name, response := range map[string]canonical.ToolResponse{
		"MCP images": {ID: "call", Name: "capture", Format: canonical.MCP, Status: canonical.ToolSuccess,
			Result: canonical.Payload{Value: json.RawMessage(`{"content":[{"type":"text","text":"captured"},{"type":"image","data":"` + encoded + `","mimeType":"image/png"},{"type":"resource","resource":{"uri":"fixture:image","mimeType":"image/png","blob":"` + encoded + `"}},{"type":"resource","resource":{"uri":"fixture:data","mimeType":"application/octet-stream","blob":"` + resource + `"}}],"structuredContent":{"retained":true}}`)}},
		"lifted image": {ID: "call", Name: "capture", Format: canonical.JSON, Status: canonical.ToolSuccess,
			Result:      canonical.Payload{Value: json.RawMessage(`{"retained":true}`)},
			Attachments: []canonical.Attachment{{Asset: canonical.AssetRef{Digest: "pixels", Mime: "image/png", Size: uint64(len(pixels))}}}},
	} {
		t.Run(name, func(t *testing.T) {
			for _, adapterVision := range []bool{true, false} {
				t.Run(fmt.Sprintf("adapter vision %v", adapterVision), func(t *testing.T) {
					model, binding, _ := pluginFixture(t, "", nil)
					(**binding.Provider.Binding).Adapter.Capabilities.Vision = adapterVision
					model.capture.Declaration.Capabilities.Vision = adapterVision
					entry := protocol.DefaultConversationModel()
					entry.ID = binding.Config.Model
					binding.Provider.Models = []protocol.ConversationModel{entry}
					turn, created := protocol.NewTurnID(), time.Unix(100, 0)
					request := Request{Records: []canonical.Record{
						canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: &canonical.ToolRequest{ID: "call", Name: "capture", Arguments: json.RawMessage(`{}`)}}), created),
						canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Block{Kind: "tool_response", Data: &response}), created),
					}}
					before, _ := json.Marshal(request.Records)
					// Switching back must recover the same stored images; filtering is
					// a per-request projection, never an edit to the tool result.
					for _, vision := range []bool{true, false, true} {
						binding.Provider.Models[0].Vision = vision
						send := vision && adapterVision
						request.Assets = nil
						if send {
							request.Assets = map[string][]byte{"pixels": pixels}
						}
						call, err := BuildCall(binding, request)
						if err != nil {
							t.Fatal(err)
						}
						wire, err := model.request(call)
						if err != nil {
							t.Fatal(err)
						}
						if len(wire.Messages) != 2 {
							t.Fatalf("model vision %v received %d messages", vision, len(wire.Messages))
						}
						result := wire.Messages[1].Data.(*protocol.PluginModelAdapterWireMessageToolPayload).Parts[0].Data.(*protocol.PluginModelAdapterWirePartJsonPayload).Value
						if strings.Contains(string(result), encoded) != send {
							t.Fatalf("model vision %v received unexpected image payload: %s", vision, result)
						}
						var body map[string]any
						if json.Unmarshal(result, &body) != nil || valueAt(body, "structuredContent.retained") != true {
							t.Fatalf("structured result changed: %s", result)
						}
						if response.Format == canonical.MCP {
							want := 2
							if send {
								want = 4
							}
							if len(wireArray(body["content"])) != want || !strings.Contains(string(result), resource) || valueAt(body, "content.0.text") != "captured" {
								t.Fatalf("non-image content changed: %s", result)
							}
						}
						after, _ := json.Marshal(request.Records)
						if string(after) != string(before) {
							t.Fatal("image projection changed canonical history")
						}
					}
				})
			}
		})
	}
}
