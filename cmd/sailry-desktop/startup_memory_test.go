package main

import (
	"os"
	"os/exec"
	"runtime/debug"
	"testing"
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
	os.Args = []string{os.Args[0], "--invalid-heap-policy-test-flag"}
	if err := run(); err == nil {
		t.Fatal("invalid startup arguments did not stop before opening a profile")
	}
	if after := debug.SetMemoryLimit(-1); after != before {
		t.Fatalf("startup replaced the runtime memory limit: got %d, want %d", after, before)
	}
}
