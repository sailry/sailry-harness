package wsl

import (
	"bytes"
	"context"
	"errors"
	"runtime"
	"slices"
	"strings"
	"testing"
	"unicode/utf16"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func encodeUTF16(text string, mark, big bool) []byte {
	var body []byte
	if mark && big {
		body = []byte{0xfe, 0xff}
	} else if mark {
		body = []byte{0xff, 0xfe}
	}
	for _, unit := range utf16.Encode([]rune(text)) {
		if big {
			body = append(body, byte(unit>>8), byte(unit))
		} else {
			body = append(body, byte(unit), byte(unit>>8))
		}
	}
	return body
}

func faultCode(err error) protocol.ErrorCode {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return fault.Code
	}
	return ""
}

func installRequest(distribution string) protocol.Request {
	return protocol.NewRequest(protocol.NodeID{1}, protocol.Command{Kind: "install_wsl_host", Data: &protocol.CommandInstallWslHostPayload{Distribution: distribution}})
}

func TestWSLDiscovery(t *testing.T) {
	t.Parallel()
	t.Run("decodes names without losing Unicode or spaces", func(t *testing.T) {
		for name, body := range map[string][]byte{
			"UTF-16LE with trailing null": encodeUTF16("Ubuntu\r\n资料 Linux\r\nUbuntu\r\n\x00", false, false),
			"UTF-16LE with mark":          encodeUTF16("Ubuntu\r\n资料 Linux\r\nUbuntu\r\n", true, false),
			"UTF-16BE with mark":          encodeUTF16("Ubuntu\r\n资料 Linux\r\nUbuntu\r\n", true, true),
			"UTF-8":                       []byte("Ubuntu\n资料 Linux\nUbuntu\n"),
			"UTF-8 with mark":             []byte("\uFEFFUbuntu\n资料 Linux\n"),
			"UTF-16LE without mark":       encodeUTF16("Ubuntu\r\n资料 Linux\r\n", false, false),
		} {
			names, err := decodeDistributions(body)
			if err != nil || !slices.Equal(names, []string{"Ubuntu", "资料 Linux"}) {
				t.Errorf("%s: names %q, error %v", name, names, err)
			}
		}
		if names, err := decodeDistributions(encodeUTF16("Distro 😀\r\n", false, false)); err != nil || !slices.Equal(names, []string{"Distro 😀"}) {
			t.Errorf("supplementary plane: names %q, error %v", names, err)
		}
		if names, err := decodeDistributions(nil); err != nil || len(names) != 0 {
			t.Errorf("empty output: names %q, error %v", names, err)
		}
	})
	t.Run("rejects malformed lists", func(t *testing.T) {
		for name, body := range map[string][]byte{
			"odd UTF-16 length":       {0xff, 0xfe, 0x41},
			"interior null":           encodeUTF16("Ubuntu\x00Other\r\n", false, false),
			"unpaired high surrogate": {0x00, 0xd8},
			"unpaired low surrogate":  {0x41, 0x00, 0x00, 0xdc},
			"invalid UTF-8":           {0xff},
			"control character":       []byte("Ubuntu\tDebian\n"),
			"bare carriage return":    []byte("Ubuntu\rDebian\n"),
		} {
			_, err := decodeDistributions(body)
			var fault *protocol.Fault
			if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeUnavailable || fault.Message != "WSL discovery distribution list is malformed" {
				t.Errorf("%s: error %v", name, err)
			}
		}
	})
	t.Run("bounds retained output", func(t *testing.T) {
		stopped := 0
		output := &capture{limit: discoveryLimit, overflow: func() { stopped++ }}
		if count, err := output.Write(bytes.Repeat([]byte{'a'}, discoveryLimit)); err != nil || count != discoveryLimit || stopped != 0 {
			t.Fatalf("limit write: count %d, error %v, stopped %d", count, err, stopped)
		}
		if _, err := output.Write([]byte{'a'}); !errors.Is(err, errOverflow) || stopped != 1 || output.buffer.Len() != discoveryLimit {
			t.Fatalf("overflow write: error %v, stopped %d, length %d", err, stopped, output.buffer.Len())
		}
	})
	t.Run("validates distribution names", func(t *testing.T) {
		for _, name := range []string{"Ubuntu", "Ubuntu 资料", "Distro ' $; &", strings.Repeat("x", 256)} {
			if err := validate(name); err != nil {
				t.Errorf("%q: %v", name, err)
			}
		}
		for _, name := range []string{"", "--install", "-d", "Ubuntu\nDebian", "Ubuntu\x00Debian", "Ubuntu\u0085", strings.Repeat("x", 257)} {
			if err := validate(name); faultCode(err) != protocol.ErrorCodeInvalidRequest {
				t.Errorf("%q: %v", name, err)
			}
		}
		service := NewService()
		if _, err := service.Begin(context.Background(), protocol.NodeID{1}, installRequest("--install")); faultCode(err) != protocol.ErrorCodeInvalidRequest {
			t.Errorf("begin accepted an option-like name: %v", err)
		}
	})
	t.Run("deploys only to WSL2", func(t *testing.T) {
		distributions := []protocol.WslDistribution{{Name: "Legacy", Version: 1}, {Name: "Ubuntu 资料", Version: 2}, {Name: "Future", Version: 3}}
		if err := deployable(distributions, "Ubuntu 资料"); err != nil {
			t.Error(err)
		}
		for _, name := range []string{"Legacy", "Future"} {
			var fault *protocol.Fault
			if err := deployable(distributions, name); !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeUnavailable || fault.Message != "Host requires a WSL2 distribution" {
				t.Errorf("%s: %v", name, err)
			}
		}
		for _, name := range []string{"Removed", "ubuntu 资料"} {
			if err := deployable(distributions, name); faultCode(err) != protocol.ErrorCodeNotFound {
				t.Errorf("%s: %v", name, err)
			}
		}
	})
	t.Run("scopes progress and cancellation", func(t *testing.T) {
		service := NewService()
		caller, other := protocol.NodeID{1}, protocol.NodeID{2}
		request := installRequest("Ubuntu 资料")
		closed, shutdown := context.WithCancel(context.Background())
		defer shutdown()
		installation, err := service.Begin(closed, caller, request)
		if err != nil {
			t.Fatal(err)
		}
		if progress, err := service.Progress(caller, request.ID, nil); err != nil || progress.Kind != "connecting" {
			t.Fatalf("initial progress %v, error %v", progress, err)
		}
		if _, err := service.Progress(other, request.ID, nil); faultCode(err) != protocol.ErrorCodeNotFound {
			t.Fatalf("another caller read progress: %v", err)
		}
		if err := service.Cancel(other, request.ID, nil); err != nil || installation.ctx.Err() != nil {
			t.Fatalf("another caller cancelled: %v", err)
		}
		duplicate := installRequest("ubuntu 资料")
		if _, err := service.Begin(closed, other, duplicate); faultCode(err) != protocol.ErrorCodeConflict {
			t.Fatalf("duplicate distribution: %v", err)
		}
		installation.stage("installing")
		if progress, _ := service.Progress(caller, request.ID, nil); progress.Kind != "installing" {
			t.Fatalf("progress %v", progress)
		}
		if err := service.Cancel(caller, request.ID, nil); err != nil || installation.ctx.Err() == nil {
			t.Fatalf("owner cancellation: %v", err)
		}
		installation.Close()
		if _, err := service.Progress(caller, request.ID, nil); faultCode(err) != protocol.ErrorCodeNotFound {
			t.Fatalf("closed installation stayed active: %v", err)
		}
		installation, err = service.Begin(closed, caller, request)
		if err != nil {
			t.Fatal(err)
		}
		defer installation.Close()
		shutdown()
		if installation.ctx.Err() == nil {
			t.Fatal("Node shutdown did not stop the installation")
		}
	})
	t.Run("preserves plugin scope", func(t *testing.T) {
		scope := func(name string) *protocol.PluginContext {
			value := protocol.DefaultPluginContext()
			value.Package = protocol.PluginReference{Name: name, Digest: strings.Repeat("a", 64), SettingsRevision: 1}
			return &value
		}
		service := NewService()
		caller := protocol.NodeID{1}
		request := installRequest("Ubuntu")
		request.Plugin = scope("fixture")
		installation, err := service.Begin(context.Background(), caller, request)
		if err != nil {
			t.Fatal(err)
		}
		defer installation.Close()
		if _, err := service.Progress(caller, request.ID, scope("fixture")); err != nil {
			t.Fatal(err)
		}
		if _, err := service.Progress(caller, request.ID, scope("other")); faultCode(err) != protocol.ErrorCodeNotFound {
			t.Fatalf("another scope read progress: %v", err)
		}
		if err := service.Cancel(caller, request.ID, scope("other")); faultCode(err) != protocol.ErrorCodePermissionDenied || installation.ctx.Err() != nil {
			t.Fatalf("another scope cancelled: %v", err)
		}
	})
	t.Run("builds launch arguments and release script", func(t *testing.T) {
		name := "Ubuntu 资料 ' & $(unexpected)"
		if arguments := launchArguments(name); !slices.Equal(arguments, []string{"--distribution", name, "--cd", "~", "--exec", "/bin/sh", "-s"}) {
			t.Fatalf("arguments %q", arguments)
		}
		for _, resource := range []string{preflightSource, releaseSource} {
			expected := strings.ReplaceAll(resource, "\r\n", "\n")
			if strings.Contains(expected, "\r") || normalize(strings.ReplaceAll(expected, "\n", "\r\n")) != expected {
				t.Fatal("embedded script does not normalize")
			}
		}
		if normalize("set -eu\r\nprintf '%s' 'one\rtwo'\r\n") != "set -eu\nprintf '%s' 'one\rtwo'\n" {
			t.Fatal("literal carriage return was changed")
		}
		relay := "https://relay.example/it's\r\nnext"
		custom := protocol.NetworkSelection{Kind: "custom", Data: &protocol.NetworkSelectionCustomPayload{Relays: []string{relay}}}
		script, err := ReleaseScript("0.1.0", false, &custom)
		prefix := "install_version='0.1.0'\ninstall_update=0\nset -- '--relay' 'https://relay.example/it'\"'\"'s\r\nnext'\n"
		if err != nil || script != prefix+normalize(releaseSource) {
			t.Fatalf("custom script %q, error %v", script, err)
		}
		script, err = ReleaseScript("0.1.0", false, &protocol.NetworkSelection{Kind: "iroh"})
		if err != nil || !strings.HasPrefix(script, "install_version='0.1.0'\ninstall_update=0\nset -- '--internet'\n") {
			t.Fatalf("iroh script %q, error %v", script, err)
		}
		script, err = ReleaseScript("0.1.0", false, nil)
		if err != nil || !strings.HasPrefix(script, "install_version='0.1.0'\ninstall_update=0\nset -- \nset -eu\n") {
			t.Fatalf("default script %q, error %v", script, err)
		}
		script, err = ReleaseScript("0.1.0", true, nil)
		if err != nil || !strings.HasPrefix(script, "install_version='0.1.0'\ninstall_update=1\nset -- \nset -eu\n") {
			t.Fatalf("update script %q, error %v", script, err)
		}
		custom.Data = &protocol.NetworkSelectionCustomPayload{Relays: []string{"https://relay.example/\x00"}}
		if _, err := ReleaseScript("0.1.0", false, &custom); faultCode(err) != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("null byte relay: %v", err)
		}
	})
	t.Run("is unavailable outside Windows", func(t *testing.T) {
		if runtime.GOOS == "windows" {
			t.Skip("Windows discovery is covered by the integration test")
		}
		service := NewService()
		unavailable := func(err error) bool {
			var fault *protocol.Fault
			return errors.As(err, &fault) && fault.Code == protocol.ErrorCodeUnavailable && fault.Message == "WSL Host installation requires Windows"
		}
		if _, err := service.List(context.Background(), nil); !unavailable(err) {
			t.Fatalf("list: %v", err)
		}
		request := installRequest("Ubuntu")
		installation, err := service.Begin(context.Background(), protocol.NodeID{1}, request)
		if err != nil {
			t.Fatal(err)
		}
		defer installation.Close()
		if _, err := installation.Execute(nil, nil, protocol.NodeID{1}, nil, nil); !unavailable(err) {
			t.Fatalf("install: %v", err)
		}
		if progress, err := service.Progress(protocol.NodeID{1}, request.ID, nil); err != nil || progress.Kind != "detecting" {
			t.Fatalf("progress %v, error %v", progress, err)
		}
	})
}
