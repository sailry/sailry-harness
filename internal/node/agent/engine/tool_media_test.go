package engine

import (
	"context"
	"database/sql"
	"encoding/base64"
	"encoding/json"
	"strconv"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestToolMedia runs a tool that returns a picture and reads the request the
// next step sends: the picture is media on its result, never text.
func TestToolMedia(t *testing.T) {
	pixels := []byte("\x89PNG\r\n\x1a\n" + strings.Repeat("pixel", 64))
	encoded := base64.StdEncoding.EncodeToString(pixels)
	// The attachment is what a tool stores for display only.
	shown := []byte("display only picture")
	var display string
	capture := objectTool("capture", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
		return ToolResult{Value: map[string]any{"kind": "browser", "data": map[string]any{"tab": 1, "inline_data": []any{map[string]any{"mime_type": "image/png", "data": encoded}}}},
			Attachments: []canonical.Attachment{{Asset: canonical.AssetRef{Digest: display, Mime: "image/png", Size: uint64(len(shown))}}}}
	})
	for _, vision := range []bool{true, false} {
		name := "text model"
		if vision {
			name = "vision model"
		}
		t.Run(name, func(t *testing.T) {
			m := &scriptedModel{scripts: []script{response(call("shot", "capture", `{}`), finish()), response(text("Done"), finish())}}
			f := newFixture(t, m, capture)
			err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
				var err error
				if display, err = store.PutBlob(f.ctx, db, shown); err != nil || !vision {
					return err
				}
				_, err = db.ExecContext(f.ctx, "UPDATE providers SET body=json_set(body,'$.provider.models[0].vision',json('true'))")
				return err
			})
			if err != nil {
				t.Fatal(err)
			}
			f.start()
			f.waitRun(f.submit("Look"), protocol.ConversationStatusCompleted)
			if m.count() != 2 {
				t.Fatalf("model requests = %d", m.count())
			}
			// The stored result holds a slot; its bytes are a blob.
			if f.readQuery("SELECT count(*) FROM agent_events WHERE instr(body,'"+encoded[:48]+"')>0") != 0 || f.readQuery("SELECT count(*) FROM agent_blobs WHERE length(bytes)="+strconv.Itoa(len(pixels))) != 1 {
				t.Error("tool media was stored inline")
			}
			var output string
			var files []fantasy.FilePart
			for _, message := range m.calls[1].Prompt {
				if message.Role != fantasy.MessageRoleTool {
					continue
				}
				for _, part := range message.Content {
					switch value := part.(type) {
					case fantasy.ToolResultPart:
						output = value.Output.(fantasy.ToolResultOutputContentText).Text
					case fantasy.FilePart:
						files = append(files, value)
					}
				}
			}
			if output == "" || strings.Contains(output, encoded[:32]) || strings.Contains(output, "inline_data") {
				t.Errorf("tool result text = %q", output)
			}
			if !vision {
				if len(files) != 0 || !strings.Contains(output, "(tool image omitted: model does not support images)") {
					t.Errorf("text model received %d files and %q", len(files), output)
				}
				return
			}
			if len(files) != 1 || files[0].MediaType != "image/png" || string(files[0].Data) != string(pixels) {
				t.Errorf("tool media = %d files", len(files))
			}
		})
	}
	t.Run("lift", func(t *testing.T) {
		slot := func(pointer, digest string) canonical.BinarySlot {
			return canonical.BinarySlot{Pointer: pointer, Asset: canonical.AssetRef{Digest: digest, Mime: "image/png", Size: 1}, Encoding: canonical.Base64}
		}
		for name, c := range map[string]struct {
			value, kept string
			slots       []canonical.BinarySlot
			media       []string
			remaining   int
		}{
			"typed output":       {`{"kind":"file_image","data":{"path":"a.png","inline_data":[{"mime_type":"image/png","data":null}]}}`, `{"data":{"path":"a.png"},"kind":"file_image"}`, []canonical.BinarySlot{slot("/data/inline_data/0/data", "a")}, []string{"a"}, 0},
			"result root":        {`{"text":"ok","inline_data":[{"mime_type":"image/png","data":null},{"mime_type":"image/png","data":null}],"other":{"mime":"image/png","data":null}}`, `{"other":{"data":null,"mime":"image/png"},"text":"ok"}`, []canonical.BinarySlot{slot("/inline_data/0/data", "a"), slot("/inline_data/1/data", "b"), slot("/other/data", "c")}, []string{"a", "b"}, 1},
			"user audience":      {`{"inline_data":[{"mime_type":"image/png","data":null,"annotations":{"audience":["user"]}},{"mime_type":"image/png","data":null,"annotations":{"audience":["user","assistant"]}}]}`, `{}`, []canonical.BinarySlot{slot("/inline_data/0/data", "a"), slot("/inline_data/1/data", "b")}, []string{"b"}, 0},
			"untyped data stays": {`{"kind":"plugin_value","data":{"inline_data":[{"mime_type":"image/png","data":null}]}}`, "", []canonical.BinarySlot{slot("/data/inline_data/0/data", "a")}, nil, 1},
			"unstored item":      {`{"inline_data":[{"mime_type":"image/png","data":null},{"mime_type":"image/png","data":"not base64"}]}`, "", []canonical.BinarySlot{slot("/inline_data/0/data", "a")}, nil, 1},
		} {
			t.Run(name, func(t *testing.T) {
				payload, media := liftToolMedia(canonical.Payload{Value: json.RawMessage(c.value), Assets: c.slots})
				digests := []string{}
				for _, attachment := range media {
					digests = append(digests, attachment.Asset.Digest)
				}
				if strings.Join(digests, ",") != strings.Join(c.media, ",") || len(payload.Assets) != c.remaining {
					t.Errorf("media = %v, remaining slots = %d", digests, len(payload.Assets))
				}
				if kept := c.kept; kept == "" && string(payload.Value) != c.value || kept != "" && string(payload.Value) != kept {
					t.Errorf("value = %s", payload.Value)
				}
			})
		}
	})
	// An attachment becomes content in the request alone, and a message
	// without one is not copied: this runs on every model step.
	t.Run("attachments change the request's copy only", func(t *testing.T) {
		id, mime := protocol.NewAttachmentID(), "text/plain"
		plain := &canonical.Message{Role: canonical.User, PartCount: 1, Content: []canonical.Block{canonical.Text(0, "plain")}}
		attached := &canonical.Message{Role: canonical.User, PartCount: 2, Content: []canonical.Block{
			canonical.Text(0, "see the note"),
			{Index: 1, Kind: "file", Data: &canonical.FileContent{File: canonical.FileRef{URI: attachmentPrefix + id.String(), Mime: &mime}}},
		}}
		records := []canonical.Record{{ID: "plain", Kind: "message", Data: plain}, {ID: "attached", Kind: "message", Data: attached}}
		r := &Runtime{
			attachment: func(context.Context, protocol.TurnID, protocol.AttachmentID) (protocol.Attachment, bool, error) {
				return protocol.Attachment{ID: id, Spec: protocol.AttachmentSpec{Name: "note.txt", MediaType: mime, Size: 4}}, true, nil
			},
			attachmentBytes: func(context.Context, protocol.Attachment) ([]byte, error) { return []byte("note"), nil },
		}
		x := &execution{ctx: t.Context(), inv: &store.AgentInvocation{Provider: &protocol.ConversationProvider{}}}
		request, assets, err := r.hydrateAttachments(x, records)
		if err != nil {
			t.Fatal(err)
		}
		if len(request) != 2 || len(assets) != 0 {
			t.Fatalf("request = %d records, %d assets", len(request), len(assets))
		}
		if records[1].Data != any(attached) || attached.PartCount != 2 || len(attached.Content) != 2 || attached.Content[1].Kind != "file" {
			t.Fatalf("history lost its attachment reference: %+v", attached)
		}
		hydrated := request[1].Message()
		if hydrated == nil || hydrated == attached || hydrated.PartCount != 3 || len(hydrated.Content) != 3 {
			t.Fatalf("request message = %+v", hydrated)
		}
		if text, ok := hydrated.Content[1].Data.(*canonical.TextContent); !ok || text.Text != "Attachment: note.txt\nnote" || hydrated.Content[2].Index != 2 {
			t.Fatalf("request content = %+v", hydrated.Content)
		}
		if request[0].Message() != plain {
			t.Fatal("a message without an attachment was copied")
		}
	})
}
