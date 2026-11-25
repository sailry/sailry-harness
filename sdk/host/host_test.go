package host_test

import (
	"bytes"
	"context"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/sdk/host"
	"github.com/sailry/sailry-harness/sdk/product"
)

func TestHostCLIUsesSuppliedProduct(t *testing.T) {
	t.Parallel()
	profile := filepath.Join(t.TempDir(), "product")
	for _, test := range []struct {
		args []string
		want string
	}{
		{[]string{"version"}, "Sailry Host 9.8.7\n"},
		{[]string{"serve", "--help"}, "Defaults to " + profile},
		{[]string{"help"}, "Commands:"},
	} {
		var stdout, stderr bytes.Buffer
		code, err := host.RunContext(t.Context(), test.args, host.Options{
			Identity: product.Desktop{Version: "9.8.7", DefaultProfile: profile, HostRelease: product.CodeHost()},
			Stdin:    strings.NewReader(""), Stdout: &stdout, Stderr: &stderr,
		})
		if code != 0 || err != nil || !strings.Contains(stdout.String(), test.want) || stderr.Len() != 0 {
			t.Fatalf("Host CLI %q = %d, %v; stdout=%q stderr=%q", test.args, code, err, stdout.String(), stderr.String())
		}
	}
}

func TestHostCLIAllowsProductOwnedAcceptance(t *testing.T) {
	t.Parallel()
	var output bytes.Buffer
	called := false
	code, err := host.RunContext(t.Context(), []string{"acceptance-cleanup", "--list"}, host.Options{
		Stdin: strings.NewReader(""), Stdout: &output, Stderr: &output,
		Acceptance: func(ctx context.Context, args []string) (int, error) {
			called = true
			if ctx != t.Context() || strings.Join(args, " ") != "--cleanup --list" {
				t.Fatalf("acceptance lost its caller context or arguments: %q", args)
			}
			return 7, nil
		},
	})
	if !called || code != 7 || err != nil || output.Len() != 0 {
		t.Fatalf("product acceptance = called %v, code %d, error %v, output %q", called, code, err, output.String())
	}
}
