package composer

import (
	"bytes"
	"image"
	"image/color"
	"image/png"
	"strconv"
	"testing"

	"github.com/egoist/mygo/transfer"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/uitest"
)

type pastedImageActions struct {
	Actions
	uploaded, discarded *Attachment
	sent                Message
}

func (a *pastedImageActions) Upload(file *Attachment) {
	a.uploaded = file
	file.Status = Ready
}

func (a *pastedImageActions) Discard(file *Attachment) { a.discarded = file }
func (a *pastedImageActions) Send(message Message)     { a.sent = message }

func TestPastedImageThumbnail(t *testing.T) {
	uitest.Parallel(t)
	const width, height = 4096, 3264
	original := image.NewRGBA(image.Rect(0, 0, width, height))
	original.SetRGBA(width-1, height-1, color.RGBA{R: 190, G: 45, A: 255})
	var data bytes.Buffer
	if err := (&png.Encoder{CompressionLevel: png.BestSpeed}).Encode(&data, original); err != nil {
		t.Fatal(err)
	}

	state := New("model")
	shown := state
	actions := &pastedImageActions{}
	ctx := &Context{Models: []Model{{ID: "model"}}, Activity: Activity{Connected: true}}
	tt := uitest.Fast(func(c *ui.Context) {
		kit.SetPreferences(c, kit.Preferences{})
		ui.Column(c).Fill().Justify(ui.End).Children(func() { View(c, shown, ctx, actions) })
	}, 760, 640, "en")
	if err := tt.Click(i18n.In("en", "composer")); err != nil {
		t.Fatal(err)
	}
	tt.SetClipboardData(transfer.New(transfer.NewItem(transfer.Bytes(transfer.PNG, data.Bytes()))))
	tt.Key(ui.Cmd, ui.KeyV)
	tt.Frame()
	if len(state.Attachments) != 1 || actions.uploaded != state.Attachments[0] {
		t.Fatal("paste did not hand the draft attachment to the uploader")
	}
	a := actions.uploaded
	if a.Mime != string(transfer.PNG) || a.Size != int64(data.Len()) || !bytes.Equal(a.Data, data.Bytes()) {
		t.Fatal("the uploader did not receive the original PNG")
	}
	if w, h := ui.ImageSize(a.Image); w != 480 || h != 382 {
		t.Fatalf("paste retained a %gx%g card bitmap; want 480x382", w, h)
	}
	if state.view.original != nil {
		t.Fatal("paste decoded an original for a closed viewer")
	}
	if err := tt.Click(a.Name); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	view := state.view.original
	if w, h := ui.ImageSize(view); w != width || h != height {
		t.Fatalf("viewer bitmap = %gx%g; want %dx%d", w, h, width, height)
	}
	shown = New("model")
	tt.Frame()
	if w, h := view.Size(); w != 0 || h != 0 || state.view.Open || len(state.view.Images) != 0 {
		t.Fatal("switching composers retained the open gallery or its original pixels")
	}
	if len(state.Attachments) != 1 || !bytes.Equal(a.Data, data.Bytes()) {
		t.Fatal("switching composers lost the draft or its original PNG")
	}

	shown = state
	tt.Frame()
	if err := tt.Click(i18n.In("en", "composer")); err != nil {
		t.Fatal(err)
	}
	tt.Key(0, ui.KeyEnter)
	tt.Frame()
	if len(state.Attachments) != 0 || len(actions.sent.Attachments) != 1 || !bytes.Equal(actions.sent.Attachments[0].Data, data.Bytes()) {
		t.Fatal("sending did not clear the composer and hand off the original PNG")
	}

	state.KeepFailedDraft(actions.sent, true)
	if len(state.Attachments) != 1 || state.Attachments[0] != a {
		t.Fatal("failed send did not restore the attachment")
	}
	backing := state.Attachments
	state.remove(a, actions)
	if len(state.Attachments) != 0 || backing[0] != nil || actions.discarded != a {
		t.Fatal("removing the last attachment left its draft slot occupied")
	}
}

func TestKeepFailedDraftRetainsAllUnsentMessages(t *testing.T) {
	state := New("model")
	state.Draft = "still editing"
	for i := range 17 {
		text := strconv.Itoa(i)
		state.KeepFailedDraft(Message{Text: text, Attachments: []*Attachment{{ID: text}}}, true)
	}
	if state.Draft != "still editing" || len(state.recovery.drafts) != 17 {
		t.Fatal("a newer draft or unsent message was lost")
	}
	for i, draft := range state.recovery.drafts {
		text := strconv.Itoa(i)
		if draft.text != text || len(draft.attachments) != 1 || draft.attachments[0].ID != text {
			t.Fatalf("message %d lost its text or attachment", i)
		}
	}
}

func TestReleaseCommittedOutgoingAttachments(t *testing.T) {
	uitest.Parallel(t)
	state := New("model")
	first, second := &Attachment{ID: "first"}, &Attachment{ID: "second"}
	state.HoldOutgoingAttachments([]*Attachment{first, second}, func() bool { return true })
	state.view.Show([]*Attachment{first, second}, 0)
	original := ui.NewBitmap(image.NewRGBA(image.Rect(0, 0, 481, 1)))
	state.view.originalFor, state.view.original = first, original
	state.ReleaseCommittedAttachment(first)
	if w, h := original.Size(); w != 0 || h != 0 || state.view.Open || len(state.view.Images) != 0 {
		t.Fatal("send admission retained the open gallery or its original pixels")
	}
	if state.holdsDraftAttachment(first) || !state.holdsDraftAttachment(second) {
		t.Fatal("commit did not release only the committed attachment")
	}
	state.ReleaseCommittedAttachment(second)
	if state.recovery.outgoing != nil || state.recovery.pending != nil {
		t.Fatal("an inactive composer retained the settled outgoing draft")
	}
}
