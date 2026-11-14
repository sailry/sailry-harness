package engine

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestImageHistoryModelSwitch(t *testing.T) {
	for _, current := range []bool{false, true} {
		for _, textAdapter := range []bool{false, true} {
			t.Run(fmt.Sprintf("current %v text adapter %v", current, textAdapter), func(t *testing.T) {
				id, mime := protocol.NewAttachmentID(), "image/png"
				pixels := []byte("\x89PNG\r\n\x1a\n" + strings.Repeat("pixel", 64))
				message := canonical.NewMessage(canonical.User,
					canonical.Text(0, "keep this instruction"),
					canonical.Block{Index: 1, Kind: "image", Data: &canonical.AssetContent{Asset: canonical.AssetRef{Digest: "original", Mime: mime, Size: uint64(len(pixels))}}},
					canonical.Block{Index: 2, Kind: "file", Data: &canonical.FileContent{File: canonical.FileRef{URI: attachmentPrefix + id.String(), Mime: &mime}}})
				records := []canonical.Record{{ID: "image-input", Kind: "message", Data: message}}
				before, _ := json.Marshal(records)
				provider := protocol.DefaultConversationProvider()
				provider.Api, provider.Authentication = protocol.ConversationModelApiAnthropic, protocol.AuthenticationApiKey
				provider.Models = []protocol.ConversationModel{{ID: "vision", Vision: true}, {ID: "text"}}
				if textAdapter {
					provider.Api = protocol.ConversationModelApiPlugin
					selection := protocol.DefaultConversationPresetsBinding()
					selection.Adapter = &protocol.PluginModelAdapterBinding{Adapter: "text", Capabilities: protocol.PluginModelAdapterCapabilities{Vision: false}}
					binding := &selection
					provider.Binding = &binding
				}
				r := &Runtime{
					attachment: func(context.Context, protocol.TurnID, protocol.AttachmentID) (protocol.Attachment, bool, error) {
						return protocol.Attachment{ID: id, Spec: protocol.AttachmentSpec{Name: "picture.png", MediaType: mime, Size: uint64(len(pixels))}}, current, nil
					},
					attachmentBytes: func(context.Context, protocol.Attachment) ([]byte, error) { return pixels, nil },
				}
				x := &execution{ctx: t.Context(), inv: &store.AgentInvocation{Provider: &provider}}
				for _, selected := range []string{"vision", "text", "vision"} {
					x.inv.Turn.Config.Model = selected
					request, assets, err := r.hydrateAttachments(x, records)
					if err != nil {
						t.Fatal(err)
					}
					images, references := 0, 0
					for _, block := range request[0].Message().Content {
						if block.Kind == "image" {
							images++
						}
						if text, ok := block.Data.(*canonical.TextContent); ok && strings.Contains(text.Text, id.String()) {
							references++
						}
					}
					wantImages, wantAssets := 0, 0
					if selected == "vision" && !textAdapter {
						wantImages, wantAssets = 2, 1
					}
					if images != wantImages || len(assets) != wantAssets || references != 1 || request[0].Message().Content[0].Data.(*canonical.TextContent).Text != "keep this instruction" {
						t.Fatalf("%s request = %d images, %d assets, %d references", selected, images, len(assets), references)
					}
					after, _ := json.Marshal(records)
					if string(after) != string(before) {
						t.Fatal("model switch changed canonical input")
					}
				}
			})
		}
	}
}
