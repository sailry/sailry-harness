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
	// install is owned by the UI thread. Each window must build its read-only
	// editors before installation may stop the Node.
	install *updateInstall
}

type updateInstall struct {
	requester *Runtime
	windows   map[*Runtime]uint8 // 0: requested, 1: building locked, 2: built
	flushed   bool
	flushing  bool
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
func (r *Runtime) SetUpdates(updates *Updates, focused func() bool, flush ...func() error) {
	r.updates, r.updatesFocused = updates, focused
	if len(flush) > 0 {
		r.updatesFlush = flush[0]
	}
	updates.mu.Lock()
	updates.windows = append(slices.DeleteFunc(updates.windows, func(window *Runtime) bool {
		window.mu.Lock()
		defer window.mu.Unlock()
		return window.closed
	}), r)
	updates.mu.Unlock()
	if updates.install != nil {
		// A new window has no prior native input surface. Its first frame,
		// which MyGo builds even while hidden, joins the same lock request.
		updates.install.windows[r] = 0
	}
	m := r.App.Settings
	m.Version, m.Build = link.ProductVersion, "settings_build_debug"
	if telemetry.BuildRevision != "" {
		m.Build = "settings_build_release"
	}
	service := updates.Service
	m.UpdateActions = &settings.UpdateActions{Check: service.Check, Download: service.Download, Cancel: service.Cancel, Install: r.requestUpdateInstall, Restart: service.Restart}
}

func (r *Runtime) documentsLocked() bool {
	root := r.localRuntime()
	updates := root.updates
	if updates == nil {
		return false
	}
	if pending := updates.install; pending != nil && pending.windows[root] > 0 {
		return true
	}
	state, _ := updates.Service.Snapshot()
	return updateLocksDocuments(state.Kind)
}

func (r *Runtime) requestUpdateInstall() {
	u := r.updates
	state, _ := u.Service.Snapshot()
	if state.Kind != updater.Ready || u.install != nil {
		return
	}
	u.mu.Lock()
	windows := slices.Clone(u.windows)
	u.mu.Unlock()
	pending := &updateInstall{requester: r, windows: map[*Runtime]uint8{}}
	for _, window := range windows {
		if window.updateWindowClosed() {
			continue
		}
		if window.updateDocumentsPending() {
			r.updateInstallFailure("updates_save_first")
			return
		}
		pending.windows[window] = 0
	}
	u.install = pending
	u.changed()
}

func (r *Runtime) updateWindowClosed() bool {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.closed
}

func (r *Runtime) updateDocumentsPending() bool {
	if len(r.UnsavedDocuments()) > 0 || r.SavingDocuments() {
		return true
	}
	pending := false
	r.workspaces(func(project *workspaceProject) { pending = pending || project.panel.PendingEdits() })
	return pending
}

func (r *Runtime) updateInstallFailure(key string) {
	r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, key)) })
}

func (u *Updates) cancelInstall(pending *updateInstall, key string) {
	if u.install != pending {
		return
	}
	u.install = nil
	pending.requester.updateInstallFailure(key)
	u.changed()
}

// beforeUpdateFrame captures input delivered since the installation request.
// Until its own frame begins, a window remains writable and keeps that input.
func (r *Runtime) beforeUpdateFrame() {
	u := r.updates
	if u == nil || u.install == nil {
		return
	}
	pending := u.install
	if phase, ok := pending.windows[r]; !ok || phase != 0 {
		return
	}
	if r.updateDocumentsPending() {
		u.cancelInstall(pending, "updates_save_first")
		return
	}
	pending.windows[r] = 1
}

func (r *Runtime) afterUpdateFrame() {
	u := r.updates
	if u == nil || u.install == nil {
		return
	}
	pending := u.install
	phase, ok := pending.windows[r]
	if !ok || phase == 0 {
		return
	}
	pending.windows[r] = 2
	u.finishInstall(pending)
	if u.install != pending || pending.flushed {
		return
	}
	r.mu.Lock()
	update := r.update
	r.mu.Unlock()
	if update != nil {
		pending.flushed = true
		// Run after this frame, never recursively build its window. Ordinary
		// windows can finish their requested frames before this fallback.
		update(func() { u.flushInstall(pending) })
	}
}

func (u *Updates) flushInstall(pending *updateInstall) {
	if u.install != pending {
		return
	}
	pending.flushing = true
	for window, phase := range pending.windows {
		if window.updateWindowClosed() || phase == 2 || window.updatesFlush == nil {
			continue
		}
		if err := window.updatesFlush(); err != nil {
			u.cancelInstall(pending, "updates_install_failed")
			return
		}
		if u.install != pending {
			return
		}
		if !window.updateWindowClosed() && pending.windows[window] != 2 {
			// A capture can return the old frame of a visible window. Without
			// an acknowledged lock, abort instead of waiting indefinitely if
			// it becomes hidden before its requested frame is delivered.
			u.cancelInstall(pending, "updates_install_failed")
			return
		}
	}
	pending.flushing = false
	u.finishInstall(pending)
}

func (u *Updates) finishInstall(pending *updateInstall) {
	if u.install != pending || pending.flushing {
		return
	}
	for window, phase := range pending.windows {
		if window.updateWindowClosed() {
			delete(pending.windows, window)
			continue
		}
		if phase != 2 {
			return
		}
		if window.updateDocumentsPending() {
			u.cancelInstall(pending, "updates_save_first")
			return
		}
	}
	u.install = nil
	// Install enters Installing synchronously, so no input can enter between
	// the acknowledged read-only frames and the shared service's lock.
	u.Service.Install()
	u.changed()
}

func updateLocksDocuments(kind updater.Kind) bool {
	return kind == updater.Installing || kind == updater.Restarting || kind == updater.Stopped
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
	if updates.install != nil {
		u.State = settings.UpdateInstalling
	}
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
