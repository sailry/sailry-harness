//go:build integration

package main

import (
	"context"
	"database/sql"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/localmodels/decision"
	"github.com/sailry/sailry-harness/internal/localmodels/onnx"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/node/computer"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/python"
	"github.com/sailry/sailry-harness/internal/shipped"
	"github.com/sailry/sailry-harness/internal/speech"
	"github.com/sailry/sailry-harness/internal/speech/capture"
	"github.com/sailry/sailry-harness/internal/turso"
)

func TestPackagedColdStartup(t *testing.T) {
	if os.Getenv("SAILRY_PACKAGE_PROBE") != "1" {
		t.Skip("set SAILRY_PACKAGE_PROBE=1 and run beside the packaged executable")
	}
	profile := filepath.Join(t.TempDir(), "profile")
	options := node.Options{
		Network: &link.NetworkScope{Bind: "127.0.0.1:0"},
		MCP: &mcp.Options{Open: func(context.Context, mcp.Launch, func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error) {
			return nil, errors.New("fixture MCP servers are unavailable")
		}},
	}
	for _, phase := range []string{"first launch", "reopen"} {
		t.Run(phase, func(t *testing.T) {
			owner, err := node.StartWithOptions(t.Context(), profile, options)
			if err != nil {
				t.Fatal(err)
			}
			if endpoint := owner.Link(); endpoint == nil {
				t.Error("packaged startup has no Link endpoint")
			} else if address, err := endpoint.Address(t.Context()); err != nil || address.ID != owner.ID() || len(address.Addrs) == 0 {
				t.Errorf("packaged Link endpoint = %v, %v", address, err)
			}
			if err := owner.Stop(t.Context()); err != nil {
				t.Fatal(err)
			}
		})
	}
}

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
	// packaged-only path for loading native libraries against the seal.
	t.Logf("release build: %v", core.Release())
	t.Run("Native seal excludes old Link libraries", func(t *testing.T) {
		seal, err := shipped.Read(directory, runtime.GOOS+"/"+runtime.GOARCH)
		if err != nil {
			t.Fatal(err)
		}
		for _, name := range []string{"libsailry_iroh.dylib", "libsailry_iroh.so", "sailry_iroh.dll"} {
			if _, err := os.Lstat(filepath.Join(directory, name)); !errors.Is(err, os.ErrNotExist) {
				t.Errorf("expected %s to be absent from the package: %v", name, err)
			}
			if _, ok := seal.File(name); ok {
				t.Errorf("old Link library %s is in the seal", name)
			}
		}
	})
	t.Run("Turso FTS", func(t *testing.T) {
		if core.Release() {
			t.Setenv("SAILRY_TURSO_LIB", filepath.Join(t.TempDir(), "ignored"))
		}
		connector, err := turso.Connector(filepath.Join(t.TempDir(), "probe.turso"), false)
		if err != nil {
			t.Fatal(err)
		}
		db := sql.OpenDB(connector)
		defer db.Close()
		ctx := context.Background()
		for _, statement := range []string{
			"CREATE TABLE probe (id INTEGER PRIMARY KEY, body TEXT NOT NULL)",
			"CREATE INDEX probe_fts ON probe USING fts(body) WITH(tokenizer='whitespace')",
			"INSERT INTO probe(body) VALUES ('packaged native search')",
		} {
			if _, err := db.ExecContext(ctx, statement); err != nil {
				t.Fatal(err)
			}
		}
		var count int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM probe WHERE fts_match(body, 'native')").Scan(&count); err != nil || count != 1 {
			t.Fatalf("Turso FTS: count=%d, error=%v", count, err)
		}
	})
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
