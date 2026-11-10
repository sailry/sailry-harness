//go:build integration

package terminal_test

import (
	"bufio"
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"errors"
	"fmt"
	"net"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
	cryptossh "golang.org/x/crypto/ssh"
)

const sshTerminalPassword = "isolated-terminal-password"

// sshWindow is a terminal size as the SSH wire carries it.
type sshWindow struct{ Columns, Rows, Width, Height uint32 }

// sshShellFixture is an in-process SSH server on loopback with a generated
// host key. Its session accepts a terminal request and a shell, greets,
// answers every input line with "echo:<line>" and every window change with
// "resize:<columns>x<rows>:<width>x<height>". It starts no process.
type sshShellFixture struct {
	key             protocol.SshHostKey
	port            uint16
	authentications atomic.Int32
	mu              sync.Mutex
	terminal        string
	opened          sshWindow
	resized         []sshWindow
	// ended is closed when the client side of the shell's channel is gone.
	ended chan struct{}
}

func startSSHShellFixture(t *testing.T) *sshShellFixture {
	t.Helper()
	_, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	signer, err := cryptossh.NewSignerFromKey(private)
	if err != nil {
		t.Fatal(err)
	}
	fixture := &sshShellFixture{
		key:   protocol.SshHostKey{Algorithm: signer.PublicKey().Type(), Fingerprint: cryptossh.FingerprintSHA256(signer.PublicKey())},
		ended: make(chan struct{}),
	}
	config := &cryptossh.ServerConfig{PasswordCallback: func(_ cryptossh.ConnMetadata, password []byte) (*cryptossh.Permissions, error) {
		fixture.authentications.Add(1)
		if string(password) != sshTerminalPassword {
			return nil, errors.New("password rejected")
		}
		return nil, nil
	}}
	config.AddHostKey(signer)
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	fixture.port = uint16(listener.Addr().(*net.TCPAddr).Port)
	go func() {
		for {
			connection, err := listener.Accept()
			if err != nil {
				return
			}
			go fixture.serve(connection, config)
		}
	}()
	return fixture
}

func (fixture *sshShellFixture) serve(connection net.Conn, config *cryptossh.ServerConfig) {
	defer connection.Close()
	server, channels, requests, err := cryptossh.NewServerConn(connection, config)
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
		go fixture.session(channel, requests)
	}
}

func (fixture *sshShellFixture) session(channel cryptossh.Channel, requests <-chan *cryptossh.Request) {
	defer channel.Close()
	var output sync.Mutex
	write := func(text string) {
		output.Lock()
		defer output.Unlock()
		_, _ = channel.Write([]byte(text + "\r\n"))
	}
	for request := range requests {
		switch request.Type {
		case "pty-req":
			var payload struct {
				Term                         string
				Columns, Rows, Width, Height uint32
				Modes                        string
			}
			if err := cryptossh.Unmarshal(request.Payload, &payload); err != nil {
				_ = request.Reply(false, nil)
				continue
			}
			fixture.mu.Lock()
			fixture.terminal = payload.Term
			fixture.opened = sshWindow{payload.Columns, payload.Rows, payload.Width, payload.Height}
			fixture.mu.Unlock()
			_ = request.Reply(true, nil)
		case "shell":
			_ = request.Reply(true, nil)
			write("fixture shell ready")
			go func() {
				defer close(fixture.ended)
				reader := bufio.NewReader(channel)
				var line []byte
				for {
					character, err := reader.ReadByte()
					if err != nil {
						return
					}
					if character != '\r' && character != '\n' {
						line = append(line, character)
						continue
					}
					if len(line) > 0 {
						write("echo:" + string(line))
						line = line[:0]
					}
				}
			}()
		case "window-change":
			var window sshWindow
			if err := cryptossh.Unmarshal(request.Payload, &window); err == nil {
				fixture.mu.Lock()
				fixture.resized = append(fixture.resized, window)
				fixture.mu.Unlock()
				write(fmt.Sprintf("resize:%dx%d:%dx%d", window.Columns, window.Rows, window.Width, window.Height))
			}
		default:
			_ = request.Reply(false, nil)
		}
	}
}

