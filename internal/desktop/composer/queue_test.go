package composer_test

import (
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

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
	return trayOf(composer.New(preview.DemoModel), &owner{}, locale, ctx)
}

// trayOf is tray for a composer's state and its owner.
func trayOf(state *composer.State, actions composer.Actions, locale string, ctx func() *composer.Context) *ui.Tester {
	tt := ui.NewTesterWith(func(c *ui.Context) {
		kit.SetPreferences(c, kit.Preferences{})
		ui.Column(c).Fill().Justify(ui.End).Children(func() { composer.View(c, state, ctx(), actions) })
	}, 760, 640, ui.TesterOptions{})
	tt.SetLocale(locale)
	tt.Frame()
	tt.Frame()
	return tt
}

// queueHeader is the tray's header, the button named by the n messages
// waiting.
func queueHeader(tt *ui.Tester, locale string, n int) (ui.AccessInfo, bool) {
	name := i18n.In(locale, "queue_messages_count", "count", n)
	if n == 1 {
		name = i18n.In(locale, "queue_messages_one")
	}
	return tt.AccessByRole(ui.RoleButton, name)
}

// fold clicks the tray's header, which folds or unfolds the rows, and lets
// them come or go.
func fold(t *testing.T, tt *ui.Tester, locale string, n int) {
	t.Helper()
	header, ok := queueHeader(tt, locale, n)
	if !ok {
		t.Fatalf("no header for %d queued: %q", n, tt.Texts())
	}
	tt.ClickAt(header.Bounds.X+header.Bounds.W/2, header.Bounds.Y+header.Bounds.H/2)
	settle(tt)
}

func settle(tt *ui.Tester) {
	tt.Frame()
	tt.Advance(300 * time.Millisecond)
	tt.Frame()
}

// buttons counts the buttons named name.
func buttons(tt *ui.Tester, name string) (n int) {
	for _, e := range tt.AccessAll() {
		if e.Role == ui.RoleButton && e.Name == name {
			n++
		}
	}
	return n
}

// The queue tray lists queued input only, for the demo's conversations as
// for a live session's: each row is its text, with no word on why it waits,
// and a conversation without queued input draws no tray, whatever
// background work it holds. The tray's header counts the messages and folds
// the rows away: folded until the user unfolds it (fold).
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
				// Folded, as a conversation starts: the header alone.
				for _, q := range cv.Queue {
					if q.Text != "" && !q.Notice && tt.HasText(q.Text) {
						t.Errorf("the folded tray shows %q", q.Text)
					}
				}
				if header, ok := queueHeader(tt, locale, len(cv.Queue)); ok != (len(cv.Queue) > 0) || header.Expanded {
					t.Errorf("header %+v, shown %v, for %d queued", header, ok, len(cv.Queue))
				}
				if len(cv.Queue) > 0 {
					fold(t, tt, locale, len(cv.Queue))
				}
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
		if _, drawn := tt.Find(i18n.In("en", "queue_messages_count", "count", 0)); drawn {
			t.Fatalf("a header without queued input: %q", tt.Texts())
		}
	})
	t.Run("fold", foldTests)
}

// four is a queue of four messages, the last with a file.
func four() []composer.Queued {
	return []composer.Queued{
		{ID: "a", Text: "Add a Retry-After header"},
		{ID: "b", Text: "Keep the counters in Redis"},
		{ID: "c", Text: "Guide the running reply", Guided: true, Joining: true},
		{ID: "d", Text: "Match the budgets in this sheet", Attachments: []*composer.Attachment{{ID: "f", Name: "plan.csv", Status: composer.Ready, Progress: 1}}},
	}
}

