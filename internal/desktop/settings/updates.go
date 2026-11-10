package settings

import (
	"fmt"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
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
	confirm       bool      // the install confirmation
	details       bool      // the failure's details
	shown         UpdateState
	shownFailure  *UpdateFailure
}

// UpdateActions are the real updater's actions. Its state arrives in Updater.
type UpdateActions struct{ Check, Download, Cancel, Install, Restart func() }

// Confirm opens the install confirmation, as the ready toast's action does.
func (u *Updater) Confirm() { u.confirm = true }

// ShowDetails opens the failure's details, as the failure toast's action does.
func (u *Updater) ShowDetails() { u.details = true }

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
			u.Failure = &UpdateFailure{"updates_network_failed", "The preview reaches no release feed: `GET https://sailry.dev/releases/experience.json` was not sent."}
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

// updates is the About page's Updates group: the state's action, Details
// after a failure, Download when one is available, the channel and the
// version or the progress; the ready version and failures toast, and Install
// asks first.
func updates(c *ui.Context, m *Model) {
	u := &m.Updater
	p := theme.Of(c)
	// The real updater reports its own state and toasts in every window.
	live := m.UpdateActions
	if live == nil {
		if u.advance(c.Now(), m.Channel == 1) {
			c.After(50 * time.Millisecond)
		}
		u.notify(c)
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
	kit.Group(c, "updates", i18n.T(c, "updates"), "", func() {
		if u.Failure != nil {
			kit.Secondary(c.Key("details"), i18n.T(c, "updates_details")).OnClick(func() { u.details = true })
		}
		if u.State == UpdateAvailable {
			iconButton(c, "download", nil, i18n.T(c, "updates_download"), true, false).OnClick(run(func(a *UpdateActions) func() { return a.Download }, func() { u.start(UpdateDownloading, c.Now()) }))
		}
		switch u.State {
		case UpdateIdle, UpdateAvailable:
			kit.Secondary(c.Key("check"), i18n.T(c, "updates_check")).OnClick(run(func(a *UpdateActions) func() { return a.Check }, func() { u.start(UpdateChecking, c.Now()) }))
		case UpdateChecking, UpdateDownloading:
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(run(func(a *UpdateActions) func() { return a.Cancel }, func() { u.start(UpdateIdle, c.Now()) }))
		case UpdateReady:
			iconButton(c, "install", nil, i18n.T(c, "updates_install"), true, false).OnClick(func() { u.confirm = true })
		case UpdateInstalling, UpdateRestarting:
			iconButton(c, "installing", nil, i18n.T(c, "updates_installing"), false, true)
		case UpdateStopped:
			// Relaunching leaves the preview.
			kit.Secondary(c.Key("restart"), i18n.T(c, "updates_restart")).OnClick(run(func(a *UpdateActions) func() { return a.Restart }, func() { notice(c) }))
		}
	}, func() {
		kit.Card(c, func() {
			kit.Row(c, "channel", i18n.T(c, "updates_channel"), "", func() {
				pick(c, "channel", i18n.T(c, "updates_channel"), &m.Channel,
					labels(c, []string{"updates_channel_stable", "updates_channel_experience"}), u.Locked()).Width(192)
			})
			kit.Row(c, "version", i18n.T(c, "updates_version"), "", func() {
				status := func(s string) { ui.Text(c, s).FontSize(14).Selectable().TextColor(p.MutedForeground) }
				switch u.State {
				case UpdateIdle:
					status(m.Version)
				case UpdateChecking:
					status(i18n.T(c, "updates_checking"))
				case UpdateAvailable, UpdateReady:
					status(u.Version)
				case UpdateInstalling:
					status(i18n.T(c, "updates_installing"))
				case UpdateRestarting:
					status(i18n.T(c, "updates_restarting"))
				case UpdateStopped:
					status(i18n.T(c, "updates_restart_needed"))
				case UpdateDownloading:
					ui.Column(c.Key("download")).Width(224).Gap(4).Children(func() {
						v := u.Percent() / 100
						if u.Verifying {
							v = -1
						}
						ui.Progress(c.Key("bar"), v).FillWidth().Height(6).Label(i18n.T(c, "updates_download"))
						label := fmt.Sprintf("%.0f%%", u.Percent())
						if u.Verifying {
							label = i18n.T(c, "updates_verifying")
						}
						ui.Text(c, label).FontSize(14).TextColor(p.MutedForeground)
					})
				}
			})
		})
	})
	if confirm(c, "install-confirm", &u.confirm, i18n.T(c, "updates_install"), i18n.T(c, "updates_restart_description"), i18n.T(c, "updates_install")) {
		run(func(a *UpdateActions) func() { return a.Install }, func() { u.start(UpdateInstalling, c.Now()) })()
	}
	if u.Failure != nil {
		kit.Details(c, &u.details, i18n.T(c, "updates_details"), "", u.Failure.Detail)
	}
}

// notify toasts what the updater reached, once: the ready version with
// Install, a failure with Details.
func (u *Updater) notify(c *ui.Context) {
	if u.State == UpdateReady && u.shown != UpdateReady {
		version := u.Version
		c.AddToast(ui.Toast{Type: "success", Title: i18n.T(c, "updates_ready", "version", version), Timeout: kit.ToastDuration(c), Action: i18n.T(c, "updates_install"),
			OnAction: func() {
				if u.State == UpdateReady && u.Version == version {
					u.confirm = true
				}
			}})
	}
	if f := u.Failure; f != nil && f != u.shownFailure {
		c.AddToast(ui.Toast{Type: "error", Title: i18n.T(c, f.Key), Timeout: -1, Action: i18n.T(c, "updates_details"),
			OnAction: func() { u.details = true }})
	}
	u.shown, u.shownFailure = u.State, u.Failure
}
