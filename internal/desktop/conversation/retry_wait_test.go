package conversation_test

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestRetryWaitBudgetShowsDistinctTerminalLabel(t *testing.T) {
	for _, language := range []string{"en", "zh-CN"} {
		t.Run(language, func(t *testing.T) {
			state := conversation.New()
			retry := &conversation.Retry{Attempt: 2, Limit: 9, Stopped: true, Reason: "chat_model_retry_wait_exhausted", Detail: "provider busy"}
			turns := []conversation.Turn{{ID: "stopped", User: "Task", Reply: conversation.Reply{Status: conversation.Failed,
				Blocks: []conversation.Block{{Kind: conversation.RetryBlock, Retry: retry}},
			}}}
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				ui.Column(c).Fill().Children(func() { conversation.View(c, state, "session", turns, &conversation.Session{}, noActions{}) })
			}, 800, 500)
			tester.SetLocale(language)
			tester.SetPreferences(ui.Preferences{ReduceMotion: true})
			tester.Frame()
			work, ok := tester.AccessByRole(ui.RoleButton, i18n.In(language, "turn_failed"))
			if !ok {
				t.Fatal("the failed turn has no work disclosure")
			}
			tester.ClickAt(work.Bounds.X+work.Bounds.W/2, work.Bounds.Y+work.Bounds.H/2)
			tester.Frame()
			label := i18n.In(language, "chat_model_retry_wait_exhausted")
			if label == "chat_model_retry_wait_exhausted" || !tester.HasText(label) {
				t.Fatalf("waiting budget has no translated terminal label: %q", tester.Texts())
			}
			for _, wrong := range []string{
				i18n.In(language, "chat_model_retry_exhausted"),
				i18n.In(language, "chat_model_reconnecting", "attempt", 2, "limit", 9),
				i18n.In(language, "chat_model_retried", "attempt", 2, "limit", 9),
				i18n.In(language, "chat_model_reconnecting", "attempt", 9, "limit", 9),
			} {
				if tester.HasText(wrong) {
					t.Fatalf("waiting budget was shown as %q: %q", wrong, tester.Texts())
				}
			}
			if retry.Attempt != 2 || retry.Exhausted {
				t.Fatal("rendering invented exhausted attempts")
			}
		})
	}
}
