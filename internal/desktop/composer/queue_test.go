package composer_test

import (
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

// The queue tray says why each row waits and lists the held background
// results with their reasons, for the demo's conversations as for a live
// session's: every reason is drawn in the window's language.
func TestQueueTrayReasons(t *testing.T) {
	keys := map[composer.Wait]string{
		composer.WaitHeld: "queue_wait_held", composer.WaitPaused: "queue_wait_paused", composer.WaitStopped: "queue_wait_stopped",
		composer.WaitInterrupted: "queue_wait_interrupted", composer.WaitUnsent: "queue_wait_unsent",
		composer.WaitLimit: "queue_wait_limit", composer.WaitFailed: "queue_wait_failed",
	}
	drawn := map[composer.Wait]bool{}
	for _, locale := range []string{"en", "zh-CN"} {
		d := preview.Demo(locale, "local")
		for _, id := range []string{preview.DemoStart, "harbor-api/deploy", "harbor-api/flaky-test", "harbor-api/invoices",
			"lumen-web/review", "lumen-web/audit", "chat/translate"} {
			t.Run(locale+"/"+id, func(t *testing.T) {
				cv := d.Find(id)
				if cv == nil {
					t.Fatal("not a demo conversation")
				}
				state := composer.New(preview.DemoModel)
				tt := ui.NewTesterWith(func(c *ui.Context) {
					kit.SetPreferences(c, kit.Preferences{})
					ctx := preview.DemoComposer(d, cv, cv.Worktree)
					ctx.Activity = composer.Activity{Connected: true}
					ui.Column(c).Fill().Justify(ui.End).Children(func() { composer.View(c, state, ctx, &owner{}) })
				}, 760, 640, ui.TesterOptions{})
				tt.SetLocale(locale)
				tt.Frame()
				tt.Frame()
				want := func(text string) {
					t.Helper()
					if !tt.HasText(text) {
						t.Errorf("the tray does not show %q: %q", text, tt.Texts())
					}
				}
				for _, q := range cv.Queue {
					if q.Text != "" && !q.Notice {
						want(q.Text)
					}
					if q.Wait != composer.WaitNone {
						want(i18n.In(locale, keys[q.Wait]))
						drawn[q.Wait] = true
					}
				}
				for _, h := range cv.Held {
					want(h.Title)
					want(i18n.In(locale, keys[h.Wait]))
					drawn[h.Wait] = true
				}
			})
		}
	}
	for wait, key := range keys {
		if !drawn[wait] {
			t.Errorf("no tray drew %s", key)
		}
	}
}
