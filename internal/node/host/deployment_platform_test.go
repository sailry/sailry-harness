package host

import (
	"context"
	"crypto/ed25519"
	"errors"
	"io"
	"net"
	"slices"
	"strings"
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
	return deploymentSSHExec(t, version, func(command string, channel cryptossh.Channel, request *cryptossh.Request) {
		_ = request.Reply(true, nil)
		stdout, stderr, status := answer(command)
		deploymentSSHResult(channel, stdout, stderr, status)
	})
}

func deploymentSSHResult(channel cryptossh.Channel, stdout, stderr string, status uint32) {
	if n, err := io.WriteString(channel, stdout); err != nil || n != len(stdout) {
		return
	}
	if n, err := io.WriteString(channel.Stderr(), stderr); err != nil || n != len(stderr) {
		return
	}
	_, _ = channel.SendRequest("exit-status", false, cryptossh.Marshal(struct{ Status uint32 }{status}))
}

func deploymentSSHExec(t *testing.T, version string, exec func(string, cryptossh.Channel, *cryptossh.Request)) (*cryptossh.Client, func() []string) {
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
					exec(payload.Value, channel, request)
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
	connection, commands := deploymentSSHExec(t, "SSH-2.0-fixture", func(command string, channel cryptossh.Channel, request *cryptossh.Request) {
		if command == "uname -s; uname -m" {
			_ = request.Reply(true, nil)
			deploymentSSHResult(channel, "", "uname unavailable", 127)
			return
		}
		_ = request.Reply(false, nil)
	})
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	_, _, err := detectPlatform(ctx, connection)
	var fault *protocol.Fault
	if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeUnavailable || fault.SSHReason != protocol.SSHCommandNonzeroExit || fault.Message != "Host installation command failed" {
		t.Fatalf("non-Windows failure was replaced: %v", err)
	}
	if !slices.Equal(commands(), []string{"uname -s; uname -m", "cmd.exe /d /c ver"}) {
		t.Fatalf("unexpected detection commands: %q", commands())
	}
}

func TestSSHStagingGuidanceIsFixed(t *testing.T) {
	for _, test := range []struct {
		name, stderr, message string
	}{
		{"missing python", "python3 is required\n", "Host installation command failed: python3 is required"},
		{"private stderr", "python3 is required\n/private/remote-home", "Host installation command failed"},
	} {
		t.Run(test.name, func(t *testing.T) {
			connection, _ := deploymentSSH(t, "SSH-2.0-fixture", func(string) (string, string, uint32) {
				return "", test.stderr, 1
			})
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			defer cancel()
			_, err := commandSSH(ctx, connection, sshStaging, stageCommand, nil)
			var fault *protocol.Fault
			if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeUnavailable || fault.SSHReason != protocol.SSHCommandNonzeroExit || fault.Message != test.message {
				t.Fatalf("staging fault = %v", err)
			}
		})
	}
}

