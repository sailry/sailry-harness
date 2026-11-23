package model

import (
	"encoding/base64"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestChatAttachmentProviderWire(t *testing.T) {
	wav := []byte("RIFF\x28\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00\x40\x1f\x00\x00\x80\x3e\x00\x00\x02\x00\x10\x00data\x04\x00\x00\x00\x00\x00\x02\x01")
	video := []byte("\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00isommp42\x00\x00\x00\x0cmdat\x00\x01\x02\x03")
	pdf := []byte("%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n%%EOF")
	encoded := map[string]string{
		"audio/wav":       base64.StdEncoding.EncodeToString(wav),
		"video/mp4":       base64.StdEncoding.EncodeToString(video),
		"application/pdf": base64.StdEncoding.EncodeToString(pdf),
	}
	for _, test := range []struct {
		api      protocol.ConversationModelApi
		response string
		path     string
		files    []string
		want     map[string]any
	}{
		{
			api: protocol.ConversationModelApiGemini, response: "google-text.sse", path: "/v1beta/models/fixture:streamGenerateContent?alt=sse",
			files: []string{"video/mp4", "audio/wav", "application/pdf"},
			want: map[string]any{
				"contents.0.parts.1.inlineData.mimeType": "video/mp4", "contents.0.parts.1.inlineData.data": encoded["video/mp4"],
				"contents.0.parts.2.inlineData.mimeType": "audio/wav", "contents.0.parts.2.inlineData.data": encoded["audio/wav"],
				"contents.0.parts.3.inlineData.mimeType": "application/pdf", "contents.0.parts.3.inlineData.data": encoded["application/pdf"],
			},
		},
		{
			api: protocol.ConversationModelApiChatCompletions, response: "chat-text.sse", path: "/v1/chat/completions",
			files: []string{"audio/wav", "application/pdf"},
			want: map[string]any{
				"messages.0.content.1.type": "input_audio", "messages.0.content.1.input_audio.format": "wav",
				"messages.0.content.1.input_audio.data": encoded["audio/wav"],
				"messages.0.content.2.type":             "file", "messages.0.content.2.file.filename": "attachment.pdf",
				"messages.0.content.2.file.file_data": "data:application/pdf;base64," + encoded["application/pdf"],
			},
		},
		{
			api: protocol.ConversationModelApiResponses, response: "responses-text.sse", path: "/v1/responses",
			files: []string{"application/pdf"},
			want: map[string]any{
				"input.1.content.0.type": "input_file", "input.1.content.0.filename": "attachment.pdf",
				"input.1.content.0.file_data": "data:application/pdf;base64," + encoded["application/pdf"],
			},
		},
		{
			api: protocol.ConversationModelApiAnthropic, response: "anthropic-text.sse", path: "/v1/messages",
			files: []string{"application/pdf"},
			want: map[string]any{
				"messages.0.content.1.type": "document", "messages.0.content.1.source.type": "base64",
				"messages.0.content.1.source.media_type": "application/pdf", "messages.0.content.1.source.data": encoded["application/pdf"],
			},
		},
	} {
		t.Run(string(test.api), func(t *testing.T) {
			server := serverFixture(t, string(fixture(t, test.response)))
			provider := protocol.DefaultConversationProvider()
			provider.Api, provider.Authentication, provider.Endpoint = test.api, protocol.AuthenticationApiKey, server.URL
			provider.Models = []protocol.ConversationModel{{ID: "fixture", Context: 128000, Output: 512, Inputs: []string{"text", "audio", "video", "pdf"}}}
			key := protocol.NewSecret("fixture-key")
			binding := Binding{Provider: provider, Config: protocol.SessionConfig{Model: "fixture"}, Secret: &key}
			language, err := New(t.Context(), binding)
			if err != nil {
				t.Fatal(err)
			}
			blocks := []canonical.Block{canonical.Text(0, "Read the attachment")}
			for _, mime := range test.files {
				blocks = append(blocks, canonical.Block{Index: uint64(len(blocks)), Kind: "file", Data: &canonical.FileContent{
					File: canonical.FileRef{URI: encoded[mime], Mime: &mime},
				}})
			}
			records := []canonical.Record{{ID: "resolved-attachments", Kind: "message", Data: canonical.NewMessage(canonical.User, blocks...)}}
			call, err := BuildProjectedCall(binding, Request{Records: records})
			if err != nil {
				t.Fatal(err)
			}
			collect(t, language, call)
			requests := server.captured()
			if len(requests) != 1 || requests[0].Path != test.path {
				t.Fatalf("expected one request to %s, got %#v", test.path, requests)
			}
			for path, want := range test.want {
				checkValue(t, requests[0].Body, path, want)
			}
		})
	}
}
