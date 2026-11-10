package shell

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/transfers"
)

// startDownload opens the Save as dialog for a file. The reference first asks
// where in the system's save panel; the preview names a folder and writes
// nothing.
func (a *App) startDownload(name string) {
	a.download = transfers.Download{Open: true, Name: name, Destination: "~/Downloads/" + name, Size: 4 << 20}
	a.downloaded = time.Time{} // starts as the dialog first shows
}

// downloadDialog plays the transfer and shows its dialog; as it finishes, a
// toast tells the outcome: canceled, or that the preview saved nothing.
func (a *App) downloadDialog(c *ui.Context) {
	if !a.Preview {
		if a.Hooks.DownloadDialog != nil {
			a.Hooks.DownloadDialog(c)
		}
		return
	}
	d := &a.download
	if d.Name == "" || !d.Open && d.Status == transfers.Finished {
		return
	}
	if a.downloaded.IsZero() {
		a.downloaded = c.Now()
	}
	was := d.Status
	if preview.PlayDownload(d, a.downloaded, c.Now()) {
		c.After(preview.Tick)
	}
	if was != transfers.Finished && d.Status == transfers.Finished {
		if d.Error == "files_download_cancelled" {
			kit.Notify(c, "", i18n.T(c, d.Error))
		} else {
			kit.Message(c, i18n.T(c, "preview_notice"))
		}
	}
	transfers.DownloadDialog(c, d, func() {})
}