func TestSSHInstallationFaults(t *testing.T) {
	root := ArtifactRoot
	ArtifactRoot = t.TempDir()
	t.Cleanup(func() { ArtifactRoot = root })
	const platform = "uname -s; uname -m"
	const paths = "printf '%s\\n' \"$HOME\"; uname -s; id -u"
	const installer = "sh -s"
	const bootstrap = "cat '/home/fixture/.sailry-host/bootstrap.ticket' 2>/dev/null"
	for _, test := range []struct {
		name, command string
		reply         string
		code          protocol.ErrorCode
		reason        protocol.SSHFailureReason
		calls         []string
	}{
		{"probe exit 73", paths, "rollback", protocol.ErrorCodeUnavailable, protocol.SSHCommandNonzeroExit, []string{platform, paths}},
		{"probe without exit", paths, "missing", protocol.ErrorCodeUnavailable, protocol.SSHCommandExitMissing, []string{platform, paths}},
		{"exec rejected", installer, "rejected", protocol.ErrorCodeUnavailable, protocol.SSHCommandRejected, []string{platform, paths, installer}},
		{"exec receipt lost", installer, "receipt", protocol.ErrorCodeOutcomeUnknown, protocol.SSHCommandReceiptLost, []string{platform, paths, installer}},
		{"installer without exit", installer, "missing", protocol.ErrorCodeOutcomeUnknown, protocol.SSHCommandExitMissing, []string{platform, paths, installer}},
		{"malformed installer result", installer, "malformed", protocol.ErrorCodeOutcomeUnknown, protocol.SSHCommandResultUnavailable, []string{platform, paths, installer}},
		{"installer killed", installer, "signal", protocol.ErrorCodeOutcomeUnknown, protocol.SSHCommandInterrupted, []string{platform, paths, installer}},
		{"ordinary installer failure", installer, "nonzero", protocol.ErrorCodeUnavailable, protocol.SSHCommandNonzeroExit, []string{platform, paths, installer}},
		{"installer rollback unconfirmed", installer, "rollback", protocol.ErrorCodeOutcomeUnknown, protocol.SSHInstallRollbackUnconfirmed, []string{platform, paths, installer}},
		{"installer output truncated", installer, "truncated", protocol.ErrorCodeOutcomeUnknown, protocol.SSHCommandOutputTruncated, []string{platform, paths, installer}},
		{"bootstrap receipt lost after install", bootstrap, "receipt", protocol.ErrorCodeOutcomeUnknown, protocol.SSHBootstrapTicketUnavailable, []string{platform, paths, installer, bootstrap}},
	} {
		t.Run(test.name, func(t *testing.T) {
			connection, commands := deploymentSSHExec(t, "SSH-2.0-fixture", func(command string, channel cryptossh.Channel, request *cryptossh.Request) {
				if command == test.command {
					switch test.reply {
					case "rejected":
						_ = request.Reply(false, nil)
						return
					case "receipt":
						return
					}
					_ = request.Reply(true, nil)
					_, _ = io.Copy(io.Discard, channel)
					switch test.reply {
					case "malformed":
						_, _ = channel.SendRequest("exit-status", false, []byte{0})
					case "signal":
						_, _ = channel.SendRequest("exit-signal", false, cryptossh.Marshal(struct {
							Signal     string
							CoreDumped bool
							Error      string
							Language   string
						}{Signal: "KILL"}))
					case "nonzero":
						deploymentSSHResult(channel, "", "fixture failure", 1)
					case "rollback":
						deploymentSSHResult(channel, "", "fixture recovery failure", 73)
					case "truncated":
						deploymentSSHResult(channel, strings.Repeat("x", 3<<20), "", 0)
					}
					return
				}
				if command != platform && command != paths && command != installer {
					_ = request.Reply(false, nil)
					return
				}
				_ = request.Reply(true, nil)
				switch command {
				case platform:
					deploymentSSHResult(channel, "Linux\nx86_64\n", "", 0)
				case paths:
					deploymentSSHResult(channel, "/home/fixture\nLinux\n1000\n", "", 0)
				case installer:
					_, _ = io.Copy(io.Discard, channel)
					deploymentSSHResult(channel, "", "", 0)
				}
			})
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			defer cancel()
			caller := protocol.NodeID{1}
			request := protocol.NewRequest(caller, protocol.Command{Kind: "install_host", Data: protocol.CommandInstallHostPayload{}})
			outcome, err := NewInstallations(DeploymentOptions{}).Execute(ctx, connection, nil, caller, request.ID, nil, nil)
			var fault *protocol.Fault
			if outcome.Kind != "" || !errors.As(err, &fault) || fault.Code != test.code || fault.SSHReason != test.reason {
				t.Fatalf("install_host = %+v, fault = %+v, want %s/%s", outcome, fault, test.code, test.reason)
			}
			if got := commands(); !slices.Equal(got, test.calls) {
				t.Fatalf("install_host sent %q, want %q", got, test.calls)
			}
		})
	}
}
