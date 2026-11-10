package transfers

import (
	"path"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

type EntryTransferStatus int

const (
	EntryReady EntryTransferStatus = iota
	EntryRunning
	EntryDone
	EntryFailed
	EntryUncertain
	EntryCancelled
	EntryExists
)

// EntryTransfer is a captured copy or move, separate from byte-stream downloads.
type EntryTransfer struct {
	Open                                 bool
	Move                                 bool
	Upload                               bool
	Phase                                string
	Copied, Size                         uint64
	Cancelable                           bool
	SourceLabel, SourcePath, Destination string
	Remaining                            []string
	Status                               EntryTransferStatus
	Canceling                            bool
}
type EntryTransferActions struct {
	Start   func(string)
	Check   func()
	Cancel  func()
	Close   func()
	Replace func()
}

func (d *EntryTransfer) Actionable() bool {
	return d.Status == EntryReady || d.Status == EntryFailed || d.Status == EntryCancelled || d.Status == EntryUncertain || d.Status == EntryExists
}

// EntryTransferDialog uses the existing transfer form's destination draft,
// remaining entries and outcome-check action. Effects stay with its owner.
func EntryTransferDialog(c *ui.Context, d *EntryTransfer, actions EntryTransferActions) {
	if !d.Open {
		return
	}
	p := theme.Of(c)
	w, _ := c.Size()
	titleKey := "files_paste"
	if d.Move {
		titleKey = "files_move"
	}
	if d.Upload {
		titleKey = "files_upload"
	}
	title := i18n.T(c, titleKey)
	open := d.Open
	kit.Dialog(c.Key("entry-transfer"), &d.Open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600).TextColor(p.PopoverForeground)
		ui.Column(c.Key("body")).Gap(12).Children(func() {
			if !d.Upload {
				ui.Column(c.Key("source")).Gap(4).Children(func() {
					ui.Text(c, i18n.T(c, "files_paste_source")).FontSize(14).TextColor(p.MutedForeground)
					ui.Text(c, d.SourceLabel).FontSize(14)
				})
			}
			if d.Upload {
				ui.Text(c, d.Destination).FontSize(14).SingleLine()
			}
			ui.Text(c, d.SourcePath).FontSize(14).TextColor(p.MutedForeground)
			if len(d.Remaining) > 0 {
				ui.Scroll(c.Key("remaining")).MaxHeight(128).Gap(4).Children(func() {
					for index, relative := range d.Remaining {
						ui.Text(c.Key(index), relative).FontSize(14).TextColor(p.MutedForeground)
					}
				})
			}
			if !d.Upload {
				ui.Column(c.Key("destination")).Gap(4).Children(func() {
					ui.Text(c, i18n.T(c, "files_paste_directory")).FontSize(14).TextColor(p.MutedForeground)
					directory := path.Dir(d.Destination)
					if directory == "." {
						directory = i18n.T(c, "files_worktree_root")
					}
					ui.Text(c, directory).FontSize(14)
				})
			}
			if !d.Upload {
				ui.TextInput(c.Key("path"), &d.Destination).Label(i18n.T(c, "files_paste_directory")).FillWidth().Disabled(d.Status != EntryReady)
			}
			if d.Move && d.Status == EntryReady {
				ui.Text(c, i18n.T(c, "files_move_recycle")).FontSize(14).TextColor(p.MutedForeground)
			}
			if d.Status == EntryRunning || d.Upload && d.Status == EntryDone {
				progress := -1.0
				key := "files_paste_copying"
				if d.Upload {
					key = d.Phase
					if d.Status == EntryDone || key == "files_upload_sending" {
						progress = 1
						if d.Status != EntryDone && d.Size > 0 {
							progress = float64(d.Copied) / float64(d.Size)
						}
					}
				}
				ui.Progress(c.Key("progress"), progress).FillWidth().Height(4).Label(title)
				if d.Status == EntryRunning {
					ui.Text(c, i18n.T(c, key)).FontSize(14).TextColor(p.MutedForeground)
				}
			}
		})
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Padding(12, 0, 0, 0).Children(func() {
			cancel := d.Status == EntryReady || d.Status == EntryRunning && (d.Cancelable || len(d.Remaining) > 0)
			label := i18n.T(c, "files_download_close")
			if cancel {
				label = i18n.T(c, "settings_cancel")
				if d.Status == EntryRunning && !d.Cancelable {
					label = i18n.T(c, "files_cancel_remaining")
				}
			}
			kit.Secondary(c.Key("close"), label).Disabled(d.Canceling).OnClick(func() {
				if cancel {
					if actions.Cancel != nil {
						actions.Cancel()
					}
				} else {
					d.Open = false
				}
			})
			if d.Actionable() {
				label := title
				if d.Status == EntryUncertain {
					label = i18n.T(c, "files_check_result")
				} else if d.Status == EntryExists {
					label = i18n.T(c, "files_upload_replace")
				} else if d.Status == EntryFailed || d.Status == EntryCancelled {
					label = i18n.T(c, "files_upload_retry")
				}
				kit.Primary(c.Key("start"), label, false).Children(func() { ui.Text(c, label).FontSize(14) }).OnClick(func() {
					if d.Status == EntryUncertain {
						if actions.Check != nil {
							actions.Check()
						}
					} else if d.Status == EntryExists {
						if actions.Replace != nil {
							actions.Replace()
						}
					} else if actions.Start != nil {
						actions.Start(d.Destination)
					}
				})
			}
		})
	}).Width(min(508, w-48)).Label(title)
	if open && !d.Open {
		if actions.Close != nil {
			actions.Close()
		}
	}
}
