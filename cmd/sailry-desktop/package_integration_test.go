//go:build integration

package main

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/localmodels/decision"
	"github.com/sailry/sailry-harness/internal/localmodels/onnx"
	"github.com/sailry/sailry-harness/internal/node/computer"
	"github.com/sailry/sailry-harness/internal/node/python"
	"github.com/sailry/sailry-harness/internal/speech"
	"github.com/sailry/sailry-harness/internal/speech/capture"
)

// TestPackagedNativeLayout runs the production loaders from inside a
// packaged application: compile this test with `go test -c -tags
// integration`, copy the binary beside the packaged executable and run it
// there with SAILRY_PACKAGE_PROBE=1. No window, device, model or Node opens.
func TestPackagedNativeLayout(t *testing.T) {
	if os.Getenv("SAILRY_PACKAGE_PROBE") != "1" {
		t.Skip("set SAILRY_PACKAGE_PROBE=1 and run beside the packaged executable")
	}
	directory, err := onnx.Directory()
	if err != nil {
		t.Fatal(err)
	}
	t.Logf("native directory: %s", directory)
	// A probe linked like a release (-X …/internal/core.Default) takes the
	// packaged-only path: sealed libraries, no SAILRY_IROH_LIB.
	t.Logf("release build: %v", core.Release())
	if err := onnx.Initialize(); err != nil {
		t.Errorf("onnxruntime: %v", err)
	}
	if err := speech.Initialize(); err != nil {
		t.Errorf("sherpa-onnx: %v", err)
	}
	if err := capture.Load(directory, core.Release()); err != nil {
		t.Errorf("capture: %v", err)
	}
	if err := decision.Available(); err != nil {
		t.Errorf("llama.cpp runtime: %v", err)
	}
	if core.Release() {
		t.Setenv("SAILRY_IROH_LIB", filepath.Join(t.TempDir(), "ignored"))
	}
	if err := iroh.LoadDefault(!core.Release()); err != nil {
		t.Errorf("iroh: %v", err)
	}
	if helper, err := computer.Executable(); err != nil {
		t.Errorf("computer helper: %v", err)
	} else {
		t.Logf("computer helper: %s", helper)
	}
	if bundled, err := python.BundledRuntime(); err != nil {
		t.Errorf("python: %v", err)
	} else if output, err := exec.Command(bundled.Python, "-I", "-B", "-c", "import sys; print(sys.version.split()[0])").Output(); err != nil ||
		strings.TrimSpace(string(output)) != python.Version {
		t.Errorf("python interpreter: %q, %v", output, err)
	}
	if terminal, err := filepath.Glob(filepath.Join(directory, "*ghostty-vt*")); err != nil || len(terminal) != 1 {
		t.Errorf("libghostty-vt: %v, %v", terminal, err)
	}
}
