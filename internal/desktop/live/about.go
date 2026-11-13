package live

import (
	"path/filepath"
	"slices"
	"sync"
	"sync/atomic"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/updater"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
)

// Settings › About: the running build and the process's one updater, which
// every window shows.

// Updates is the process's update service as its windows share it.
type Updates struct {
	Service *updater.Service
	channel atomic.Int32 // settings' channel index the service checks for
	mu      sync.Mutex
	windows []*Runtime
}

// NewUpdates starts no work: Service.Startup begins the launch check. The
// updater keeps its staged downloads beside the preferences; profile is the
// data directory an installation must not replace, stop ends the Node before
// the bundle is swapped and quit ends the process for the relaunch.
func NewUpdates(prefs *preferences.Store, profile string, stop func() error, quit func()) *Updates {
	updates := &Updates{}
	updates.channel.Store(int32(updateChannel(prefs.Data.UpdateChannel, link.ProductVersion)))
	updates.Service = updater.New(updater.Options{
		Version: link.ProductVersion, Directory: filepath.Join(filepath.Dir(prefs.Path), "updater"), Profile: profile,
		Channel: func() updater.Channel {
			if updates.channel.Load() == 1 {
				return updater.Experience
			}
			return updater.Stable
		},
		Stop: stop, Quit: quit, Changed: updates.changed,
	})
	return updates
}

// changed redraws every open window from the service's goroutines.
func (u *Updates) changed() {
	u.mu.Lock()
	windows := slices.Clone(u.windows)
	u.mu.Unlock()
	for _, window := range windows {
		window.wake()
	}
}

// SetUpdates binds the window to the updater. focused reports whether this
// window is the one that shows the updater's toasts.
func (r *Runtime) SetUpdates(updates *Updates, focused func() bool) {
	r.updates, r.updatesFocused = updates, focused
	updates.mu.Lock()
	updates.windows = append(slices.DeleteFunc(updates.windows, func(window *Runtime) bool {
		window.mu.Lock()
		defer window.mu.Unlock()
		return window.closed
	}), r)
	updates.mu.Unlock()
	m := r.App.Settings
	m.Version, m.Build = link.ProductVersion, "settings_build_debug"
	if telemetry.BuildRevision != "" {
		m.Build = "settings_build_release"
	}
	service := updates.Service
	m.UpdateActions = &settings.UpdateActions{Check: service.Check, Download: service.Download, Cancel: service.Cancel, Install: func() {
		// An installation stops the Node and ends the process, so unsaved
		// documents in any window are saved first.
		updates.mu.Lock()
		windows := slices.Clone(updates.windows)
		updates.mu.Unlock()
		for _, window := range windows {
			window.mu.Lock()
			closed := window.closed
			window.mu.Unlock()
			if !closed && len(window.UnsavedDocuments()) > 0 {
				r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "updates_save_first")) })
				return
			}
		}
		service.Install()
	}, Restart: service.Restart}
}

// updateView is the service's state as the About page shows it. The failure
// keeps its identity until the service reports another.
func updateView(u *settings.Updater, state updater.State, failure *updater.Failure) {
	u.State = map[updater.Kind]settings.UpdateState{
		updater.Idle: settings.UpdateIdle, updater.Checking: settings.UpdateChecking, updater.Available: settings.UpdateAvailable,
		updater.Downloading: settings.UpdateDownloading, updater.Ready: settings.UpdateReady, updater.Installing: settings.UpdateInstalling,
		updater.Restarting: settings.UpdateRestarting, updater.Stopped: settings.UpdateStopped,
	}[state.Kind]
	u.Version = state.Version
	u.Copied, u.Total, u.Verifying = 0, 0, false
	if state.Kind == updater.Downloading {
		u.Copied, u.Total, u.Verifying = int64(state.Progress.Copied), int64(state.Progress.Total), state.Progress.Verifying
	}
	switch {
	case failure == nil:
		u.Failure = nil
	case u.Failure == nil || u.Failure.Key != failure.Key || u.Failure.Detail != failure.Detail:
		u.Failure = &settings.UpdateFailure{Key: failure.Key, Detail: failure.Detail}
	}
}

// updateToast is a notice as a toast. An installation whose result is not
// known warns instead of failing, and names the retained original in Details.
func updateToast(notice updater.Notice, title, action string, timeout time.Duration, run func()) ui.Toast {
	toast := ui.Toast{Type: "success", Title: title, Timeout: timeout}
	if notice.Error {
		toast.Type = "error"
		if notice.Key == "updates_install_uncertain" || notice.Key == "updates_receipt_failed" {
			toast.Type = "warning"
		}
	}
	if notice.Version != "" || notice.Error && notice.Detail != "" {
		toast.Action, toast.OnAction = action, run
	}
	return toast
}

// aboutFrame follows a changed channel, shows the service's state and, in
// the focused window, its outcomes as toasts.
func (r *Runtime) aboutFrame(c *ui.Context) {
	updates := r.updates
	if updates == nil {
		return
	}
	m := r.App.Settings
	service := updates.Service
	if channel := int32(min(max(m.Channel, 0), 1)); updates.channel.Swap(channel) != channel {
		if service.ChannelChanged() {
			service.Check()
		}
	}
	state, failure := service.Snapshot()
	u := &m.Updater
	updateView(u, state, failure)
	if r.updatesFocused != nil && !r.updatesFocused() {
		return
	}
	about := func() {
		m.Open(settings.About)
		r.App.Navigate(shell.SettingsPage)
	}
	for _, notice := range service.Notices() {
		switch {
		case notice.Version != "":
			version := notice.Version
			c.AddToast(updateToast(notice, i18n.T(c, "updates_ready", "version", version), i18n.T(c, "updates_install"), kit.ToastDuration(c), func() {
				if u.State == settings.UpdateReady && u.Version == version {
					about()
					u.Confirm()
				}
			}))
		default:
			c.AddToast(updateToast(notice, i18n.T(c, notice.Key), i18n.T(c, "updates_details"), kit.ToastDuration(c), func() {
				if u.Failure != nil {
					about()
					u.ShowDetails()
				}
			}))
		}
	}
}
