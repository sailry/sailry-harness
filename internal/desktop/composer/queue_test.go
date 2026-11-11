package composer_test

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// owner is a composer's owner that counts what the queue's rows ask of it.
type owner struct {
	composer.Actions
	asked int
}

func (o *owner) SendQueued(string)            { o.asked++ }
func (o *owner) RemoveQueued(string)          { o.asked++ }
func (o *owner) WithdrawGuidance(string)      { o.asked++ }
func (o *owner) Discard(*composer.Attachment) {}

// removedCopy is the copy the queue strip dropped (docs/removed-ui.md): the
// reasons a row or a held background result waited on.
var removedCopy = []string{"Held", "Paused", "Paused after stop", "Paused after interruption", "Not sent",
	"Held at automatic limit", "Held after failure", "暂缓", "已暂停", "停止后暂停", "中断后暂停", "未发送", "已达自动续接上限", "失败后暂缓"}

// tray draws the composer over ctx in an 760 by 640 window.
func tray(locale string, ctx func() *composer.Context) *ui.Tester {
	state := composer.New(preview.DemoModel)
	tt := ui.NewTesterWith(func(c *ui.Context) {
		kit.SetPreferences(c, kit.Preferences{})
		ui.Column(c).Fill().Justify(ui.End).Children(func() { composer.View(c, state, ctx(), &owner{}) })
	}, 760, 640, ui.TesterOptions{})
	tt.SetLocale(locale)
	tt.Frame()
	tt.Frame()
	return tt
}

// The queue tray lists queued input only, for the demo's conversations as
// for a live session's: each row is its text, with no word on why it waits,
// and a conversation without queued input draws no tray, whatever
// background work it holds.
func TestQueueTray(t *testing.T) {
	for _, locale := range []string{"en", "zh-CN"} {
		d := preview.Demo(locale, "local")
		for _, id := range []string{preview.DemoStart, "harbor-api/deploy", "harbor-api/flaky-test", "harbor-api/invoices",
			"lumen-web/review", "lumen-web/audit", "chat/translate"} {
			t.Run(locale+"/"+id, func(t *testing.T) {
				cv := d.Find(id)
				if cv == nil {
					t.Fatal("not a demo conversation")
				}
				tt := tray(locale, func() *composer.Context {
					ctx := preview.DemoComposer(d, cv, cv.Worktree)
					ctx.Activity.Connected = true
					return ctx
				})
				for _, q := range cv.Queue {
					if q.Text != "" && !q.Notice && !tt.HasText(q.Text) {
						t.Errorf("the tray does not show %q: %q", q.Text, tt.Texts())
					}
				}
				for _, text := range tt.Texts() {
					if slices.Contains(removedCopy, text) {
						t.Errorf("the tray says why a row waits: %q", text)
					}
				}
				_, drawn := tt.Find(i18n.In(locale, "queue_count", "count", len(cv.Queue)))
				if drawn != (len(cv.Queue) > 0) {
					t.Errorf("tray drawn %v for %d queued", drawn, len(cv.Queue))
				}
			})
		}
	}
	// Background commands, running or finished, are the activity strip's:
	// they never open the tray.
	t.Run("background work alone draws no tray", func(t *testing.T) {
		tt := tray("en", func() *composer.Context {
			return &composer.Context{Activity: composer.Activity{Connected: true, Commands: []composer.Command{
				{ID: "soak", Command: "go test ./internal/booking -race -count=500"},
			}}}
		})
		if _, drawn := tt.Find(i18n.In("en", "queue_count", "count", 0)); drawn {
			t.Fatalf("a tray without queued input: %q", tt.Texts())
		}
	})
}

