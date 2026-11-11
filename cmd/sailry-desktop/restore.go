package main

import (
	"context"
	"log/slog"
	"sync/atomic"
	"time"

	"github.com/egoist/mygo"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/node"
)

// profileRestart switches the Desktop to a profile restored from a backup:
// the restored directory takes the place of the profile in use, which is
// kept beside it, and the application starts again.
type profileRestart struct {
	profile string
	check   func(ctx context.Context, current, restored string) error
	stop    func() error
	swap    func(ctx context.Context, current, restored string, now time.Time) (string, error)
	// main runs on the main thread; failed shows why the switch did not
	// happen and relaunch quits and starts the application again, both there.
	main     func(func())
	failed   func(error)
	relaunch func()
	replaced atomic.Bool
}

func newProfileRestart(backend *desktopBackend, relaunch func()) *profileRestart {
	return &profileRestart{
		profile: backend.Path, check: node.CheckRestoredProfile, stop: backend.Stop, swap: node.SwapProfile,
		main: mygo.RunOnMain, relaunch: relaunch,
		failed: func(err error) {
			_, _ = mygo.Dialog.Message(mygo.MessageOptions{Type: mygo.MessageError,
				Message: i18n.In(i18n.Match(mygo.App.Locale()), "backup_restart_failed"), Detail: err.Error()})
		},
	}
}

// run checks the restored profile while the Node still runs: a profile that
// cannot be used is refused with nothing changed. confirm then asks, on the
// main thread, about unsaved documents and running work before it proceeds.
func (restart *profileRestart) run(restored string, refused func(error), confirm func(proceed func())) {
	go func() {
		if err := restart.check(context.Background(), restart.profile, restored); err != nil {
			refused(err)
			return
		}
		restart.main(func() { confirm(func() { go restart.replace(restored) }) })
	}()
}

// replace stops the Node and swaps the profiles. From the stop on the
// application starts again whatever follows: on the restored profile, or on
// the one it had after a native message says why.
func (restart *profileRestart) replace(restored string) {
	if !restart.replaced.CompareAndSwap(false, true) {
		return
	}
	err := restart.stop()
	if err == nil {
		var kept string
		if kept, err = restart.swap(context.Background(), restart.profile, restored, time.Now()); err == nil {
			slog.Info("Restored profile is in place", "kept", kept)
		}
	}
	restart.main(func() {
		if err != nil {
			slog.Error("Restored profile was not put in place", "error", err.Error())
			restart.failed(err)
		}
		restart.relaunch()
	})
}
