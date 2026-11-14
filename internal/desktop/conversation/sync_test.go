package conversation

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestUnsyncedRunDoesNotShowLiveCompactionPhase(t *testing.T) {
	state := New()
	turn := Turn{ID: "turn", Reply: Reply{Status: Unsynced, Phase: Compacting}}
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		state.reply(c, &turn, "", still{})
	}, 600, 300)
	tester.SetLocale("en")
	tester.Frame()
	if !tester.HasText(i18n.In("en", "chat_status_unsynced")) || tester.HasText(i18n.In("en", "turn_compacting")) {
		t.Fatal("an unsynced run still shows a live phase", tester.Texts())
	}
	turn.Reply.Status = Running
	tester.Frame()
	if !tester.HasText(i18n.In("en", "turn_compacting")) {
		t.Fatal("a resumed run did not show its phase", tester.Texts())
	}
}

func TestUnsyncedManualCompactionShowsUnknownStatus(t *testing.T) {
	state := New()
	turn := Turn{ID: "compact", Compaction: &Compaction{State: CompactionUnsynced}}
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		state.compaction(c, &turn)
	}, 600, 200)
	tester.SetLocale("en")
	tester.Frame()
	if !tester.HasText(i18n.In("en", "chat_status_unsynced")) || tester.HasText(i18n.In("en", "chat_compacting")) || tester.HasText(i18n.In("en", "chat_compact_failed")) {
		t.Fatal("unknown compaction outcome was presented as running or failed", tester.Texts())
	}
}
