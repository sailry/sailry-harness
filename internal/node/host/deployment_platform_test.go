package host

import (
	"context"
	"crypto/ed25519"
	"errors"
	"io"
	"net"
	"slices"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
	cryptossh "golang.org/x/crypto/ssh"
)

// deploymentSSH answers exec requests in process and records every command;
// no shell, installer or file-transfer subsystem runs.
func deploymentSSH(t *testing.T, version string, answer func(string) (string, string, uint32)) (*cryptossh.Client, func() []string) {
	t.Helper()
	signer, err := cryptossh.NewSignerFromKey(ed25519.NewKeyFromSeed(make([]byte, ed25519.SeedSize)))
	if err != nil {
		t.Fatal(err)
	}
	config := &cryptossh.ServerConfig{NoClientAuth: true, ServerVersion: version}
	config.AddHostKey(signer)
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	var mu sync.Mutex
	var commands []string
	done := make(chan struct{})
	t.Cleanup(func() {
		_ = listener.Close()
		select {
		case <-done:
		case <-time.After(5 * time.Second):
			t.Error("SSH fixture did not stop")
		}
	})
	go func() {
		defer close(done)
		raw, err := listener.Accept()
		if err != nil {
			return
		}
		defer raw.Close()
		server, channels, requests, err := cryptossh.NewServerConn(raw, config)
		if err != nil {
			return
		}
		defer server.Close()
		go cryptossh.DiscardRequests(requests)
		for offered := range channels {
			if offered.ChannelType() != "session" {
				_ = offered.Reject(cryptossh.UnknownChannelType, "session expected")
				continue
			}
			channel, requests, err := offered.Accept()
			if err != nil {
				return
			}
			go func() {
				defer channel.Close()
				for request := range requests {
					var payload struct{ Value string }
					err := cryptossh.Unmarshal(request.Payload, &payload)
					name := request.Type + ":" + payload.Value
					if request.Type == "exec" {
						name = payload.Value
					}
					mu.Lock()
					commands = append(commands, name)
					mu.Unlock()
					if err != nil || request.Type != "exec" {
						_ = request.Reply(false, nil)
						return
					}
					_ = request.Reply(true, nil)
					stdout, stderr, status := answer(payload.Value)
					_, _ = io.WriteString(channel, stdout)
					_, _ = io.WriteString(channel.Stderr(), stderr)
					_, _ = channel.SendRequest("exit-status", false, cryptossh.Marshal(struct{ Status uint32 }{status}))
					return
				}
			}()
		}
	}()
	client, err := cryptossh.Dial("tcp", listener.Addr().String(), &cryptossh.ClientConfig{
		User: "fixture", HostKeyCallback: cryptossh.FixedHostKey(signer.PublicKey()), Timeout: 5 * time.Second,
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = client.Close() })
	return client, func() []string {
		mu.Lock()
		defer mu.Unlock()
		return slices.Clone(commands)
	}
}

func TestSSHWindowsInstallationGuidance(t *testing.T) {
	for _, test := range []struct {
		name, version, platform, stderr string
		status                          uint32
		probe                           bool
	}{
		{"Windows OpenSSH cmd", "SSH-2.0-OpenSSH_for_Windows_9.5", "", "uname is not recognized", 1, false},
		{"Windows OpenSSH PowerShell", "SSH-2.0-OpenSSH_for_Windows_9.5", "", "uname : The term 'uname' is not recognized", 0, false},
		{"another Windows SSH server", "SSH-2.0-fixture", "", "uname : The term 'uname' is not recognized", 1, true},
		{"Windows POSIX shell", "SSH-2.0-fixture", "MINGW64_NT-10.0-26100\nx86_64\n", "", 0, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			connection, commands := deploymentSSH(t, test.version, func(command string) (string, string, uint32) {
				switch command {
				case "uname -s; uname -m":
					return test.platform, test.stderr, test.status
				case "cmd.exe /d /c ver":
					return "\r\nMicrosoft Windows [Version 10.0.26100.1]\r\n", "", 0
				default:
					return "", "unexpected installation command", 99
				}
			})
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			defer cancel()
			caller := protocol.NodeID{1}
			request := protocol.NewRequest(caller, protocol.Command{Kind: "install_host", Data: protocol.CommandInstallHostPayload{}})
			outcome, err := NewInstallations(DeploymentOptions{}).Execute(ctx, connection, nil, caller, request.ID, nil, nil)
			var fault *protocol.Fault
			if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeUnavailable || fault.Message != protocol.SSHWindowsDesktopRequired || outcome.Kind != "" {
				t.Fatalf("Windows installation = %+v, %v", outcome, err)
			}
			want := []string{"uname -s; uname -m"}
			if test.probe {
				want = append(want, "cmd.exe /d /c ver")
			}
			if got := commands(); !slices.Equal(got, want) {
				t.Fatalf("installation sent %q, want read-only detection %q", got, want)
			}
		})
	}
}

func TestSSHPlatformDetectionKeepsPOSIX(t *testing.T) {
	for _, test := range []struct {
		name, version, platform, target, turso string
	}{
		{"Linux", "SSH-2.0-fixture", "Linux\nx86_64\n", "linux-amd64", "libturso_sync_sdk_kit.so"},
		{"macOS", "SSH-2.0-fixture", "Darwin\narm64\n", "darwin-arm64", "libturso_sync_sdk_kit.dylib"},
		{"WSL behind Windows OpenSSH", "SSH-2.0-OpenSSH_for_Windows_9.5", "Linux\naarch64\n", "linux-arm64", "libturso_sync_sdk_kit.so"},
	} {
		t.Run(test.name, func(t *testing.T) {
			connection, commands := deploymentSSH(t, test.version, func(command string) (string, string, uint32) {
				if command == "uname -s; uname -m" {
					return test.platform, "", 0
				}
				return "", "unexpected Windows probe", 99
			})
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			defer cancel()
			target, turso, err := detectPlatform(ctx, connection)
			if err != nil || target != test.target || turso != test.turso || !slices.Equal(commands(), []string{"uname -s; uname -m"}) {
				t.Fatalf("platform = %q, %q, %v; commands %q", target, turso, err, commands())
			}
		})
	}
}

func TestSSHPlatformDetectionKeepsFailure(t *testing.T) {
	connection, commands := deploymentSSH(t, "SSH-2.0-fixture", func(command string) (string, string, uint32) {
		if command == "uname -s; uname -m" {
			return "", "uname unavailable", 127
		}
		return "", "cmd unavailable", 127
	})
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	_, _, err := detectPlatform(ctx, connection)
	var fault *protocol.Fault
	if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeUnavailable || fault.Message != "Host installation command failed: uname unavailable" {
		t.Fatalf("non-Windows failure was replaced: %v", err)
	}
	if !slices.Equal(commands(), []string{"uname -s; uname -m", "cmd.exe /d /c ver"}) {
		t.Fatalf("unexpected detection commands: %q", commands())
	}
}