// foldTests are the header's: the tray is folded until the user unfolds it,
// per composer, by a click, Enter or Space; a message being edited is never
// folded away.
func foldTests(t *testing.T) {
	texts := func(tt *ui.Tester, queue []composer.Queued) (n int) {
		for _, q := range queue {
			if tt.HasText(q.Text) {
				n++
			}
		}
		return n
	}
	// line reports whether the hairline is drawn: a row of the border's
	// color across the tray, under the header.
	line := func(tt *ui.Tester, header ui.Rect) bool {
		img := tt.Image()
		scale := float32(img.Bounds().Dx()) / 760
		ground := img.At(int((header.X+header.W/2)*scale), int((header.Y+header.H-2)*scale))
		y := int((header.Y + header.H + 4.5) * scale)
		for x := int((header.X + 16) * scale); x < int((header.X+header.W-16)*scale); x++ {
			if img.At(x, y) == ground {
				return false
			}
		}
		return true
	}
	en := func(key string) string { return i18n.In("en", key) }
	view := func(queue *[]composer.Queued) func() *composer.Context {
		return func() *composer.Context {
			return &composer.Context{Queue: slices.Clone(*queue), Activity: composer.Activity{Connected: true}}
		}
	}

	t.Run("folded until clicked, then unfolded, then folded", func(t *testing.T) {
		queue := four()
		tt := tray("en", view(&queue))
		header, ok := queueHeader(tt, "en", 4)
		if !ok || header.Expanded || !header.Focusable || header.Disabled {
			t.Fatalf("header %+v, found %v: %q", header, ok, tt.Texts())
		}
		if !tt.HasText("4 queued messages") {
			t.Errorf("the header does not count the queue: %q", tt.Texts())
		}
		if n := texts(tt, queue); n != 0 {
			t.Errorf("the folded tray shows %d rows", n)
		}
		for _, key := range []string{"queue_edit", "queue_delete", "queue_withdraw", "queue_drag"} {
			if n := buttons(tt, en(key)); n != 0 {
				t.Errorf("the folded tray has %d %q", n, en(key))
			}
		}
		// Send now is named as the composer's own Send, which stays.
		sends := buttons(tt, en("queue_send_now"))
		if sends != 1 {
			t.Errorf("the folded tray has %d %q beside the composer's", sends-1, en("queue_send_now"))
		}
		if line(tt, header.Bounds) {
			t.Error("the folded tray draws the hairline")
		}

		fold(t, tt, "en", 4)
		header, _ = queueHeader(tt, "en", 4)
		if !header.Expanded {
			t.Errorf("the unfolded header is not expanded: %+v", header)
		}
		if n := texts(tt, queue); n != 4 {
			t.Errorf("the unfolded tray shows %d rows of 4: %q", n, tt.Texts())
		}
		for key, want := range map[string]int{"queue_edit": 4, "queue_send_now": sends + 4, "queue_delete": 4, "queue_withdraw": 1, "queue_drag": 3} {
			if n := buttons(tt, en(key)); n != want {
				t.Errorf("%d %q, want %d", n, en(key), want)
			}
		}
		// The guiding row's Edit and Remove are off, as before.
		off := 0
		for _, e := range tt.AccessAll() {
			if e.Role == ui.RoleButton && e.Disabled && (e.Name == en("queue_edit") || e.Name == en("queue_delete")) {
				off++
			}
		}
		if off != 2 {
			t.Errorf("%d disabled Edit and Remove, want the guiding row's 2", off)
		}
		if !line(tt, header.Bounds) {
			t.Error("the unfolded tray draws no hairline under its header")
		}
		// The header is the tray's first row, over the rows.
		if first, ok := tt.Find(queue[0].Text); !ok || first.Y < header.Bounds.Y+header.Bounds.H {
			t.Errorf("row %+v is not under the header %+v", first, header.Bounds)
		}

		fold(t, tt, "en", 4)
		if header, _ = queueHeader(tt, "en", 4); header.Expanded || texts(tt, queue) != 0 || line(tt, header.Bounds) {
			t.Errorf("a second click did not fold: %+v, %q", header, tt.Texts())
		}
	})

	t.Run("the count follows the queue, in both languages", func(t *testing.T) {
		for _, row := range []struct {
			locale    string
			n         int
			want, not string
		}{
			{"en", 4, "4 queued messages", "1 queued message"},
			{"en", 1, "1 queued message", "1 queued messages"},
			{"zh-CN", 4, "4 条消息排队", ""},
			{"zh-CN", 1, "1 条消息排队", ""},
		} {
			queue := four()[:row.n]
			tt := tray(row.locale, view(&queue))
			if header, ok := tt.AccessByRole(ui.RoleButton, row.want); !ok || !tt.HasText(row.want) {
				t.Errorf("%s, %d queued: no header %q: %+v, %q", row.locale, row.n, row.want, header, tt.Texts())
			}
			if row.not != "" && tt.HasText(row.not) {
				t.Errorf("%s, %d queued: the header says %q", row.locale, row.n, row.not)
			}
		}
		// The same composer, as its queue grows and shrinks.
		queue := four()[:2]
		tt := tray("en", view(&queue))
		if !tt.HasText("2 queued messages") {
			t.Fatalf("2 queued: %q", tt.Texts())
		}
		queue = four()
		settle(tt)
		if !tt.HasText("4 queued messages") || tt.HasText("2 queued messages") {
			t.Errorf("4 queued: %q", tt.Texts())
		}
		queue = four()[:1]
		settle(tt)
		if !tt.HasText("1 queued message") || tt.HasText("4 queued messages") {
			t.Errorf("1 queued: %q", tt.Texts())
		}
		// A message on its way is a row already, and counted.
		tt = tray("en", func() *composer.Context {
			return &composer.Context{Queue: four()[:1], Outgoing: &composer.Queued{Text: "On its way"}, Activity: composer.Activity{Connected: true}}
		})
		if !tt.HasText("2 queued messages") {
			t.Errorf("one queued and one on its way: %q", tt.Texts())
		}
	})

	t.Run("each composer keeps its own fold", func(t *testing.T) {
		queue := four()
		one, other := composer.New(preview.DemoModel), composer.New(preview.DemoModel)
		tt := trayOf(one, &owner{}, "en", view(&queue))
		fold(t, tt, "en", 4)
		if texts(tt, queue) != 4 {
			t.Fatalf("the clicked composer did not unfold: %q", tt.Texts())
		}
		if tt := trayOf(other, &owner{}, "en", view(&queue)); texts(tt, queue) != 0 {
			t.Errorf("another composer unfolded with it: %q", tt.Texts())
		}
		// The same state drawn again, as its conversation shown again.
		if tt := trayOf(one, &owner{}, "en", view(&queue)); texts(tt, queue) != 4 {
			t.Errorf("the composer lost its fold: %q", tt.Texts())
		}
	})

	t.Run("the fold is kept while the queue empties and fills", func(t *testing.T) {
		queue := four()
		tt := tray("en", view(&queue))
		fold(t, tt, "en", 4)
		queue = nil
		settle(tt)
		if _, ok := tt.Find(i18n.In("en", "queue_count", "count", 0)); ok {
			t.Fatalf("a tray without queued input: %q", tt.Texts())
		}
		queue = four()[:2]
		settle(tt)
		if texts(tt, queue) != 2 {
			t.Errorf("the tray folded when the queue emptied: %q", tt.Texts())
		}
	})

	t.Run("Enter and Space on the focused header", func(t *testing.T) {
		queue := four()
		tt := tray("en", view(&queue))
		// Tab and Shift+Tab are the draft's own keys (queue, cycle the
		// mode), so the header takes the focus here by a click, which
		// unfolds; the keys then fold and unfold it.
		fold(t, tt, "en", 4)
		if h, _ := queueHeader(tt, "en", 4); !h.Focused || !h.Expanded {
			t.Fatalf("the clicked header: %+v", h)
		}
		tt.Key(0, ui.KeySpace)
		settle(tt)
		if h, _ := queueHeader(tt, "en", 4); h.Expanded || !h.Focused || texts(tt, queue) != 0 {
			t.Fatalf("Space did not fold: %+v, %q", h, tt.Texts())
		}
		tt.Key(0, ui.KeyEnter)
		settle(tt)
		if h, _ := queueHeader(tt, "en", 4); !h.Expanded || texts(tt, queue) != 4 {
			t.Fatalf("Enter did not unfold: %+v, %q", h, tt.Texts())
		}
		// Unfolded, the next stop after the header is in the first row.
		tt.Key(0, ui.KeyTab)
		tt.Frame()
		first, _ := tt.Find(queue[0].Text)
		focused := false
		for _, e := range tt.AccessAll() {
			if e.Focused && e.Bounds.Y >= first.Y-16 && e.Bounds.Y <= first.Y+first.H {
				focused = true
			}
		}
		if !focused {
			t.Errorf("Tab from the header does not go to the first row")
		}
		// And back from the row to the header, before it.
		tt.Key(ui.Shift, ui.KeyTab)
		tt.Frame()
		if h, _ := queueHeader(tt, "en", 4); !h.Focused {
			t.Errorf("Shift+Tab from the first row does not go to the header: %+v", h)
		}
	})

	t.Run("a message being edited is never folded away", func(t *testing.T) {
		queue := four()
		state := composer.New(preview.DemoModel)
		tt := trayOf(state, &owner{}, "en", view(&queue))
		fold(t, tt, "en", 4)
		edit, _ := tt.AccessByRole(ui.RoleButton, en("queue_edit"))
		tt.ClickAt(edit.Bounds.X+edit.Bounds.W/2, edit.Bounds.Y+edit.Bounds.H/2)
		settle(tt)
		tt.Type(" soon")
		tt.Frame()
		// The editor's text, the caret wherever the editor put it.
		drafted := func() bool {
			area, ok := tt.AccessByRole(ui.RoleTextField, en("queue_edit"))
			return ok && len(area.Value) == len(queue[0].Text)+len(" soon") && strings.Contains(area.Value, " soon")
		}
		if !tt.HasText(en("queue_save")) || !drafted() {
			t.Fatalf("no editor: %q", tt.Texts())
		}
		// The header stays, counting, and takes no fold: by a click or a key.
		header, ok := queueHeader(tt, "en", 4)
		if !ok || !header.Expanded || !header.Disabled {
			t.Fatalf("header while editing: %+v, found %v", header, ok)
		}
		if !line(tt, header.Bounds) {
			t.Error("no hairline between the header and the editor")
		}
		tt.ClickAt(header.Bounds.X+header.Bounds.W/2, header.Bounds.Y+header.Bounds.H/2)
		settle(tt)
		if !tt.HasText(en("queue_save")) || !drafted() {
			t.Fatalf("a click on the header hid the editor or lost its draft: %q", tt.Texts())
		}
		// Escape leaves the editor, as before, to the rows still unfolded.
		if !state.Escape(&owner{}) {
			t.Fatal("Escape did not close the editor")
		}
		settle(tt)
		if tt.HasText(en("queue_save")) || texts(tt, queue) != 4 {
			t.Errorf("after Escape: %q", tt.Texts())
		}
		if header, _ := queueHeader(tt, "en", 4); header.Disabled || !header.Expanded {
			t.Errorf("header after Escape: %+v", header)
		}
	})

	t.Run("a child session's tray", func(t *testing.T) {
		queue := four()
		tt := tray("en", func() *composer.Context {
			return &composer.Context{Child: true, Readonly: true, Queue: slices.Clone(queue), Activity: composer.Activity{Connected: true}}
		})
		header, ok := queueHeader(tt, "en", 4)
		if !ok || header.Expanded || texts(tt, queue) != 0 {
			t.Fatalf("the child's tray is not folded under its header: %+v, %v, %q", header, ok, tt.Texts())
		}
		fold(t, tt, "en", 4)
		if texts(tt, queue) != 4 || buttons(tt, en("queue_read")) != 4 || buttons(tt, en("queue_send_now")) != 0 || buttons(tt, en("queue_drag")) != 0 {
			t.Errorf("the child's unfolded tray: %q", tt.Texts())
		}
		fold(t, tt, "en", 4)
		if texts(tt, queue) != 0 {
			t.Errorf("the child's tray did not fold: %q", tt.Texts())
		}
	})

	// The header starts as a row does: the chevron in the leading slot, the
	// label on the rows' edge, in the rows' height.
	t.Run("the header shares the rows' edges", func(t *testing.T) {
		queue := four()
		tt := tray("en", view(&queue))
		fold(t, tt, "en", 4)
		header, _ := queueHeader(tt, "en", 4)
		label, _ := tt.Find("4 queued messages")
		row, _ := tt.Find(queue[0].Text)
		if d := label.X - row.X; d > 0.5 || d < -0.5 {
			t.Errorf("the header's label starts at %.1f, a row's at %.1f", label.X, row.X)
		}
		if header.Bounds.H != 32 {
			t.Errorf("the header is %.1f high, a row 32", header.Bounds.H)
		}
	})

	// SAILRY_UI_SHOTS=dir writes the tray folded and unfolded, dark and
	// light, to look at.
	if dir := os.Getenv("SAILRY_UI_SHOTS"); dir != "" {
		for _, dark := range []bool{true, false} {
			queue := four()
			d := preview.Demo("en", "local")
			cv := d.Find(preview.DemoStart)
			tt := tray("en", func() *composer.Context {
				ctx := preview.DemoComposer(d, cv, cv.Worktree)
				ctx.Queue, ctx.Activity = slices.Clone(queue), composer.Activity{Connected: true}
				return ctx
			})
			tt.SetDark(dark)
			settle(tt)
			name := map[bool]string{true: "dark", false: "light"}[dark]
			shoot := func(file string) {
				f, err := os.Create(filepath.Join(dir, file+"-"+name+".png"))
				if err != nil {
					t.Fatal(err)
				}
				defer f.Close()
				if err := png.Encode(f, tt.Image()); err != nil {
					t.Fatal(err)
				}
			}
			shoot("queue-folded")
			header, _ := queueHeader(tt, "en", 4)
			tt.Move(header.Bounds.X+60, header.Bounds.Y+16)
			settle(tt)
			shoot("queue-folded-hover")
			fold(t, tt, "en", 4)
			tt.Move(4, 4)
			settle(tt)
			shoot("queue-unfolded")
		}
	}
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
			waiting := len(ctx.Queue)
			if ctx.Outgoing != nil {
				waiting++
			}
			fold(t, tt, "en", waiting) // the rows show once unfolded
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
