package transfers

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// DownloadDialog is the Save as dialog (downloads.rs Form): 460 wide, the
// file over where it goes, the progress (unknown while preparing and
// publishing, none after a failure), the step until it finishes, and
// Cancel, which becomes Close once it finished. It does not close from
// outside; closing it cancels. cancel asks the transfer to stop.
func DownloadDialog(c *ui.Context, d *Download, cancel func()) {
	if !d.Open {
		return
	}
	p := theme.Of(c)
	w, _ := c.Size()
	title := i18n.T(c, "files_download")
	finished := d.Status == Finished
	open := d.Open
	kit.Dialog(c.Key("download"), &d.Open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600).TextColor(p.PopoverForeground)
		ui.Text(c, d.Name).FontSize(14)
		ui.Text(c, d.Destination).FontSize(14).TextColor(p.MutedForeground)
		key, value := d.Key()
		if !(finished && d.Error != "") {
			ui.Progress(c.Key("progress"), value).FillWidth().Height(4).Label(title)
		}
		if !finished {
			ui.Text(c, i18n.T(c, key)).FontSize(14).TextColor(p.MutedForeground)
		}
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Padding(12, 0, 0, 0).Children(func() {
			label := i18n.T(c, "settings_cancel")
			if finished {
				label = i18n.T(c, "files_download_close")
			}
			b := kit.Secondary(c.Key("close"), label)
			if !finished && (d.Canceled || d.Status == Publishing) {
				b.Disabled(true).Opacity(0.5)
			}
			b.OnClick(func() {
				if finished {
					d.Open = false
					return
				}
				d.Canceled = true
				cancel()
			})
		})
	}).Width(min(460, w-48)).Label(title)
	if open && !d.Open && !finished {
		// Closed with Escape: the transfer stops (on_close).
		d.Canceled = true
		cancel()
	}
}
