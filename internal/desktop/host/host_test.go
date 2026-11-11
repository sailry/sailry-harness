package host

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
)

func TestFormats(t *testing.T) {
	if Protocol(3, 3) != "v3" || Protocol(1, 3) != "v1-v3" || Protocol(3, 1) != "" || Protocol(0, 3) != "" {
		t.Error("protocol ranges")
	}
	if got := UptimeParts(3*24*time.Hour + 5*time.Hour + 12*time.Minute); len(got) != 2 || got[0][0] != "host_uptime_days" || got[1][0] != "host_uptime_hours" {
		t.Errorf("uptime keeps two units: %v", got)
	}
	if got := UptimeParts(30 * time.Second); len(got) != 1 || got[0][0] != "host_uptime_minutes" || got[0][1] != int64(0) {
		t.Errorf("uptime under a minute: %v", got)
	}
	if UptimeParts(-1) != nil {
		t.Error("unknown uptime")
	}
	if Bytes(1024) != "1.0 KiB" || Bytes(512) != "512 B" || Bytes(-1) != "" {
		t.Error("bytes")
	}
	if Rate(2048, 2*time.Second) != "1.0 KiB" || Rate(1, 0) != "" {
		t.Error("rates")
	}
	if Percent(250) != "250.0%" || Percent(-1) != "" {
		t.Error("percent")
	}
	if _, ok := Usage(10, 0); ok {
		t.Error("usage of an unknown total")
	}
	if v, _ := Usage(20, 10); v != 100 {
		t.Error("usage clamps")
	}
	// A live host's names are its own; the preview's are i18n keys.
	var live, local, preview, process string
	tt := ui.NewTester(func(c *ui.Context) {
		live = (&Host{Live: true, Name: "git"}).label(c, "git")
		local = (&Host{Live: true}).label(c, "local_host_name")
		preview = (&Host{}).label(c, "remote_host_name")
		process = (&Host{}).label(c, "metrics_worker")
	}, 100, 100)
	tt.SetLocale("en")
	tt.Frame()
	if live != "git" || local != i18n.In("en", "local_host_name") || preview != i18n.In("en", "remote_host_name") || process != i18n.In("en", "metrics_worker") {
		t.Errorf("labels: live %q, local %q, preview %q, process %q", live, local, preview, process)
	}
}

// Each phase offers its action; a stale click does nothing.
func TestUpdatePhases(t *testing.T) {
	for _, ph := range []Phase{Checking, Downloading, Verifying, Applying, RestartWait, UpdateLoading} {
		if _, _, ok := (&Update{Phase: ph}).Primary(); ok {
			t.Errorf("%s has an action", ph)
		}
	}
	if _, _, ok := (&Update{Phase: Idle, Note: "n"}).Primary(); ok {
		t.Error("an unavailable update has an action")
	}
	u := &Update{Phase: Idle, Current: "0.1.0", Progress: -1}
	stale := *u
	u.Advance(*u)
	if u.Phase != Available || u.Target == "" {
		t.Fatalf("check: %+v", u)
	}
	u.Advance(stale)
	if u.Phase != Available {
		t.Error("a stale click advanced")
	}
	u.Advance(*u)
	if u.Phase != Staged || u.Progress != 100 {
		t.Errorf("download: %+v", u)
	}
	u.Advance(*u)
	if u.Phase != RestartWait {
		t.Errorf("apply: %+v", u)
	}
	f := &Update{Phase: Downloading, Error: "e", Retry: Status}
	if a, label, ok := f.Primary(); !ok || a != Status || label != "update_retry" {
		t.Error("a failed request retries its own action")
	}
}

func TestAddPort(t *testing.T) {
	ports := []Port{{Remote: 3000, Local: 13000, Listening: true}}
	for _, bad := range [][2]string{{"", ""}, {"0", ""}, {"70000", ""}, {"80", "-1"}, {"80", "x"}} {
		if _, p := AddPort(ports, bad[0], bad[1]); p != "port_invalid" {
			t.Errorf("%v: %q", bad, p)
		}
	}
	if _, p := AddPort(ports, "3000", ""); p != "port_exists" {
		t.Error("a repeated port")
	}
	if x, p := AddPort(ports, "8080", "0"); p != "" || x.Local == 0 || !x.Listening {
		t.Errorf("auto port %+v %q", x, p)
	}
	if x, _ := AddPort(ports, "8080", "9000"); x.Local != 9000 {
		t.Error("the local port")
	}
}
