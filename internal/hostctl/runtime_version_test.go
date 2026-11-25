package hostctl

import (
	"bytes"
	"io"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/identity"
	"github.com/sailry/sailry-harness/internal/link"
)

func TestProductRuntimeVersionDoesNotRequireLinkerFlags(t *testing.T) {
	previous := link.ProductVersion
	t.Cleanup(func() { link.ProductVersion = previous })
	var output bytes.Buffer
	code, err := RunContext(t.Context(), []string{"--version"}, Options{
		Identity: identity.Desktop{Version: "0.1.0"},
		Stdin:    strings.NewReader(""), Stdout: &output, Stderr: io.Discard,
	})
	if err != nil || code != 0 || !strings.Contains(output.String(), "0.1.0") || link.ProductVersion != "0.1.0" {
		t.Fatalf("Host runtime identity = %q, CLI %q, exit %d: %v", link.ProductVersion, output.String(), code, err)
	}
}