// Every row of the tray starts with the same leading slot, so the labels
// share one left edge whatever the row is: the drag handle on a row that can
// be moved, the loader on one being taken, the icon of its kind otherwise.
func TestQueueLead(t *testing.T) {
	file := []*composer.Attachment{{ID: "f", Name: "plan.csv", Status: composer.Ready, Progress: 1}}
	rows := []composer.Queued{
		{ID: "message", Text: "Queued message"},
		{ID: "guiding", Text: "Guiding message", Guided: true, Joining: true},
		{ID: "files", Text: "Message with a file", Attachments: file},
		{ID: "compaction", Compaction: true},
		{ID: "notice", Text: "go vet ./...", Notice: true},
	}
	labels := []string{"Queued message", "Guiding message", "Message with a file", i18n.In("en", "chat_compact"), i18n.In("en", "background_notice_title")}
	drag, running := i18n.In("en", "queue_drag"), i18n.In("en", "activity_running")
	for _, row := range []struct {
		name   string
		change func(*composer.Context)
		// handles and loaders are the marks the rows draw, in order: "h" a
		// drag handle, "l" the loader, "i" the row's icon.
		marks string
	}{
		{"a conversation", func(*composer.Context) {}, "hlhhi"},
		{"read-only", func(ctx *composer.Context) { ctx.Readonly = true }, "hlhhi"},
		{"a child session", func(ctx *composer.Context) { ctx.Child, ctx.Readonly = true, true }, "iliii"},
		{"the notice is next", func(ctx *composer.Context) { slices.Reverse(ctx.Queue) }, "lhhlh"},
		{"a message on its way", func(ctx *composer.Context) { ctx.Outgoing = &composer.Queued{Text: "Outgoing message"} }, "hlhhil"},
	} {
		t.Run(row.name, func(t *testing.T) {
			build := func() *composer.Context {
				ctx := &composer.Context{Queue: slices.Clone(rows), Activity: composer.Activity{Connected: true}}
				row.change(ctx)
				return ctx
			}
			tt := tray("en", build)
			ctx := build()
			names := make([]string, 0, len(ctx.Queue)+1)
			for _, q := range ctx.Queue {
				names = append(names, labels[slices.IndexFunc(rows, func(r composer.Queued) bool { return r.ID == q.ID })])
			}
			if ctx.Outgoing != nil {
				names = append(names, ctx.Outgoing.Text)
			}
			// A row and its label carry the same name: the row is the wider
			// box, the label the one that starts after the leading slot.
			var boxes, texts []ui.Rect
			for _, name := range names {
				var box, text ui.Rect
				for _, node := range tt.AccessAll() {
					if node.Name != name {
						continue
					}
					if box.W == 0 || node.Bounds.W > box.W {
						box = node.Bounds
					}
					if text.W == 0 || node.Bounds.X > text.X {
						text = node.Bounds
					}
				}
				if box.W == 0 || text.X <= box.X {
					t.Fatalf("row %q: box %+v, label %+v", name, box, text)
				}
				boxes, texts = append(boxes, box), append(texts, text)
			}
			for i, text := range texts {
				if d := text.X - texts[0].X; d > 0.5 || d < -0.5 {
					t.Errorf("row %q starts its label at %.1f, row %q at %.1f", names[i], text.X, names[0], texts[0].X)
				}
				if boxes[i].H != boxes[0].H || boxes[i].X != boxes[0].X {
					t.Errorf("row %q is %+v, row %q %+v", names[i], boxes[i], names[0], boxes[0])
				}
				// The slot is 24 wide after the row's padding of 8, with
				// the gap of 8 before the label.
				if d := text.X - boxes[i].X - 40; d > 0.5 || d < -0.5 {
					t.Errorf("row %q: the label starts %.1f into the row, want 40", names[i], text.X-boxes[i].X)
				}
			}
			// The mark in each slot: a handle or the loader by its name,
			// the icon by what the slot draws.
			inside := func(name string, box ui.Rect) bool {
				for _, node := range tt.AccessAll() {
					b := node.Bounds
					if node.Name == name && b.X >= box.X+8 && b.X+b.W <= box.X+32.5 && b.Y >= box.Y && b.Y+b.H <= box.Y+box.H+0.5 {
						return true
					}
				}
				return false
			}
			img := tt.Image()
			scale := float32(img.Bounds().Dx()) / 760
			inked := func(box ui.Rect) bool {
				ground := img.At(int((box.X+4)*scale), int((box.Y+box.H/2)*scale))
				for y := int((box.Y + 4) * scale); y < int((box.Y+box.H-4)*scale); y++ {
					for x := int((box.X + 8) * scale); x < int((box.X+32)*scale); x++ {
						if img.At(x, y) != ground {
							return true
						}
					}
				}
				return false
			}
			marks := ""
			for i, box := range boxes {
				switch {
				case inside(drag, box):
					marks += "h"
				case inside(running, box):
					marks += "l"
				case inked(box):
					marks += "i"
				default:
					marks += "-"
					t.Errorf("row %q has an empty leading slot", names[i])
				}
			}
			if marks != row.marks {
				t.Errorf("leading marks %q, want %q", marks, row.marks)
			}
		})
	}
	// The demo keeps a notice row behind its messages.
	if cv := preview.Demo("en", "local").Find(preview.DemoStart); !slices.ContainsFunc(cv.Queue, func(q composer.Queued) bool { return q.Notice }) {
		t.Error("the demo's queue has no background notice")
	}
}
