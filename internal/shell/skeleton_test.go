package shell

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/settings"
)

// busy reports whether the window shows a loading skeleton: a busy
// element (aria-busy) holding the content's place.
func busy(tt *ui.Tester) bool {
	for _, x := range tt.AccessAll() {
		if x.Busy {
			return true
		}
	}
	return false
}

// The market's cards, the extension details and the model library show
// their skeletons while the preview loads them, then the content.
func TestSettingsSkeletons(t *testing.T) {
	a, tt := newTester(t)
	a.Navigate(MarketPage)
	tt.Frame()
	if !a.Settings.MarketLoad.Loading || !busy(tt) || tt.HasText("Jira") {
		t.Fatalf("no market skeleton: %q", tt.Texts())
	}
	snapshot(t, tt, "market-loading")
	tt.Advance(time.Second)
	tt.Frame()
	if busy(tt) || !tt.HasText("Jira") {
		t.Fatalf("market cards not loaded: %q", tt.Texts())
	}

	// Each kind's facts wait for the load: a plugin's version, a skill's
	// origin, a theme's formats.
	for _, x := range []struct {
		kind     settings.DetailKind
		id, fact string
	}{
		{settings.DetailPlugin, "git", i18n.In("en", "market_details_version")},
		{settings.DetailSkill, "sql-style", "skills.sh/acme/sql-style"},
		{settings.DetailTheme, "paper", i18n.In("en", "market_details_formats")},
	} {
		a.Settings.ShowDetails(x.kind, x.id)
		tt.Frame()
		tt.Advance(400 * time.Millisecond) // the right column opens
		tt.Frame()
		if !a.Settings.Details.Load.Loading || !busy(tt) || tt.HasText(x.fact) {
			t.Fatalf("%s: no skeleton: %q", x.id, tt.Texts())
		}
		if x.kind == settings.DetailPlugin {
			snapshot(t, tt, "details-loading")
		}
		tt.Advance(time.Second)
		tt.Frame()
		if busy(tt) || !tt.HasText(x.fact) {
			t.Fatalf("%s not loaded: %q", x.id, tt.Texts())
		}
	}
	a.Settings.CloseDetails()

	a.Navigate(SettingsPage)
	a.Settings.Open(settings.Knowledge)
	tt.Frame()
	click(t, tt, i18n.In("en", "model_library_manage"))
	tt.Advance(300 * time.Millisecond) // the dialog opens
	tt.Frame()
	if !a.Settings.LibraryLoad.Loading || !busy(tt) || tt.HasText("Whisper Small") {
		t.Fatalf("no library skeleton: %q", tt.Texts())
	}
	snapshot(t, tt, "library-loading")
	tt.Advance(time.Second)
	tt.Frame()
	if busy(tt) || !tt.HasText("Whisper Small") {
		t.Fatalf("library not loaded: %q", tt.Texts())
	}
	// A later search keeps the rows under the spinner.
	click(t, tt, i18n.In("en", "model_library_search"))
	if !busy(tt) || !tt.HasText("Whisper Small") {
		t.Fatalf("search hid the rows: %q", tt.Texts())
	}
	tt.Advance(time.Second)
	tt.Frame()
	if busy(tt) {
		t.Error("the search kept loading")
	}
}

// The host's metrics show the loading bar and the processes' skeleton
// until the first sample.
func TestProcessSkeleton(t *testing.T) {
	a, tt := newTester(t)
	a.openHost("local", false)
	tt.Frame()
	tt.Advance(400 * time.Millisecond) // the right column opens
	tt.Frame()
	if h, _ := a.hostPage(); h.Metrics != nil || !busy(tt) || !tt.HasText(i18n.In("en", "metrics_processes")) {
		t.Fatalf("no processes skeleton: %q", tt.Texts())
	}
	snapshot(t, tt, "processes-loading")
	tt.Advance(time.Second)
	tt.Frame()
	if h, _ := a.hostPage(); h.Metrics == nil || busy(tt) || !tt.HasText(i18n.In("en", "metrics_worker")) {
		t.Fatalf("no sample: %q", tt.Texts())
	}
}
