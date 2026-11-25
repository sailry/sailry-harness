package telemetry

import (
	"bytes"
	"testing"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHostCrashUsesFaultingWorker(t *testing.T) {
	remote := &endpoints{}
	c := New(Options{Application: "host", Lookup: func(k string) string {
		if k == "SAILRY_SENTRY_DSN" {
			return "http://public@reports.invalid/42"
		}
		return ""
	}, Transport: remote, Timer: remote.timer})
	c.Configure(protocol.TelemetrySettings{Errors: true, InstallationID: uuid.New()})
	raw := "panic: " + plantedSecret + "\n\ngoroutine 73 [running]:\nmain.worker()\n\t" + plantedPath + ":12 +0xf\n\ngoroutine 1 [select]:\nmain.idle()\n\t/main.go:21 +0xa\n"
	c.ReportCrash(&Crash{reduced: reduce([]byte(raw))})
	c.Close()
	_, reports := remote.sent()
	if len(reports) != 1 {
		t.Fatalf("got %d crash reports", len(reports))
	}
	assertClean(t, "host crash report", reports[0])
	if !bytes.Contains(reports[0], []byte(`"host.crash:panic.other"`)) || !bytes.Contains(reports[0], []byte(`"function":"worker"`)) || bytes.Contains(reports[0], []byte(`"function":"idle"`)) {
		t.Fatal("report did not use the faulting Host worker stack")
	}
}
