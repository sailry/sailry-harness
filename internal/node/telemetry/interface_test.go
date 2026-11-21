package telemetry

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
)

func TestNativeProbeAnnotatesEveryHangOutcome(t *testing.T) {
	for _, tc := range []struct {
		status, heartbeat string
		replied, measured bool
	}{
		{"replied", "pending", true, true},
		{"replied", "recovered", true, true},
		{"timeout", "pending", false, true},
		{"unavailable", "pending", false, false},
	} {
		t.Run(tc.status+"/"+tc.heartbeat, func(t *testing.T) {
			dir := t.TempDir()
			w := NewWatchdog(WatchdogOptions{Directory: dir, ProbeNativeWindow: func() (bool, bool) { return tc.replied, tc.measured }})
			w.waiting = tc.heartbeat == "pending"
			w.hang()
			raw, err := os.ReadFile(filepath.Join(dir, HangFile))
			if err != nil {
				t.Fatal(err)
			}
			if !bytes.HasPrefix(raw, []byte("native_window."+tc.status+"\nheartbeat."+tc.heartbeat+"\ngoroutine ")) {
				t.Fatal("missing native dispatch outcome or goroutines")
			}
			assertReduced(t, raw)
		})
	}
}
