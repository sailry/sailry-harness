package hostctl

import (
	"bufio"
	"bytes"
	"context"
	"strings"
	"testing"
)

func TestHostCLIHelpVersionAndServiceFlags(t *testing.T) {
	t.Parallel()
	serviceCalls := 0
	service := func(_ context.Context, args []string) (int, error) {
		serviceCalls++
		if strings.Join(args, " ") != "--data-dir /fixture --internet" {
			t.Fatalf("service args = %q", args)
		}
		return 0, nil
	}
	for _, test := range []struct {
		args []string
		want string
	}{
		{nil, "Commands:"},
		{[]string{"help"}, "Commands:"},
		{[]string{"version"}, "Sailry Host "},
	} {
		var output bytes.Buffer
		code, err := RunCLI(t.Context(), test.args, strings.NewReader(""), &output, &output, CLIHandlers{Service: service})
		if err != nil || code != 0 || !strings.Contains(output.String(), test.want) {
			t.Fatalf("RunCLI(%q) = %d, %v, %q", test.args, code, err, output.String())
		}
	}
	code, err := RunCLI(t.Context(), []string{"--data-dir", "/fixture", "--internet"}, strings.NewReader(""), &bytes.Buffer{}, &bytes.Buffer{}, CLIHandlers{Service: service})
	if err != nil || code != 0 || serviceCalls != 1 {
		t.Fatalf("service flags = %d, %v; calls=%d", code, err, serviceCalls)
	}
}

func TestRunMenuSelectsDefaultAndNamedCommands(t *testing.T) {
	t.Parallel()
	for _, row := range []struct{ input, want string }{{"\n", "link"}, {"restart\n", "restart"}, {"0\n", ""}} {
		var output bytes.Buffer
		selected, err := RunMenu(t.Context(), bufio.NewReader(strings.NewReader(row.input)), &output)
		if err != nil || selected != row.want || !strings.Contains(output.String(), "Connect a device") {
			t.Fatalf("RunMenu(%q) = %q, %v", row.input, selected, err)
		}
	}
}

func TestLinkDispatcherPreservesTerminalStateAcrossBufferedInput(t *testing.T) {
	t.Parallel()
	arguments := []string{"link", "--data-dir", t.TempDir()}
	_, err := runCLI(t.Context(), arguments, bufio.NewReader(strings.NewReader("")), true, &bytes.Buffer{}, &bytes.Buffer{}, CLIHandlers{})
	if err == nil || strings.Contains(err.Error(), "requires a terminal") {
		t.Fatalf("interactive link was treated as redirected input: %v", err)
	}
	_, err = runCLI(t.Context(), arguments, bufio.NewReader(strings.NewReader("")), false, &bytes.Buffer{}, &bytes.Buffer{}, CLIHandlers{})
	if err == nil || !strings.Contains(err.Error(), "requires a terminal") {
		t.Fatalf("noninteractive link did not require explicit relay flags: %v", err)
	}
}
