package shell

import (
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/transfers"
)

// The rail's installations button opens the requests; a Save as plays to
// its end and closes with Close.
func TestTransfers(t *testing.T) {
	a, tt := newTester(t)
	// Settings has no other Remove in sight.
	a.Navigate(SettingsPage)
	tt.Frame()
	clickButton(t, tt, i18n.In("en", "installation_title"))
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if !tt.HasText("Git worktrees") || !tt.HasText("45%") || !tt.HasText("12/40") {
		t.Fatalf("no requests: %q", tt.Texts())
	}
	snapshot(t, tt, "installations")
	n := len(a.installs.Entries)
	click(t, tt, i18n.In("en", "installation_remove"))
	tt.Frame()
	if len(a.installs.Entries) != n-1 {
		t.Error("remove")
	}
	a.installsOpen = false
	a.startDownload("report.pdf")
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if !tt.HasText(i18n.In("en", "files_download_receiving")) {
		t.Fatalf("not receiving: %q", tt.Texts())
	}
	for range 4 {
		tt.Advance(time.Second)
		tt.Frame()
	}
	if a.download.Status != transfers.Finished || a.download.Error != "" {
		t.Fatalf("download %+v", a.download)
	}
	clickButton(t, tt, i18n.In("en", "files_download_close"))
	tt.Frame()
	if a.download.Open {
		t.Error("Close kept the dialog")
	}
}
