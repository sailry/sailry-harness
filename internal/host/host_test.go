package host

import (
	"slices"
	"testing"
	"time"
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
}

// The policy's validation is atomic, and a stale editor changes nothing
// (model/tunnels.rs validation_and_replacement).
func TestTunnelPolicy(t *testing.T) {
	tn := &Tunnels{Ports: []int{3000, 5173}, Editable: true, Loaded: true, Entries: []Tunnel{{0, 3000, 3000}, {1, 5173, 5173}}}
	orig := slices.Clone(tn.Ports)
	for _, bad := range []string{"0", "65536", "-1", "3000,no", "1.5", "3000;5173"} {
		if tn.SavePorts(orig, bad) != "tunnel_invalid_ports" || !slices.Equal(tn.Ports, orig) {
			t.Errorf("%q was taken", bad)
		}
	}
	if tn.SavePorts(orig, "8080, 3000 3000") != "" || !slices.Equal(tn.Ports, []int{3000, 8080}) {
		t.Errorf("ports %v", tn.Ports)
	}
	if tn.SavePorts(orig, "1") != "tunnel_policy_changed" {
		t.Error("a stale editor saved")
	}
	tn.Close(Tunnel{0, 3000, 9999})
	tn.Close(Tunnel{1, 5173, 5173})
	if len(tn.Entries) != 1 {
		t.Errorf("entries %v", tn.Entries)
	}
	ro := &Tunnels{Ports: []int{1}, Loaded: true}
	if ro.SavePorts([]int{1}, "2") != "tunnel_policy_changed" {
		t.Error("a read-only policy saved")
	}
}

// Each phase offers its action; a stale click does nothing
// (model/updates.rs phases_and_retries).
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
