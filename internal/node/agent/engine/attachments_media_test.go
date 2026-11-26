package engine

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"image"
	"image/color"
	"image/gif"
	"image/png"
	"math/rand/v2"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/office"
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
			if strings.Contains(joined, "read_pdf") || strings.Contains(joined, "read_office") ||
				strings.Contains(joined, "no document tool is available") != (test.wants["application/pdf"] == nil) {
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

func TestPDFAttachmentPrefersDocumentToolWithinItsLimit(t *testing.T) {
	t.Parallel()
	pdf := make([]byte, office.Limit+1)
	copy(pdf, "%PDF-1.4\n")
	smallPDF, largePDF := pdf[:inlinePDFMaxBytes], pdf[:inlinePDFMaxBytes+1]
	smallID, largeID, disguisedID := protocol.NewAttachmentID(), protocol.NewAttachmentID(), protocol.NewAttachmentID()
	atLimitID, aboveLimitID := protocol.NewAttachmentID(), protocol.NewAttachmentID()

	specs := map[protocol.AttachmentID]struct {
		name     string
		declared string
		data     []byte
	}{
		smallID:      {"small.pdf", "application/pdf", smallPDF},
		largeID:      {"manual.pdf", "application/pdf", largePDF},
		disguisedID:  {"manual.bin", "application/octet-stream", largePDF},
		atLimitID:    {"at-limit.pdf", "application/pdf", pdf[:office.Limit]},
		aboveLimitID: {"above-limit.pdf", "application/pdf", pdf},
	}
	readCounts := map[protocol.AttachmentID]int{}
	runtime := &Runtime{
		attachment: func(_ context.Context, _ protocol.TurnID, id protocol.AttachmentID) (protocol.Attachment, bool, error) {
			item := specs[id]
			return protocol.Attachment{ID: id, Spec: protocol.AttachmentSpec{Name: item.name, MediaType: item.declared, Size: uint64(len(item.data))}}, true, nil
		},
		attachmentBytes: func(_ context.Context, attachment protocol.Attachment) ([]byte, error) {
			readCounts[attachment.ID]++
			return specs[attachment.ID].data, nil
		},
	}

	makeRecords := func(ids ...protocol.AttachmentID) []canonical.Record {
		blocks := []canonical.Block{canonical.Text(0, "Inspect these PDFs")}
		for _, id := range ids {
			mime := specs[id].declared
			blocks = append(blocks, canonical.Block{Index: uint64(len(blocks)), Kind: "file", Data: &canonical.FileContent{
				File: canonical.FileRef{URI: attachmentPrefix + id.String(), Mime: &mime},
			}})
		}
		return []canonical.Record{{ID: "pdf-turn", Kind: "message", Data: canonical.NewMessage(canonical.User, blocks...)}}
	}

	provider := protocol.DefaultConversationProvider()
	provider.Api, provider.Authentication = protocol.ConversationModelApiResponses, protocol.AuthenticationApiKey
	provider.Models = []protocol.ConversationModel{
		{ID: "with-tools", Tools: true, Inputs: []string{"text", "pdf"}},
		{ID: "no-tools", Inputs: []string{"text", "pdf"}},
		{ID: "text-with-tools", Tools: true, Inputs: []string{"text"}},
		{ID: "text-no-tools", Inputs: []string{"text"}},
	}

	t.Run("read_pdf preferred for declared and detected PDFs just above the inline limit", func(t *testing.T) {
		clear(readCounts)
		x := &execution{
			ctx:   t.Context(),
			inv:   &store.AgentInvocation{Provider: &provider},
			tools: []Tool{{Definition: fantasy.FunctionTool{Name: "read_pdf"}, Deferred: true}},
		}
		x.inv.Turn.Config.Model = "with-tools"
		records := makeRecords(smallID, largeID, disguisedID)
		projected, assets, err := runtime.hydrateAttachments(x, records)
		if err != nil {
			t.Fatal(err)
		}
		binding := model.Binding{Provider: provider, Config: x.inv.Turn.Config}
		call, err := model.BuildProjectedCall(binding, model.Request{Records: projected, Assets: assets})
		if err != nil {
			t.Fatal(err)
		}
		var fileParts []fantasy.FilePart
		var notes []string
		for _, part := range call.Prompt[0].Content {
			switch value := part.(type) {
			case fantasy.FilePart:
				fileParts = append(fileParts, value)
			case fantasy.TextPart:
				notes = append(notes, value.Text)
			}
		}
		if len(fileParts) != 1 || !bytes.Equal(fileParts[0].Data, smallPDF) {
			t.Fatalf("expected only small PDF inline, got %d file parts", len(fileParts))
		}
		if readCounts[largeID] != 0 {
			t.Fatalf("declared large PDF was read from disk %d times, want 0", readCounts[largeID])
		}
		if readCounts[disguisedID] != 1 || x.attachments[disguisedID].data != nil {
			t.Fatalf("disguised large PDF reads=%d retainedData=%t", readCounts[disguisedID], x.attachments[disguisedID].data != nil)
		}
		joined := strings.Join(notes, "\n")
		for _, id := range []protocol.AttachmentID{largeID, disguisedID} {
			want := `exceeds the 5 MiB inline limit. Use read_pdf with source {"kind":"attachment","value":"` + id.String() + `"}`
			if !strings.Contains(joined, want) || !strings.Contains(joined, `select:read_pdf`) {
				t.Errorf("missing read_pdf reference note for %s: %q", id, joined)
			}
		}
		// Second step must not re-read either large PDF.
		if _, _, err := runtime.hydrateAttachments(x, records); err != nil {
			t.Fatal(err)
		}
		if readCounts[largeID] != 0 || readCounts[disguisedID] != 1 {
			t.Fatalf("second step re-read large PDFs: large=%d disguised=%d", readCounts[largeID], readCounts[disguisedID])
		}
	})

	for _, test := range []struct {
		name, model, tool string
		id                protocol.AttachmentID
		wantInline        bool
		wantNote          string
	}{
		{"tool at its upper limit", "with-tools", "read_office", atLimitID, false, "5 MiB inline limit. Use read_office"},
		{"native PDF above tool limit", "with-tools", "read_pdf", aboveLimitID, true, ""},
		{"native PDF without a captured tool", "with-tools", "", largeID, true, ""},
		{"native PDF with a model that cannot call tools", "no-tools", "read_pdf", largeID, true, ""},
		{"only the tool can read a PDF", "text-with-tools", "read_pdf", smallID, false, "Use read_pdf with source"},
		{"above tool limit without native PDF", "text-with-tools", "read_pdf", aboveLimitID, false, "document tools' attachment size limit"},
		{"neither PDF input nor tool calls", "text-no-tools", "read_pdf", largeID, false, "no document tool is available"},
	} {
		t.Run(test.name, func(t *testing.T) {
			clear(readCounts)
			x := &execution{ctx: t.Context(), inv: &store.AgentInvocation{Provider: &provider}}
			x.inv.Turn.Config.Model = test.model
			if test.tool != "" {
				x.tools = []Tool{{Definition: fantasy.FunctionTool{Name: test.tool}, Deferred: true}}
			}
			projected, _, err := runtime.hydrateAttachments(x, makeRecords(test.id))
			if err != nil {
				t.Fatal(err)
			}
			blocks := projected[0].Message().Content
			note := blocks[len(blocks)-1].Data.(*canonical.TextContent).Text
			if !strings.Contains(note, test.id.String()) || !strings.Contains(note, test.wantNote) {
				t.Fatalf("unexpected reference note: %q", note)
			}
			wantToolHint := strings.Contains(test.wantNote, "Use ")
			if (strings.Contains(note, "read_pdf") || strings.Contains(note, "read_office")) != wantToolHint ||
				strings.Contains(note, "select:") != wantToolHint {
				t.Fatalf("incorrect tool guidance: %q", note)
			}
			if !test.wantInline {
				if len(blocks) != 2 || blocks[1].Kind != "text" {
					t.Fatalf("expected a reference without native content, got %d blocks", len(blocks))
				}
				if test.id == atLimitID && readCounts[test.id] != 0 {
					t.Fatal("PDF at the tool limit should not be loaded for native input")
				}
				return
			}
			if len(blocks) != 3 {
				t.Fatalf("expected native PDF content and a reference, got %d blocks", len(blocks))
			}
			file, ok := blocks[1].Data.(*canonical.FileContent)
			if !ok || file.File.Mime == nil || *file.File.Mime != "application/pdf" ||
				len(file.File.URI) != base64.StdEncoding.EncodedLen(len(specs[test.id].data)) {
				t.Fatal("projected file does not carry the complete PDF")
			}
		})
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

func TestGrowingImageReencodeUsesPreparedSizeAndMIME(t *testing.T) {
	t.Parallel()
	palette := color.Palette{color.Black, color.White}
	first := image.NewPaletted(image.Rect(0, 0, 1, 1), palette)
	second := image.NewPaletted(first.Bounds(), palette)
	second.SetColorIndex(0, 0, 1)
	var raw bytes.Buffer
	animation := &gif.GIF{
		Image: []*image.Paletted{first, second}, Delay: []int{0, 0}, LoopCount: -1,
		Config: image.Config{ColorModel: palette, Width: 1, Height: 1},
	}
	if err := gif.EncodeAll(&raw, animation); err != nil {
		t.Fatal(err)
	}
	_, sent, encoded, err := files.PrepareImage(raw.Bytes())
	if err != nil || sent == nil || sent.MimeType != "image/png" || len(encoded) <= raw.Len() {
		t.Fatalf("fixture must grow when re-encoded as PNG: source=%d prepared=%d sent=%+v err=%v", raw.Len(), len(encoded), sent, err)
	}

	const paddingSize = 1 << 20
	padding := make([]byte, paddingSize)
	copy(padding, smallAttachmentPNG(t))
	imageID, paddingID, lastPaddingID := protocol.NewAttachmentID(), protocol.NewAttachmentID(), protocol.NewAttachmentID()
	data := map[protocol.AttachmentID][]byte{imageID: raw.Bytes(), paddingID: padding, lastPaddingID: padding[:paddingSize-raw.Len()]}
	mimes := map[protocol.AttachmentID]string{imageID: "image/gif", paddingID: "image/png", lastPaddingID: "image/png"}
	imageReads := 0
	runtime := &Runtime{
		attachment: func(_ context.Context, _ protocol.TurnID, id protocol.AttachmentID) (protocol.Attachment, bool, error) {
			return protocol.Attachment{ID: id, Spec: protocol.AttachmentSpec{Name: "image", MediaType: mimes[id], Size: uint64(len(data[id]))}}, true, nil
		},
		attachmentBytes: func(_ context.Context, attachment protocol.Attachment) ([]byte, error) {
			if attachment.ID == imageID {
				imageReads++
			}
			return data[attachment.ID], nil
		},
	}
	block := func(id protocol.AttachmentID) canonical.Block {
		mime := mimes[id]
		return canonical.Block{Kind: "file", Data: &canonical.FileContent{File: canonical.FileRef{URI: attachmentPrefix + id.String(), Mime: &mime}}}
	}
	provider := protocol.DefaultConversationProvider()
	provider.Api, provider.Authentication = protocol.ConversationModelApiResponses, protocol.AuthenticationApiKey
	provider.Models = []protocol.ConversationModel{{ID: "vision", Vision: true, Inputs: []string{"text", "image"}}}
	x := &execution{ctx: t.Context(), inv: &store.AgentInvocation{Provider: &provider}}
	x.inv.Turn.Config.Model = "vision"
	// The final two sources fit together, but the PNG output and the short padding do not.
	const paddingCount = inlineAttachmentBudget/paddingSize - 1
	hydrate := func(first, second protocol.AttachmentID) ([]canonical.Block, map[string][]byte) {
		t.Helper()
		blocks := make([]canonical.Block, 0, paddingCount+2)
		for range paddingCount {
			blocks = append(blocks, block(paddingID))
		}
		blocks = append(blocks, block(first), block(second))
		records := []canonical.Record{{ID: "image", Kind: "message", Data: canonical.NewMessage(canonical.User, blocks...)}}
		projected, assets, err := runtime.hydrateAttachments(x, records)
		if err != nil {
			t.Fatal(err)
		}
		content := projected[0].Message().Content
		if len(assets) != 2 || len(content) != 2*(paddingCount+1)+1 {
			t.Fatalf("got %d assets and %d blocks; expected the final attachment to be reference-only", len(assets), len(content))
		}
		note := content[len(content)-1].Data.(*canonical.TextContent).Text
		if !strings.Contains(note, second.String()) || !strings.Contains(note, "exceeds the remaining inline attachment budget") {
			t.Fatalf("expected a budget note for the final attachment, got %q", note)
		}
		return content, assets
	}

	content, assets := hydrate(imageID, lastPaddingID)
	asset, ok := content[2*paddingCount].Data.(*canonical.AssetContent)
	if !ok || asset.Asset.Mime != sent.MimeType || asset.Asset.Size != uint64(len(encoded)) || !bytes.Equal(assets[asset.Asset.Digest], encoded) {
		t.Fatalf("expected the prepared PNG and its actual length, got %+v", asset)
	}
	// Reverse the order: test the budget check both with and without cached prepared bytes.
	for step := range 2 {
		hydrate(lastPaddingID, imageID)
		if imageReads != step+1 || x.attachments[imageID].data != nil {
			t.Fatalf("step %d: image reads=%d, retained bytes=%d", step, imageReads, len(x.attachments[imageID].data))
		}
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
