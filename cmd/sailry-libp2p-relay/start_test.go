package main

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/libp2p/go-libp2p/core/peer"
	"github.com/sailry/sailry-harness/internal/link"
)

const startTestPeer = "12D3KooWQK1wnefoLrcVHbbnf5tLzbopUd3K3bFAoJpA7YJgL5pV"

func TestStartConfigSelectionAndPortOverride(t *testing.T) {
	root := t.TempDir()
	t.Chdir(root)
	defaultDir := filepath.Join(root, "default-data")
	writeStartTestConfig(t, relayConfigName, fmt.Sprintf("mode = \"public\"\ndata_dir = %q\n", defaultDir))
	selectedDir := filepath.Join(root, "selected-data")
	const selectedFile = "selected.toml"
	selected := fmt.Sprintf(`# Preserve the original document.
mode = "private"
data_dir = %q
listen_ip = "::"
port = 4100
announce_ip = "2001:db8::1"
allowed_peers = [%q]
`, selectedDir, startTestPeer)
	writeStartTestConfig(t, selectedFile, selected)

	options, err := parseStartOptions(nil, io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	assertStartOptions(t, options, defaultDir, "/ip4/127.0.0.1/tcp/4001", "", "")
	options, raw, err := readRelayConfig(selectedFile)
	if err != nil || string(raw) != selected {
		t.Fatalf("readRelayConfig did not return the original document: %v", err)
	}
	assertStartOptions(t, options, selectedDir, "/ip6/::/tcp/4100", "/ip6/2001:db8::1/tcp/4100", startTestPeer)
	options, err = parseStartOptions([]string{"--config", selectedFile, "--port=4501"}, io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	assertStartOptions(t, options, selectedDir, "/ip6/::/tcp/4501", "/ip6/2001:db8::1/tcp/4501", startTestPeer)
	if got, err := os.ReadFile(selectedFile); err != nil || string(got) != selected {
		t.Fatalf("port override changed the TOML document: %v", err)
	}

	for _, args := range [][]string{{"--public"}, {"--config", selectedFile, "--data-dir", defaultDir}} {
		if _, err := parseStartOptions(args, io.Discard); err == nil || !strings.Contains(err.Error(), "cannot be combined with relay config") {
			t.Fatalf("starting with a config and service flags %v: %v", args, err)
		}
	}
	if _, err := parseStartOptions([]string{"--config", "missing.toml"}, io.Discard); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("explicit config path fell back to %s: %v", relayConfigName, err)
	}
	for _, directory := range []string{defaultDir, selectedDir} {
		assertStartStateAbsent(t, directory)
	}
}

func TestStartFlagsWithoutConfig(t *testing.T) {
	root := t.TempDir()
	t.Chdir(root)
	directory := filepath.Join(root, "data")
	for _, tc := range []struct {
		name, listen, announce, allowed, failure string
		flags                                    []string
	}{
		{name: "default public", listen: "/ip4/127.0.0.1/tcp/4001", flags: []string{"--public"}},
		{name: "private port", listen: "/ip4/127.0.0.1/tcp/4600", allowed: startTestPeer, flags: []string{"--port", "4600", "--allow", startTestPeer}},
		{name: "full QUIC addresses", listen: "/ip6/::/udp/4001/quic-v1", announce: "/ip6/2001:db8::1/udp/443/quic-v1", flags: []string{"--public", "--listen", "/ip6/::/udp/4001/quic-v1", "--announce", "/ip6/2001:db8::1/udp/443/quic-v1"}},
		{name: "missing access mode", failure: "exactly one access mode"},
		{name: "mixed access modes", flags: []string{"--public", "--allow", startTestPeer}, failure: "exactly one access mode"},
		{name: "mixed listen options", flags: []string{"--public", "--port=4001", "--listen", "/ip4/127.0.0.1/tcp/0"}, failure: "--port cannot be combined with --listen"},
		{name: "zero port", flags: []string{"--public", "--port=0"}, failure: "port must be between"},
		{name: "wildcard without announce", flags: []string{"--public", "--listen", "/ip4/0.0.0.0/tcp/4001"}, failure: "wildcard listen address"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			args := append([]string{"--data-dir", directory}, tc.flags...)
			options, err := parseStartOptions(args, io.Discard)
			if tc.failure != "" {
				if err == nil || !strings.Contains(err.Error(), tc.failure) {
					t.Fatalf("flags %v: got %v, want %q", tc.flags, err, tc.failure)
				}
			} else {
				if err != nil {
					t.Fatal(err)
				}
				assertStartOptions(t, options, directory, tc.listen, tc.announce, tc.allowed)
			}
			assertStartStateAbsent(t, directory)
		})
	}
}