func sshTerminalPayload[Value any](data any) (Value, bool) {
	if value, valid := data.(Value); valid {
		return value, true
	}
	if value, valid := data.(*Value); valid && value != nil {
		return *value, true
	}
	var empty Value
	return empty, false
}

// TestSSHTerminalIntegration opens a real SSH terminal through a Node's
// open_ssh_terminal against an in-process SSH server: the untrusted host key
// is reported before authentication, the terminal streams output, takes
// input and resizes after the open command returned, and close_ssh_terminal
// ends it.
func TestSSHTerminalIntegration(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 90*time.Second)
	defer cancel()
	fixture := startSSHShellFixture(t)
	owner, err := node.Start(ctx, filepath.Join(t.TempDir(), "profile"))
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Stop(context.Background())
	controller := client.New(link.NewLocal(owner.ID(), owner.ID(), owner))
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	do := func(command protocol.Command) protocol.Output {
		t.Helper()
		output, err := controller.Do(ctx, command)
		if err != nil {
			t.Fatalf("%s: %v", command.Kind, err)
		}
		return output
	}
	terminalStatus := func(id protocol.TerminalID) string {
		for _, info := range controller.Snapshot().Terminals {
			if info.ID == id {
				return info.Status.Kind
			}
		}
		return ""
	}
	listed := func(profile protocol.SshID) []protocol.TerminalID {
		t.Helper()
		output := do(protocol.Command{Kind: "list_ssh_terminals", Data: protocol.CommandListSshTerminalsPayload{Profile: profile}})
		values, valid := sshTerminalPayload[[]protocol.TerminalInfo](output.Data)
		if !valid || output.Kind != "terminals" {
			t.Fatalf("list_ssh_terminals = %#v", output)
		}
		ids := []protocol.TerminalID{}
		for _, info := range values {
			ids = append(ids, info.ID)
		}
		return ids
	}
	// eventually polls a condition for at most 15 seconds.
	eventually := func(label string, ready func() bool) {
		t.Helper()
		deadline := time.NewTimer(15 * time.Second)
		defer deadline.Stop()
		ticker := time.NewTicker(10 * time.Millisecond)
		defer ticker.Stop()
		for !ready() {
			select {
			case <-ctx.Done():
				t.Fatalf("%s: %v", label, ctx.Err())
			case <-deadline.C:
				t.Fatalf("%s did not happen within 15 seconds; Node terminals: %+v", label, controller.Snapshot().Terminals)
			case <-ticker.C:
			}
		}
	}

	saved := do(protocol.Command{Kind: "save_ssh", Data: protocol.CommandSaveSshPayload{
		Profile:    protocol.SshProfile{ID: protocol.NewSshID(), Name: "Terminal fixture", Host: "127.0.0.1", Port: fixture.port, Username: "fixture", Authentication: protocol.SshAuthenticationPassword},
		Credential: &protocol.SshCredential{Kind: "password", Data: &protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret(sshTerminalPassword)}},
	}})
	profile, valid := sshTerminalPayload[protocol.SshProfile](saved.Data)
	if !valid || profile.HostKey != nil {
		t.Fatalf("saved SSH profile = %#v", saved)
	}
	launch := protocol.SshTerminalLaunch{
		Viewport:   protocol.TerminalViewport{Columns: 80, Rows: 24, PixelWidth: 640, PixelHeight: 384},
		Appearance: protocol.TerminalAppearance{ColorScheme: protocol.TerminalColorSchemeLight},
	}
	open := func(revision uint64) (protocol.Request, protocol.SshOutcome) {
		t.Helper()
		request := controller.Prepare(protocol.Command{Kind: "open_ssh_terminal", Data: protocol.CommandOpenSshTerminalPayload{Profile: profile.ID, ExpectedRevision: revision, Launch: launch}})
		output, err := controller.Execute(ctx, request)
		if err != nil {
			t.Fatalf("open_ssh_terminal: %v", err)
		}
		outcome, valid := sshTerminalPayload[protocol.SshOutcome](output.Data)
		if !valid || output.Kind != "ssh_outcome" {
			t.Fatalf("open_ssh_terminal = %#v", output)
		}
		return request, outcome
	}

	// An untrusted host key is reported before any authentication and no
	// terminal starts.
	_, outcome := open(profile.Revision)
	required, valid := sshTerminalPayload[protocol.SshOutcomeHostKeyRequiredPayload](outcome.Data)
	if outcome.Kind != "host_key_required" || !valid || required.Changed || required.Key != fixture.key {
		t.Fatalf("untrusted host key outcome = %#v", outcome)
	}
	if count := fixture.authentications.Load(); count != 0 {
		t.Fatalf("authentication ran %d times before the host key was trusted", count)
	}
	if terminals := controller.Snapshot().Terminals; len(terminals) != 0 || len(listed(profile.ID)) != 0 {
		t.Fatalf("an untrusted host started a terminal: %+v", terminals)
	}
	t.Log("host key: open_ssh_terminal reported host_key_required before authentication")

	trusted := do(protocol.Command{Kind: "trust_ssh", Data: protocol.CommandTrustSshPayload{Profile: profile.ID, ExpectedRevision: profile.Revision, Key: required.Key}})
	if profile, valid = sshTerminalPayload[protocol.SshProfile](trusted.Data); !valid || profile.HostKey == nil || *profile.HostKey != fixture.key {
		t.Fatalf("trusted SSH profile = %#v", trusted)
	}

	// The terminal is named by its request and belongs to the profile.
	request, outcome := open(profile.Revision)
	info, valid := sshTerminalPayload[protocol.TerminalInfo](outcome.Data)
	if outcome.Kind != "terminal" || !valid {
		t.Fatalf("open_ssh_terminal outcome = %#v", outcome)
	}
	if info.ID != protocol.TerminalID(request.ID) || info.Ssh == nil || *info.Ssh != profile.ID || info.Worktree != nil || info.Access != nil || info.Directory != nil ||
		info.Status.Kind != "running" || info.Owner == nil || *info.Owner != owner.ID() {
		t.Fatalf("SSH terminal = %+v", info)
	}
	fixture.mu.Lock()
	terminal, opened := fixture.terminal, fixture.opened
	fixture.mu.Unlock()
	if terminal != "xterm-256color" || opened != (sshWindow{80, 24, 640, 384}) {
		t.Fatalf("terminal request = %q %+v", terminal, opened)
	}
	if fixture.authentications.Load() != 1 {
		t.Fatalf("authentications = %d, want 1", fixture.authentications.Load())
	}

	// The open command has returned. The terminal outlives it: it is still
	// running a moment later and everything below happens on its stream.
	time.Sleep(500 * time.Millisecond)
	eventually("the Node reports the SSH terminal", func() bool { return terminalStatus(info.ID) == "running" })
	select {
	case <-fixture.ended:
		t.Fatal("the SSH shell ended when the open command returned")
	default:
	}
	if ids := listed(profile.ID); !slices.Equal(ids, []protocol.TerminalID{info.ID}) {
		t.Fatalf("list_ssh_terminals = %v, want %v", ids, info.ID)
	}

	// Output arrives on the terminal topic: first the snapshot, then frames.
	stream, err := controller.Subscribe(ctx, protocol.Topic{Kind: "terminal", Data: info.ID})
	if err != nil {
		t.Fatal(err)
	}
	defer stream.Close()
	var received strings.Builder
	kinds := map[string]int{}
	streamed := func(needle string) {
		t.Helper()
		bounded, stop := context.WithTimeout(ctx, 15*time.Second)
		defer stop()
		for !strings.Contains(received.String(), needle) {
			update, err := stream.Next(bounded)
			if err != nil {
				t.Fatalf("the terminal stream did not deliver %q: %v; received %q", needle, err, received.String())
			}
			kinds[update.Kind]++
			var screen protocol.TerminalScreen
			switch update.Kind {
			case "terminal_snapshot":
				value, valid := sshTerminalPayload[protocol.TerminalSnapshot](update.Data)
				if !valid || value.Info.ID != info.ID {
					t.Fatalf("terminal snapshot = %#v", update)
				}
				screen = value.Screen
			case "terminal_frame":
				value, valid := sshTerminalPayload[protocol.TerminalFrame](update.Data)
				replacement, replaced := sshTerminalPayload[protocol.TerminalScreenScreenUpdateReplacePayload](value.Screen.Data)
				if !valid || !replaced || value.Info.ID != info.ID {
					t.Fatalf("terminal frame = %#v", update)
				}
				screen = replacement.Screen
			default:
				continue
			}
			if screen.VT == nil {
				t.Fatalf("%s carries no screen bytes", update.Kind)
			}
			received.Write(screen.VT.Bytes)
		}
	}
	streamed("fixture shell ready")
	t.Log("output: the shell's greeting arrived on the terminal topic after the open command returned")

	input := do(protocol.Command{Kind: "input_terminal", Data: protocol.CommandInputTerminalPayload{
		Terminal: info.ID, Revision: info.Revision, Bytes: protocol.Bytes("b10-ssh-input\n"), Inputs: []protocol.TerminalInput{},
	}})
	if input.Kind != "terminal_input" {
		t.Fatalf("input_terminal = %#v", input)
	}
	streamed("echo:b10-ssh-input")
	if kinds["terminal_frame"] == 0 {
		t.Fatalf("the reply did not arrive as a frame: %v", kinds)
	}
	t.Log("input: input_terminal reached the remote shell and its reply streamed back as a frame")

	resized := do(protocol.Command{Kind: "resize_terminal", Data: protocol.CommandResizeTerminalPayload{
		Terminal: info.ID, Revision: info.Revision, Viewport: protocol.TerminalViewport{Columns: 100, Rows: 30, PixelWidth: 800, PixelHeight: 480},
	}})
	if resized.Kind != "terminal" {
		t.Fatalf("resize_terminal = %#v", resized)
	}
	streamed("resize:100x30:800x480")
	fixture.mu.Lock()
	changes := slices.Clone(fixture.resized)
	fixture.mu.Unlock()
	if len(changes) == 0 || changes[len(changes)-1] != (sshWindow{100, 30, 800, 480}) {
		t.Fatalf("window changes = %+v", changes)
	}
	inspected := do(protocol.Command{Kind: "inspect_terminal", Data: protocol.CommandInspectTerminalPayload{Terminal: info.ID}})
	snapshot, valid := sshTerminalPayload[protocol.TerminalSnapshot](inspected.Data)
	if !valid || snapshot.Screen.VT == nil || snapshot.Screen.VT.Viewport.Columns != 100 || snapshot.Screen.VT.Viewport.Rows != 30 ||
		!strings.Contains(string(snapshot.Screen.VT.Bytes), "echo:b10-ssh-input") {
		t.Fatalf("inspected SSH terminal = %#v", inspected)
	}
	t.Log("resize: resize_terminal sent the window change with its pixel size and the mirror follows")

	// close_ssh_terminal ends the remote shell and the Node terminal.
	closed := do(protocol.Command{Kind: "close_ssh_terminal", Data: protocol.CommandCloseSshTerminalPayload{Terminal: info.ID}})
	final, valid := sshTerminalPayload[protocol.TerminalInfo](closed.Data)
	if !valid || closed.Kind != "terminal" || final.ID != info.ID || final.Status.Kind != "closed" || final.Owner != nil {
		t.Fatalf("close_ssh_terminal = %#v", closed)
	}
	select {
	case <-fixture.ended:
	case <-time.After(15 * time.Second):
		t.Fatal("the remote shell's channel stayed open after close_ssh_terminal")
	}
	eventually("the Node reports the terminal closed", func() bool { return terminalStatus(info.ID) == "closed" })
	if ids := listed(profile.ID); len(ids) != 0 {
		t.Fatalf("a closed SSH terminal is still listed: %v", ids)
	}
	_, err = controller.Do(ctx, protocol.Command{Kind: "input_terminal", Data: protocol.CommandInputTerminalPayload{
		Terminal: info.ID, Revision: final.Revision, Bytes: protocol.Bytes("late\n"), Inputs: []protocol.TerminalInput{},
	}})
	var fault *link.FaultError
	if !errors.As(err, &fault) || fault.Fault.Code != protocol.ErrorCodeNotFound {
		t.Fatalf("input after close = %v", err)
	}
	t.Log("close: close_ssh_terminal closed the remote channel and the Node terminal")
}
