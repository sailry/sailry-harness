package engine

import (
	"bytes"
	"context"
	"encoding/json"
	"image"
	"image/color"
	"image/png"
	"math/rand/v2"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestChatAttachmentInputs(t *testing.T) {
	t.Parallel()
	wav := []byte("RIFF\x28\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00\x40\x1f\x00\x00\x80\x3e\x00\x00\x02\x00\x10\x00data\x04\x00\x00\x00\x00\x00\x02\x01")
	video := []byte("\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00isommp42\x00\x00\x00\x0cmdat\x00\x01\x02\x03")
	pdf := []byte("%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n%%EOF")
	spoof := []byte("\x00\x01\x00\x02not an MP4")
	text := []byte("Some text for the model")
	type fixture struct {
		id       protocol.AttachmentID
		name     string
		declared string
		data     []byte
	}
	files := []fixture{
		{protocol.NewAttachmentID(), "clip.bin", "application/octet-stream", video},
		{protocol.NewAttachmentID(), "sound.bin", "application/octet-stream", wav},
		{protocol.NewAttachmentID(), "report.pdf", "application/pdf", pdf},
		{protocol.NewAttachmentID(), "bogus.mp4", "video/mp4", spoof},
		{protocol.NewAttachmentID(), "note.txt", "text/plain", text},
	}
	content := []canonical.Block{canonical.Text(0, "Read the attachments")}
	byID := map[protocol.AttachmentID]fixture{}
	for _, file := range files {
		byID[file.id] = file
		content = append(content, canonical.Block{Index: uint64(len(content)), Kind: "file", Data: &canonical.FileContent{
			File: canonical.FileRef{URI: attachmentPrefix + file.id.String(), Mime: &file.declared},
		}})
	}
	records := []canonical.Record{{ID: "attachments", Kind: "message", Data: canonical.NewMessage(canonical.User, content...)}}
	before, err := json.Marshal(records)
	if err != nil {
		t.Fatal(err)
	}
	readCounts := map[protocol.AttachmentID]int{}
	runtime := &Runtime{
		attachment: func(_ context.Context, _ protocol.TurnID, id protocol.AttachmentID) (protocol.Attachment, bool, error) {
			file := byID[id]
			return protocol.Attachment{ID: id, Spec: protocol.AttachmentSpec{Name: file.name, MediaType: file.declared, Size: uint64(len(file.data))}}, true, nil
		},
		attachmentBytes: func(_ context.Context, attachment protocol.Attachment) ([]byte, error) {
			readCounts[attachment.ID]++
			return byID[attachment.ID].data, nil
		},
	}
	allInputs := []string{"text", "audio", "video", "pdf"}
	for _, test := range []struct {
		name   string
		api    protocol.ConversationModelApi
		inputs []string
		wants  map[string][]byte
	}{
		{"Gemini", protocol.ConversationModelApiGemini, allInputs, map[string][]byte{"video/mp4": video, "audio/wav": wav, "application/pdf": pdf}},
		{"Gemini, video only", protocol.ConversationModelApiGemini, []string{"text", "video"}, map[string][]byte{"video/mp4": video}},
		{"Gemini, legacy", protocol.ConversationModelApiGemini, nil, map[string][]byte{"application/pdf": pdf}},
		{"Gemini, explicit empty", protocol.ConversationModelApiGemini, []string{}, nil},
		{"Chat Completions", protocol.ConversationModelApiChatCompletions, allInputs, map[string][]byte{"audio/wav": wav, "application/pdf": pdf}},
		{"Responses", protocol.ConversationModelApiResponses, allInputs, map[string][]byte{"application/pdf": pdf}},
		{"Anthropic, legacy", protocol.ConversationModelApiAnthropic, nil, map[string][]byte{"application/pdf": pdf}},
		{"Azure OpenAI, unknown wire support", protocol.ConversationModelApiAzureOpenAi, allInputs, nil},
		{"OpenCode, unknown downstream", protocol.ConversationModelApiOpenCodeZen, allInputs, nil},
	} {
		t.Run(test.name, func(t *testing.T) {
			clear(readCounts)
			provider := protocol.DefaultConversationProvider()
			provider.Api, provider.Authentication = test.api, protocol.AuthenticationApiKey
			provider.Models = []protocol.ConversationModel{{ID: "other", Inputs: allInputs}, {ID: "captured", Inputs: test.inputs}}
			x := &execution{ctx: t.Context(), inv: &store.AgentInvocation{Provider: &provider}}
			x.inv.Turn.Config.Model = "captured"
			projected, assets, err := runtime.hydrateAttachments(x, records)
			if err != nil {
				t.Fatal(err)
			}
			binding := model.Binding{Provider: provider, Config: x.inv.Turn.Config}
			call, err := model.BuildProjectedCall(binding, model.Request{Records: projected, Assets: assets})
			if err != nil || len(call.Prompt) != 1 {
				t.Fatalf("attachment prompt has %d messages: %v", len(call.Prompt), err)
			}
			actual := map[string][]byte{}
			var notes []string
			for _, block := range call.Prompt[0].Content {
				switch value := block.(type) {
				case fantasy.FilePart:
					if _, duplicate := actual[value.MediaType]; duplicate {
						t.Fatalf("duplicate file MIME %q", value.MediaType)
					}
					actual[value.MediaType] = value.Data
				case fantasy.TextPart:
					notes = append(notes, value.Text)
				}
			}
			if len(actual) != len(test.wants) {
				t.Fatalf("got %d file parts, want %d", len(actual), len(test.wants))
			}
			for mime, data := range test.wants {
				if !bytes.Equal(actual[mime], data) {
					t.Errorf("file %s did not carry the detected bytes", mime)
				}
			}
			joined := strings.Join(notes, "\n")
			for _, file := range files {
				if !strings.Contains(joined, `"id":"`+file.id.String()+`"`) {
					t.Errorf("no attachment reference for %s", file.name)
				}
			}
			const notDirect = "cannot directly read this attachment"
			omitted := 1 // bogus.mp4, even when the model accepts video
			for _, mime := range []string{"audio/wav", "video/mp4"} {
				if test.wants[mime] == nil {
					omitted++
				}
			}
			if strings.Count(joined, notDirect) != omitted || strings.Contains(joined, "not an MP4") {
				t.Errorf("unsupported media did not remain reference-only: %q", joined)
			}
			pdfNote := `Use read_office with source {"kind":"attachment","value":"` + files[2].id.String() + `"}`
			if strings.Contains(joined, pdfNote) != (test.wants["application/pdf"] == nil) {
				t.Errorf("wrong PDF fallback: %q", joined)
			}
			if !strings.Contains(joined, "Attachment: note.txt\n"+string(text)) {
				t.Error("small text attachment was not sent as text")
			}
			for _, rejected := range []struct {
				id   protocol.AttachmentID
				mime string
			}{{files[0].id, "video/mp4"}, {files[1].id, "audio/wav"}, {files[2].id, "application/pdf"}, {files[3].id, ""}} {
				if test.wants[rejected.mime] == nil && x.attachments[rejected.id].data != nil {
					t.Errorf("reference-only attachment %s retained its contents", rejected.id)
				}
			}
			if _, _, err := runtime.hydrateAttachments(x, records); err != nil {
				t.Fatal(err)
			}
			for _, file := range files {
				if readCounts[file.id] != 1 {
					t.Errorf("%s read %d times across two requests", file.name, readCounts[file.id])
				}
			}
			if _, _, err := runtime.hydrateAttachments(x, nil); err != nil {
				t.Fatal(err)
			}
			if len(x.attachments) != 0 {
				t.Errorf("attachments outside request history still cached: %d", len(x.attachments))
			}
		})
	}
	after, err := json.Marshal(records)
	if err != nil || !bytes.Equal(before, after) {
		t.Fatalf("attachment request changed canonical history: %v", err)
	}
}

func TestUnpreparableImageAttachmentRemainsReferenceOnly(t *testing.T) {
	t.Parallel()
	// The PNG signature is detectable, but there is no image header to decode.
	raw := []byte("\x89PNG\r\n\x1a\n")
	id, mime := protocol.NewAttachmentID(), "image/png"
	reads := 0
	runtime := &Runtime{
		attachment: func(_ context.Context, _ protocol.TurnID, got protocol.AttachmentID) (protocol.Attachment, bool, error) {
			return protocol.Attachment{ID: got, Spec: protocol.AttachmentSpec{Name: "broken.png", MediaType: mime, Size: uint64(len(raw))}}, true, nil
		},
		attachmentBytes: func(context.Context, protocol.Attachment) ([]byte, error) {
			reads++
			return raw, nil
		},
	}
	uri := attachmentPrefix + id.String()
	file := &canonical.FileContent{File: canonical.FileRef{URI: uri, Mime: &mime}}
	records := []canonical.Record{{ID: "image", Kind: "message", Data: canonical.NewMessage(canonical.User,
		canonical.Block{Kind: "file", Data: file},
	)}}
	provider := protocol.DefaultConversationProvider()
	provider.Api, provider.Authentication = protocol.ConversationModelApiResponses, protocol.AuthenticationApiKey
	provider.Models = []protocol.ConversationModel{{ID: "vision", Vision: true, Inputs: []string{"text", "image"}}}
	x := &execution{ctx: t.Context(), inv: &store.AgentInvocation{Provider: &provider}}
	x.inv.Turn.Config.Model = "vision"
	for step := range 2 {
		projected, assets, err := runtime.hydrateAttachments(x, records)
		if err != nil || len(assets) != 0 {
			t.Fatalf("step %d: expected no inline image, got %d assets, err=%v", step, len(assets), err)
		}
		blocks := projected[0].Message().Content
		if len(blocks) != 1 || blocks[0].Kind != "text" {
			t.Fatalf("step %d: expected just the attachment reference, got %+v", step, blocks)
		}
		note := blocks[0].Data.(*canonical.TextContent).Text
		if !strings.Contains(note, `"id":"`+id.String()+`"`) || !strings.Contains(note, "could not be prepared") {
			t.Fatalf("step %d: missing reference or reason: %s", step, note)
		}
	}
	if reads != 1 || x.attachments[id].data != nil {
		t.Fatalf("unpreparable image was read %d times or its bytes were retained", reads)
	}
	if original := records[0].Message().Content[0]; original.Kind != "file" || original.Data.(*canonical.FileContent).File.URI != uri {
		t.Fatalf("canonical attachment was changed: %+v", original)
	}
}

func TestOversizedUserImageAttachmentIsPreparedBeforeModelRequest(t *testing.T) {
	t.Parallel()
	photo := image.NewNRGBA(image.Rect(0, 0, 2000, 1400))
	rng := rand.New(rand.NewPCG(11, 13))
	for y := 0; y < 1400; y++ {
		for x := 0; x < 2000; x++ {
			jitter := int(rng.IntN(9)) - 4
			photo.SetNRGBA(x, y, color.NRGBA{
				R: uint8(min(255, max(0, x*255/2000+jitter))),
				G: uint8(min(255, max(0, y*255/1400+jitter))),
				B: uint8(min(255, max(0, (x+y)*255/3400+jitter))),
				A: 255,
			})
		}
	}
	var raw bytes.Buffer
	if err := png.Encode(&raw, photo); err != nil {
		t.Fatal(err)
	}
	id := protocol.NewAttachmentID()
	declared := "image/png"
	reads := 0
	runtime := &Runtime{
		attachment: func(_ context.Context, _ protocol.TurnID, got protocol.AttachmentID) (protocol.Attachment, bool, error) {
			return protocol.Attachment{ID: got, Spec: protocol.AttachmentSpec{Name: "retina.png", MediaType: declared, Size: uint64(raw.Len())}}, true, nil
		},
		attachmentBytes: func(_ context.Context, _ protocol.Attachment) ([]byte, error) {
			reads++
			return raw.Bytes(), nil
		},
	}
	records := []canonical.Record{{ID: "img-turn", Kind: "message", Data: canonical.NewMessage(canonical.User,
		canonical.Text(0, "Inspect this screenshot"),
		canonical.Block{Index: 1, Kind: "file", Data: &canonical.FileContent{File: canonical.FileRef{URI: attachmentPrefix + id.String(), Mime: &declared}}},
	)}}
	provider := protocol.DefaultConversationProvider()
	provider.Api, provider.Authentication = protocol.ConversationModelApiResponses, protocol.AuthenticationApiKey
	provider.Models = []protocol.ConversationModel{{ID: "vision", Vision: true, Inputs: []string{"text", "image"}}}
	x := &execution{ctx: t.Context(), inv: &store.AgentInvocation{Provider: &provider}}
	x.inv.Turn.Config.Model = "vision"

	projected, assets, err := runtime.hydrateAttachments(x, records)
	if err != nil {
		t.Fatal(err)
	}
	if len(assets) != 1 {
		t.Fatalf("got %d image assets, want 1", len(assets))
	}
	var sentBytes []byte
	for _, data := range assets {
		sentBytes = data
	}
	var assetBlock *canonical.AssetContent
	for _, block := range projected[0].Message().Content {
		if value, ok := block.Data.(*canonical.AssetContent); ok {
			assetBlock = value
		}
	}
	if assetBlock == nil || assetBlock.Asset.Mime != "image/jpeg" || len(sentBytes) >= raw.Len()/2 {
		t.Fatalf("expected prepared compact JPEG for oversized user screenshot: block=%+v sent=%d raw=%d", assetBlock, len(sentBytes), raw.Len())
	}
	// Second step reuses cached prepared image without re-reading disk.
	if _, assets2, err := runtime.hydrateAttachments(x, records); err != nil || len(assets2) != 1 || reads != 1 {
		t.Fatalf("second step reads=%d assets=%d err=%v", reads, len(assets2), err)
	}
}