func TestStartRejectsInvalidTOMLWithoutCreatingIdentity(t *testing.T) {
	root := t.TempDir()
	for _, tc := range []struct{ name, toml, failure string }{
		{"syntax", `mode = [`, "toml:"},
		{"unknown field", "mode = \"public\"\nunknown = 1", "unknown relay config fields"},
		{"case alias", "mode = \"public\"\nMODE = \"private\"", "unknown relay config fields"},
		{"nested table", "mode = \"public\"\n[relay]\nport = 4500", "unknown relay config fields"},
		{"missing mode", "", "mode must be"},
		{"string port", "mode = \"public\"\nport = \"4500\"", "port has the wrong TOML type"},
		{"out of range port", "mode = \"public\"\nport = 65536", "port must be between"},
		{"wildcard without announce", "mode = \"public\"\nlisten_ip = \"0.0.0.0\"", "wildcard listen address"},
		{"wildcard with loopback announce", "mode = \"public\"\nlisten_ip = \"0.0.0.0\"\nannounce_ip = \"127.0.0.1\"", "non-loopback"},
		{"hostname instead of IP", "mode = \"public\"\nlisten_ip = \"localhost\"", "listen_ip"},
		{"public with allowlist", fmt.Sprintf("mode = \"public\"\nallowed_peers = [%q]", startTestPeer), "public relay cannot be combined"},
		{"private without allowlist", `mode = "private"`, "allowed peers"},
		{"private with invalid PeerID", "mode = \"private\"\nallowed_peers = [\"not-a-peer\"]", "invalid allowed_peers"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			directory := filepath.Join(root, "relay")
			path := filepath.Join(root, relayConfigName)
			writeStartTestConfig(t, path, fmt.Sprintf("data_dir = %q\n%s\n", directory, tc.toml))
			output := relayOutput(func([]byte) (int, error) {
				return 0, errors.New("invalid configuration reached relay startup")
			})
			if err := run(t.Context(), []string{"start", "--config", path}, output, io.Discard); err == nil || !strings.Contains(err.Error(), tc.failure) {
				t.Fatalf("invalid configuration returned %v, want %q", err, tc.failure)
			}
			assertStartStateAbsent(t, directory)
		})
	}
}

func TestStartConfigSizeLimit(t *testing.T) {
	root := t.TempDir()
	directory := filepath.Join(root, "relay")
	path := filepath.Join(root, relayConfigName)
	config := fmt.Sprintf("mode = \"public\"\ndata_dir = %q\n#", directory)
	config += strings.Repeat(" ", maxRelayConfig-len(config)-1) + "\n"
	writeStartTestConfig(t, path, config)
	if _, raw, err := readRelayConfig(path); err != nil || string(raw) != config {
		t.Fatalf("configuration at the byte limit did not round trip: %v", err)
	}
	writeStartTestConfig(t, path, config+"\n")
	if _, err := parseStartOptions([]string{"--config", path}, io.Discard); err == nil || !strings.Contains(err.Error(), "65536-byte limit") {
		t.Fatalf("oversize configuration: %v", err)
	}
	assertStartStateAbsent(t, directory)
}

func TestStartRejectsConfigSymlink(t *testing.T) {
	root := t.TempDir()
	directory := filepath.Join(root, "relay")
	path := filepath.Join(root, relayConfigName)
	link := filepath.Join(root, "linked.toml")
	writeStartTestConfig(t, path, fmt.Sprintf("mode = \"public\"\ndata_dir = %q\n", directory))
	symlinkRelayTestInput(t, path, link)
	if _, err := parseStartOptions([]string{"--config", link}, io.Discard); err == nil || !strings.Contains(err.Error(), "symbolic links are not allowed") {
		t.Fatalf("symlink configuration: %v", err)
	}
	assertStartStateAbsent(t, directory)
}

