package application

import (
	"log"
	"path/filepath"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
)

// firstFrameGrace covers first-frame GPU shader pipeline compilation (such as
// Metal makePipelines on Intel macOS) while still reporting real deadlocks past 30s.
const firstFrameGrace = 30 * time.Second

// interfaceReports keeps the interface's crash and hang traces beside the
// application's log, and reports them while error reports are on.
type interfaceReports struct {
	directory string
	crash     *telemetry.Crash
	watchdog  *telemetry.Watchdog
	visible   map[*mygo.Window]bool
}

// catchCrashes runs once the log is installed. MyGo has no hook that
// recovers a panic of the interface thread, so a panic still ends the
// process with its traceback; the runtime now also copies that traceback
// into the log directory, and the next launch reduces and reports it.
func catchCrashes() *interfaceReports {
	logs := logging.Directory()
	if logs == "" {
		return nil
	}
	reports := &interfaceReports{directory: filepath.Join(logs, "desktop"), visible: map[*mygo.Window]bool{}}
	var err error
	if reports.crash, err = telemetry.CatchCrashes(reports.directory); err != nil {
		log.Printf("crash traces: %v", err)
	}
	return reports
}

// start reports the previous run's crash and arms the hang watchdog. The
// heartbeat runs on the interface thread without drawing a frame.
func (reports *interfaceReports) start(control *telemetry.Control) {
	if reports == nil {
		return
	}
	control.ReportCrash(reports.crash)
	reports.watchdog = telemetry.NewWatchdog(telemetry.WatchdogOptions{
		Ping: mygo.RunOnMain, ProbeNativeWindow: nativeWindowProbe(), Directory: reports.directory, Control: control,
	})
	mygo.Power.OnSuspend(func() { reports.watchdog.Suspended(true) })
	mygo.Power.OnResume(func() { reports.watchdog.Suspended(false) })
}

// watch follows one window: each frame it starts is interface activity, and
// a hang counts only while some window is on screen.
func (reports *interfaceReports) watch(window *mygo.Window, app *shell.App) {
	if reports == nil || reports.watchdog == nil {
		return
	}
	presented := false
	var endWarmup func()
	before := app.Hooks.BeforeFrame
	app.Hooks.BeforeFrame = func(c *ui.Context) {
		if !presented {
			presented = true
			end := reports.extend(firstFrameGrace)
			endWarmup = end
			reports.watchdog.Touch()
			go mygo.RunOnMain(func() {
				end()
				reports.watchdog.Touch()
			})
		} else {
			reports.watchdog.Touch()
		}
		if before != nil {
			before(c)
		}
	}
	shown := func(visible bool) func() {
		return func() {
			reports.visible[window] = visible
			reports.apply()
		}
	}
	reports.visible[window] = true
	reports.apply()
	window.OnShow(shown(true))
	window.OnRestore(shown(true))
	window.OnHide(shown(false))
	window.OnMinimize(shown(false))
	window.OnClosed(func() {
		if endWarmup != nil {
			endWarmup()
		}
		delete(reports.visible, window)
		reports.apply()
	})
}

func (reports *interfaceReports) pause() func() {
	if reports == nil || reports.watchdog == nil {
		return func() {}
	}
	return reports.watchdog.Pause()
}

func (reports *interfaceReports) extend(limit time.Duration) func() {
	if reports == nil || reports.watchdog == nil {
		return func() {}
	}
	return reports.watchdog.Extend(limit)
}

func callPause(pause func() func()) func() {
	if pause == nil {
		return func() {}
	}
	return pause()
}

func (reports *interfaceReports) apply() {
	visible := false
	for _, shown := range reports.visible {
		visible = visible || shown
	}
	reports.watchdog.Visible(visible)
}

func (reports *interfaceReports) close() {
	if reports != nil && reports.watchdog != nil {
		reports.watchdog.Close()
	}
}
