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
