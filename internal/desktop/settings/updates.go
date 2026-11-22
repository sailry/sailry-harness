package settings

import (
	"cmp"
	"fmt"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// UpdateState is the app updater's state.
type UpdateState int

const (
	UpdateIdle        UpdateState = iota
	UpdateChecking                // asking the release feed
	UpdateAvailable               // Version can be downloaded
	UpdateDownloading             // Copied of Total, then verifying
	UpdateReady                   // Version is staged; Install restarts
	UpdateInstalling
	UpdateRestarting
	UpdateStopped // installed; Restart is needed
)

// UpdateFailure is the last failure: its message key and the detail Details
// shows.
type UpdateFailure struct{ Key, Detail string }

// Updater is the About page's updater.
type Updater struct {
	State         UpdateState
	Version       string // the version available or ready
	Copied, Total int64
	Verifying     bool
	Failure       *UpdateFailure
	since         time.Time // when the preview's step began
	deferred      string    // the ready version Later folded
	seen          string    // the found version About has shown
	details       bool      // the failure's details
	shown         UpdateState
	shownFailure  *UpdateFailure
}

// UpdateActions are the real updater's actions. Its state arrives in Updater.
type UpdateActions struct{ Check, Download, Cancel, Install, Restart func() }

// Attention reports a found update About has not shown yet, which the
// rail's Settings and About's entry mark.
func (u *Updater) Attention() bool {
	return (u.State == UpdateAvailable || u.State == UpdateReady) && u.Version != u.seen
}

// Review unfolds the ready row.
func (u *Updater) Review() { u.deferred = "" }

// Locked reports whether the channel cannot change now.
func (u *Updater) Locked() bool {
	switch u.State {
	case UpdateChecking, UpdateDownloading, UpdateInstalling, UpdateRestarting, UpdateStopped:
		return true
	}
	return false
}

// Percent is the download's progress, 100 while verifying.
func (u *Updater) Percent() float64 {
	if u.Verifying || u.Total == 0 {
		if u.Verifying {
			return 100
		}
		return 0
	}
	return float64(u.Copied) / float64(u.Total) * 100
}

// Steps of the preview's updater, on the frame clock.
const (
	checkTime    = 1200 * time.Millisecond
	downloadTime = 3 * time.Second
	verifyTime   = 600 * time.Millisecond
	installTime  = time.Second
)

// advance plays the preview's updater (no feed, download or installer
// runs): a check finds 0.2.0-preview on the stable channel and fails to
// reach the feed on the experience one, so both outcomes show; a download
// copies and verifies, then is ready; installing stops at Restart, which
// would relaunch the app. It reports whether a step is still running.
func (u *Updater) advance(now time.Time, experience bool) bool {
	t := now.Sub(u.since)
	switch u.State {
	case UpdateChecking:
		if t < checkTime {
			return true
		}
		if experience {
			u.State = UpdateIdle
			u.Failure = &UpdateFailure{"updates_network_failed", "The preview does not contact a release feed."}
			return false
		}
		u.State, u.Version = UpdateAvailable, "0.2.0-preview"
	case UpdateDownloading:
		u.Total = 48 << 20
		if t < downloadTime {
			u.Copied = u.Total * int64(t) / int64(downloadTime)
			return true
		}
		u.Copied, u.Verifying = u.Total, true
		if t < downloadTime+verifyTime {
			return true
		}
		u.State, u.Verifying = UpdateReady, false
	case UpdateInstalling:
		if t < installTime {
			return true
		}
		u.State = UpdateStopped
	}
	return false
}

// start begins a step at now.
func (u *Updater) start(s UpdateState, now time.Time) {
	u.State, u.since, u.Failure = s, now, nil
	u.Copied, u.Verifying = 0, false
}

// updates is the About page's Updates group: the channel over one status
// row that carries the state's actions. A download's progress spans the
// row; a ready update states what a restart stops and restarts in place,
// so installing needs no dialog. Later folds the ready row to its action.
func updates(c *ui.Context, m *Model) {
	u := &m.Updater
	p := theme.Of(c)
	name := cmp.Or(m.ProductName, i18n.T(c, "app"))
	// The real updater reports its own state and toasts in every window.
	live := m.UpdateActions
	if live == nil {
		if u.advance(c.Now(), m.Channel == 1) {
			c.After(50 * time.Millisecond)
		}
		u.notify(c, name)
	}
	if u.State != UpdateReady || u.Version != u.deferred {
		u.deferred = ""
	}
	if u.Attention() {
		u.seen = u.Version
	}
	// run is an action: the real updater's, or the preview's step.
	run := func(action func(*UpdateActions) func(), preview func()) func() {
		return func() {
			if live != nil {
				action(live)()
			} else {
				preview()
			}
		}
	}
	check := run(func(a *UpdateActions) func() { return a.Check }, func() { u.start(UpdateChecking, c.Now()) })
	cancel := func() {
		kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(run(func(a *UpdateActions) func() { return a.Cancel }, func() { u.start(UpdateIdle, c.Now()) }))
	}
	busy := func(label string) { kit.SecondaryBusy(c.Key("busy"), label, true) }
	mark := func(icon *ui.SVG, color ui.Color) func() {
		return func() { ui.Icon(c, icon).FontSize(16).TextColor(color) }
	}
	// row is the status: an optional mark, the title and a muted line, the
	// actions, and what spans the row below it.
	row := func(lead func(), title, description string, actions, below func()) {
		ui.Column(c.Key("status")).Padding(kit.RowPad, 0).Gap(10).MinHeight(kit.RowMinHeight).Children(func() {
			ui.Row(c).Gap(16).MinHeight(kit.RowMinHeight - 2*kit.RowPad).AlignItems(ui.Center).Children(func() {
				ui.Row(c).Grow(1).Shrink(1).MinWidth(0).Gap(kit.RowIconGap).AlignItems(ui.Center).Children(func() {
					if lead != nil {
						ui.Row(c.Key("lead")).Size(kit.RowIcon, kit.RowIcon).Shrink(0).Center().Children(lead)
					}
					kit.RowText(c, title, description).Grow(1).Shrink(1)
				})
				if actions != nil {
					ui.Row(c).Shrink(0).Gap(8).Children(actions)
				}
			})
			if below != nil {
				below()
			}
		})
	}
	kit.Group(c, "updates", i18n.T(c, "updates"), "", nil, func() {
		kit.Card(c, func() {
			kit.Row(c, "channel", i18n.T(c, "updates_channel"), "", func() {
				pick(c, "channel", i18n.T(c, "updates_channel"), &m.Channel,
					labels(c, []string{"updates_channel_stable", "updates_channel_experience"}), u.Locked()).Width(192)
			})
			version := u.Version
			switch {
			case u.Failure != nil && (u.State == UpdateIdle || u.State == UpdateAvailable):
				row(mark(icons.TriangleAlert, p.Danger), i18n.T(c, u.Failure.Key, "app", name), m.Version, func() {
					kit.Secondary(c.Key("details"), i18n.T(c, "updates_details")).OnClick(func() { u.details = true })
					kit.Secondary(c.Key("check"), i18n.T(c, "updates_check")).OnClick(check)
				}, nil)
			case u.State == UpdateIdle:
				row(nil, i18n.T(c, "updates_version"), m.Version, func() {
					kit.Secondary(c.Key("check"), i18n.T(c, "updates_check")).OnClick(check)
				}, nil)
			case u.State == UpdateChecking:
				row(nil, i18n.T(c, "updates_checking"), m.Version, cancel, nil)
			case u.State == UpdateAvailable:
				row(mark(icons.Download, p.Info), i18n.T(c, "updates_available", "version", version), m.Version, func() {
					iconButton(c, "download", nil, i18n.T(c, "updates_download"), true, false).
						OnClick(run(func(a *UpdateActions) func() { return a.Download }, func() { u.start(UpdateDownloading, c.Now()) }))
				}, nil)
			case u.State == UpdateDownloading:
				title, detail := i18n.T(c, "updates_downloading", "version", version), ""
				if u.Verifying {
					title = i18n.T(c, "updates_verifying")
				} else if u.Total > 0 {
					detail = fmt.Sprintf("%s / %s · %.0f%%", megabytes(u.Copied), megabytes(u.Total), u.Percent())
				}
				row(nil, title, detail, cancel, func() {
					v := u.Percent() / 100
					if u.Verifying {
						v = -1
					}
					ui.Progress(c.Key("bar"), v).FillWidth().Height(6).Label(title)
				})
			case u.State == UpdateReady:
				restart := func() {
					iconButton(c, "install", nil, i18n.T(c, "updates_restart_now"), true, false).
						OnClick(run(func(a *UpdateActions) func() { return a.Install }, func() { u.start(UpdateInstalling, c.Now()) }))
				}
				if u.deferred == version {
					row(mark(icons.CircleCheck, p.Success), i18n.T(c, "updates_ready_now", "version", version), "", restart, nil)
					break
				}
				row(mark(icons.CircleCheck, p.Success), i18n.T(c, "updates_ready_now", "version", version), i18n.T(c, "updates_restart_description"), func() {
					kit.Secondary(c.Key("later"), i18n.T(c, "updates_later")).OnClick(func() { u.deferred = version })
					restart()
				}, nil)
			case u.State == UpdateInstalling:
				row(nil, i18n.T(c, "updates_installing"), version, func() { busy(i18n.T(c, "updates_installing")) }, nil)
			case u.State == UpdateRestarting:
				row(nil, i18n.T(c, "updates_restarting"), version, func() { busy(i18n.T(c, "updates_restarting")) }, nil)
			case u.State == UpdateStopped:
				// Relaunching leaves the preview.
				row(mark(icons.CircleCheck, p.Success), i18n.T(c, "updates_restart_needed"), version, func() {
					iconButton(c, "restart", nil, i18n.T(c, "updates_restart"), true, false).OnClick(run(func(a *UpdateActions) func() { return a.Restart }, func() { notice(c) }))
				}, nil)
			}
		})
	})
	if u.Failure != nil {
		kit.Details(c.Key("details"), &u.details, i18n.T(c, "updates_details"), "", u.Failure.Detail)
	}
}

// megabytes is a byte count in MB with one decimal.
func megabytes(n int64) string { return fmt.Sprintf("%.1f MB", float64(n)/(1<<20)) }

// notify reports the updater's latest outcome once.
func (u *Updater) notify(c *ui.Context, name string) {
	if u.State == UpdateReady && u.shown != UpdateReady {
		version := u.Version
		kit.Toast(c, kit.Done, i18n.T(c, "updates_ready", "version", version))
	}
	if f := u.Failure; f != nil && f != u.shownFailure {
		kit.Toast(c, kit.Failed, i18n.T(c, f.Key, "app", name))
	}
	u.shown, u.shownFailure = u.State, u.Failure
}
