package main

import (
	"os"
	"os/exec"
	"runtime/debug"
	"testing"

	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/identity"
	desktopsdk "github.com/sailry/sailry-harness/sdk/desktop"
)

func TestDesktopPreservesRuntimeHeapPolicy(t *testing.T) {
	const helper = "SAILRY_TEST_HEAP_STARTUP"
	if os.Getenv(helper) != "1" {
		for _, limit := range []string{"", "768MiB"} {
			t.Run(map[bool]string{true: "default", false: "explicit"}[limit == ""], func(t *testing.T) {
				t.Setenv("GOMEMLIMIT", limit)
				self, err := os.Executable()
				if err != nil {
					t.Fatal(err)
				}
				command := exec.Command(self, "-test.run=^TestDesktopPreservesRuntimeHeapPolicy$")
				command.Env = append(os.Environ(), helper+"=1")
				if output, err := command.CombinedOutput(); err != nil {
					t.Fatalf("startup changed runtime heap policy: %v\n%s", err, output)
				}
			})
		}
		return
	}

	before := debug.SetMemoryLimit(-1)
	if err := desktopsdk.Run([]string{"--invalid-heap-policy-test-flag"}, desktopsdk.Options{Identity: identity.OfficialDesktop(), Assembly: desktop.Harness()}); err == nil {
		t.Fatal("invalid startup arguments did not stop before opening a profile")
	}
	if after := debug.SetMemoryLimit(-1); after != before {
		t.Fatalf("startup replaced the runtime memory limit: got %d, want %d", after, before)
	}
}