func TestStartServesInForeground(t *testing.T) {
	root := t.TempDir()
	directory := filepath.Join(root, "relay")
	path := filepath.Join(root, relayConfigName)
	writeStartTestConfig(t, path, fmt.Sprintf("mode = \"public\"\ndata_dir = %q\n", directory))
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	port := strconv.Itoa(listener.Addr().(*net.TCPAddr).Port)
	if err := listener.Close(); err != nil {
		t.Fatal(err)
	}

	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	ready, done := make(chan string, 1), make(chan struct{})
	var runErr error
	go func() {
		defer close(done)
		output := relayOutput(func(data []byte) (int, error) {
			ready <- strings.TrimSpace(string(data))
			return len(data), nil
		})
		runErr = run(ctx, []string{"start", "--config", path, "--port", port}, output, io.Discard)
	}()
	t.Cleanup(func() {
		cancel()
		select {
		case <-done:
			if runErr != nil {
				t.Errorf("start returned an error: %v", runErr)
			}
		case <-time.After(10 * time.Second):
			t.Error("start did not stop after cancellation")
		}
	})
	var address string
	select {
	case address = <-ready:
	case <-done:
		t.Fatalf("start stopped before announcing a relay: %v", runErr)
	case <-ctx.Done():
		t.Fatal("start did not announce a relay before the deadline")
	}
	info, err := peer.AddrInfoFromString(address)
	if err != nil || info.ID == "" || len(info.Addrs) != 1 || info.Addrs[0].String() != "/ip4/127.0.0.1/tcp/"+port {
		t.Fatalf("start returned an unexpected address %q: %v", address, err)
	}
	connection, err := (&net.Dialer{}).DialContext(ctx, "tcp4", "127.0.0.1:"+port)
	if err != nil {
		t.Fatalf("dial foreground relay: %v", err)
	}
	_ = connection.Close()
	if id, err := readRelayPeerID(directory); err != nil || id != info.ID {
		t.Fatalf("advertised identity %s not persisted: %s, %v", info.ID, id, err)
	}
	select {
	case <-done:
		t.Fatalf("start returned before cancellation: %v", runErr)
	default:
	}
}

func TestLegacyServeIgnoresRelayConfig(t *testing.T) {
	root := t.TempDir()
	t.Chdir(root)
	writeStartTestConfig(t, relayConfigName, "invalid = [")
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	var address bytes.Buffer
	output := relayOutput(func(data []byte) (int, error) {
		defer cancel()
		return address.Write(data)
	})
	args := []string{"serve", "--data-dir", filepath.Join(root, "relay"), "--listen", "/ip4/127.0.0.1/tcp/0", "--public"}
	if err := run(ctx, args, output, io.Discard); err != nil {
		t.Fatalf("legacy serve did not ignore %s: %v", relayConfigName, err)
	}
	if got, err := peer.AddrInfoFromString(strings.TrimSpace(address.String())); err != nil || got.ID == "" || len(got.Addrs) != 1 {
		t.Fatalf("legacy serve did not advertise its bound address: %q, %v", address.String(), err)
	}
}

func TestRelayVersion(t *testing.T) {
	var output bytes.Buffer
	if err := run(t.Context(), []string{"--version"}, &output, io.Discard); err != nil {
		t.Fatal(err)
	}
	if got, want := output.String(), "Sailry Relay "+link.ProductVersion+"\n"; got != want {
		t.Fatalf("version = %q, want %q", got, want)
	}
}

func writeStartTestConfig(t *testing.T, path, config string) {
	t.Helper()
	if err := os.WriteFile(path, []byte(config), 0o600); err != nil {
		t.Fatal(err)
	}
}

func symlinkRelayTestInput(t *testing.T, target, link string) {
	t.Helper()
	if err := os.Symlink(target, link); err != nil {
		if runtime.GOOS == "windows" {
			t.Skipf("fixture symlink unavailable: %v", err)
		}
		t.Fatal(err)
	}
}

func assertStartStateAbsent(t *testing.T, directory string) {
	t.Helper()
	if _, err := readRelayPeerID(directory); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("readRelayPeerID on missing state: %v", err)
	}
	if _, err := os.Lstat(directory); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("configuration validation created relay state: %v", err)
	}
}

func assertStartOptions(t *testing.T, options startOptions, directory, listen, announce, allowed string) {
	t.Helper()
	if options.directory != directory || options.config.ListenAddr.String() != listen || options.config.AnnounceAddr.String() != announce {
		t.Fatalf("relay options = (%q, %s, %s), want (%q, %s, %s)", options.directory, options.config.ListenAddr, options.config.AnnounceAddr, directory, listen, announce)
	}
	if options.config.PrivateKey != nil {
		t.Fatal("configuration validation loaded a relay identity")
	}
	peers := options.config.AllowedPeers
	if allowed == "" {
		if !options.config.Public || len(peers) != 0 {
			t.Fatal("expected a public relay without a peer allowlist")
		}
	} else if options.config.Public || len(peers) != 1 || peers[0].String() != allowed {
		t.Fatalf("expected a private relay with peer %s, got %v", allowed, peers)
	}
}
