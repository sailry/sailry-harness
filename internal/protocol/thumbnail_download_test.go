package protocol

import (
	"bytes"
	"encoding/json"
	"reflect"
	"testing"
)

func TestThumbnailDownloadRequestsAreAdditive(t *testing.T) {
	attachment := Attachment{ID: AttachmentID{1}, Spec: AttachmentSpec{Worktree: WorktreeID{2}, Name: "picture.png", MediaType: "image/png", Size: 512, Revision: "original"}}
	picture := ConversationImage{Entry: "entry", Attachment: attachment}
	for _, tc := range []struct {
		kind              string
		original, derived any
	}{
		{"download_file", CommandDownloadFilePayload{Worktree: WorktreeID{2}, Path: "picture.png"}, CommandDownloadFilePayload{Worktree: WorktreeID{2}, Path: "picture.png", ThumbnailEdge: 480}},
		{"download_attachment", CommandDownloadAttachmentPayload{Attachment: attachment.ID}, CommandDownloadAttachmentPayload{Attachment: attachment.ID, ThumbnailEdge: 480}},
		{"download_image", CommandDownloadImagePayload{Session: SessionID{3}, Image: picture}, CommandDownloadImagePayload{Session: SessionID{3}, Image: picture, ThumbnailEdge: 480}},
	} {
		t.Run(tc.kind, func(t *testing.T) {
			original, err := json.Marshal(Command{Kind: tc.kind, Data: tc.original})
			if err != nil || bytes.Contains(original, []byte(`"thumbnail_edge"`)) {
				t.Fatalf("original request = %s: %v", original, err)
			}
			derived, err := json.Marshal(Command{Kind: tc.kind, Data: tc.derived})
			if err != nil {
				t.Fatal(err)
			}
			for _, data := range [][]byte{original, derived} {
				var decoded Command
				if err := json.Unmarshal(data, &decoded); err != nil {
					t.Fatal(err)
				}
				if decoded.Durable() || decoded.UsesDraftAttachment() != (tc.kind == "download_attachment") {
					t.Fatalf("download scope changed: %+v", decoded)
				}
				roundTrip, err := json.Marshal(decoded)
				if err != nil || !bytes.Equal(roundTrip, data) {
					t.Fatalf("request round trip = %s, want %s: %v", roundTrip, data, err)
				}
			}
			legacy, current := fixtureJSON(t, original), fixtureJSON(t, derived)
			payload := current.(map[string]any)["data"].(map[string]any)
			if !bytes.Contains(derived, []byte(`"thumbnail_edge":480`)) {
				t.Fatalf("missing thumbnail selector: %s", derived)
			}
			delete(payload, "thumbnail_edge")
			if !reflect.DeepEqual(legacy, current) {
				t.Fatalf("existing wire fields changed: %s versus %s", original, derived)
			}
		})
	}
	// The old attachment request decoder's open payload also accepts the
	// selector on a zero-worktree draft, while retaining its original scope.
	var old attachmentLocation
	data, err := json.Marshal(CommandDownloadAttachmentPayload{Attachment: attachment.ID, ThumbnailEdge: 480})
	if err != nil {
		t.Fatal(err)
	}
	if err := DecodeObject(data, &old, []string{"worktree", "attachment"}, false); err != nil || old.Worktree != (attachmentWorktreeID{}) || old.Attachment != attachment.ID {
		t.Fatalf("old attachment decoder: %+v, %v", old, err)
	}
}

func TestThumbnailDownloadOutputSeparatesSourceAndStreamRevisions(t *testing.T) {
	source := FileDownload{Stream: StreamID{1}, Worktree: WorktreeID{2}, Path: "picture.png", Size: 568, Revision: "original", Stamp: "source stamp"}
	legacy, err := json.Marshal(source)
	if err != nil || bytes.Contains(legacy, []byte(`"thumbnail"`)) {
		t.Fatalf("original output = %s: %v", legacy, err)
	}
	var decoded FileDownload
	if err := json.Unmarshal(legacy, &decoded); err != nil || decoded != source {
		t.Fatalf("original output decoded as %+v: %v", decoded, err)
	}
	facts := ThumbnailTransfer{Size: 84, Revision: "derivative"}
	source.Thumbnail = &facts
	updated, err := json.Marshal(source)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(updated, &decoded); err != nil || decoded.Thumbnail == nil || *decoded.Thumbnail != facts {
		t.Fatalf("thumbnail output decoded as %+v: %v", decoded, err)
	}
	decoded.Thumbnail = nil
	source.Thumbnail = nil
	if decoded != source {
		t.Fatalf("source facts changed to %+v", decoded)
	}
	attachment := AttachmentDownload{Stream: source.Stream, Attachment: Attachment{ID: AttachmentID{3}, Spec: AttachmentSpec{Size: source.Size, Revision: source.Revision}}, Thumbnail: &facts}
	updated, err = json.Marshal(attachment)
	if err != nil {
		t.Fatal(err)
	}
	var reply AttachmentDownload
	if err := json.Unmarshal(updated, &reply); err != nil || reply.Thumbnail == nil || *reply.Thumbnail != facts || reply.Attachment != attachment.Attachment {
		t.Fatalf("attachment source/stream facts = %+v: %v", reply, err)
	}
}
