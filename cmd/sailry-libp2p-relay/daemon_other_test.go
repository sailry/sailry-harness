//go:build !linux || android

package main

import (
	"io"
	"strings"
	"testing"
)

func TestDaemonUnsupportedPlatform(t *testing.T) {
	for _, args := range [][]string{{"status"}, {"install", "--config", "not-opened.toml"}, {"update"}} {
		err := runDaemon(t.Context(), args, io.Discard, io.Discard)
		if err == nil || !strings.Contains(err.Error(), "requires Linux with systemd") || !strings.Contains(err.Error(), "use start") {
			t.Fatalf("daemon %v: %v", args, err)
		}
	}
}
