package application

import (
	"testing"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
)

func TestInterfaceReportsPauseAndExtendHelpers(t *testing.T) {
	var nilReports *interfaceReports
	nilReports.pause()()
	nilReports.extend(time.Second)()
	callPause(nil)()

	empty := &interfaceReports{}
	empty.pause()()
	empty.extend(time.Second)()

	w := telemetry.NewWatchdog(telemetry.WatchdogOptions{
		Directory: t.TempDir(),
		Clock:     clockwork.NewFakeClock(),
		Ping:      func(done func()) { done() },
	})
	defer w.Close()

	reports := &interfaceReports{watchdog: w}
	called := 0
	callPause(func() func() {
		called++
		return reports.pause()
	})()
	if called != 1 {
		t.Fatalf("callPause invoked callback %d times, want 1", called)
	}
	reports.extend(firstFrameGrace)()
}
