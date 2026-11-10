package terminal

import (
	"crypto/ed25519"
	"crypto/rand"
	"errors"
	"io"
	"net"
	"os"
	"path/filepath"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
	cryptossh "golang.org/x/crypto/ssh"
)

// testShellCommand checks how saved settings become a terminal program. It is
// a helper for a grouped test to call as a subtest; it starts no process.
func TestShellCommand(t *testing.T) {
	t.Helper()
	if command, variables, directory, err := shellCommand("linux", protocol.TerminalSettings{}, "/work"); err != nil || command != nil || variables != nil || directory != "/work" {
		t.Fatalf("default shell = %v %v %q %v", command, variables, directory, err)
	}
	shell := filepath.Join(t.TempDir(), "shell")
	if _, _, _, err := shellCommand("linux", protocol.TerminalSettings{Shell: shell}, "/work"); err == nil {
		t.Fatal("a missing shell was accepted")
	}
	if err := os.WriteFile(shell, []byte("#!/bin/sh\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	command, variables, _, err := shellCommand("linux", protocol.TerminalSettings{Shell: shell}, "/work")
	if err != nil || !slices.Equal(command, []string{shell, "-l"}) || !slices.Equal(variables, []string{"SHELL=" + shell}) {
		t.Fatalf("login shell = %v %v %v", command, variables, err)
	}
	command, _, _, err = shellCommand("linux", protocol.TerminalSettings{Shell: shell, ShellArguments: []string{"-i"}}, "/work")
	if err != nil || !slices.Equal(command, []string{shell, "-i"}) {
		t.Fatalf("shell with arguments = %v %v", command, err)
	}
	if values := Environment([]string{"A=1", "B=2"}, protocol.TerminalSettings{Environment: map[string]string{"B": "3", "C": "4"}}); !slices.Equal(values, []string{"A=1", "B=2", "B=3", "C=4"}) {
		t.Fatalf("environment = %v", values)
	}

	const launcher = `C:\Windows\System32\wsl.exe`
	for _, test := range []struct {
		name      string
		shell     string
		arguments []string
		root      string
		command   []string
		directory string
		message   string
	}{
		{"plain shell", `C:\Program Files\PowerShell\7\pwsh.exe`, []string{"-NoLogo"}, `C:\work`, []string{`C:\Program Files\PowerShell\7\pwsh.exe`, "-NoLogo"}, `C:\work`, ""},
		{"local drive", launcher, nil, `C:\work`, []string{launcher, "--cd", `C:\work`}, `C:\work`, ""},
		{"verbatim drive", launcher, nil, `\\?\C:\work`, []string{launcher, "--cd", `C:\work`}, `C:\work`, ""},
		{"own share", launcher, []string{"--distribution", "Ubuntu"}, `\\wsl$\ubuntu\home\me\work`, []string{launcher, "--distribution", "Ubuntu", "--cd", "/home/me/work"}, `C:\Windows\System32`, ""},
		{"localhost share root", launcher, []string{"-d", "Ubuntu"}, `\\wsl.localhost\Ubuntu`, []string{launcher, "-d", "Ubuntu", "--cd", "/"}, `C:\Windows\System32`, ""},
		{"another distribution", launcher, []string{"-d", "Debian"}, `\\wsl$\Ubuntu\home`, nil, "", "WSL worktree requires its matching distribution"},
		{"no distribution", launcher, nil, `\\wsl$\Ubuntu\home`, nil, "", "WSL worktree requires its matching distribution"},
		{"network share", launcher, nil, `\\server\share\work`, nil, "", "WSL worktree requires a local drive or matching distribution path"},
	} {
		command, _, directory, err := shellCommand("windows", protocol.TerminalSettings{Shell: test.shell, ShellArguments: test.arguments}, test.root)
		message := ""
		var failure *protocol.Fault
		if errors.As(err, &failure) {
			message = failure.Message
		}
		if message != test.message || !slices.Equal(command, test.command) || directory != test.directory {
			t.Fatalf("%s = %v %q %v", test.name, command, directory, err)
		}
	}

	t.Run("launch variables", func(t *testing.T) {
		t.Setenv("VTE_VERSION", "7600")
		variables := launchVariables("windows", `\\?\C:\work`)
		for _, entry := range []string{"TERM_PROGRAM=ghostty", "TERM_PROGRAM_VERSION=" + terminalVersion(), `PWD=C:\work`, "VTE_VERSION="} {
			if !slices.Contains(variables, entry) {
				t.Fatalf("launch variables = %v, missing %q", variables, entry)
			}
		}
		if variables := launchVariables("linux", "/work"); !slices.Contains(variables, "PWD=/work") {
			t.Fatalf("launch variables = %v", variables)
		}
	})

	t.Run("activity", func(t *testing.T) {
		const none = protocol.TerminalActivity("")
		for _, test := range []struct {
			name  string
			steps []string
			want  []protocol.TerminalActivity
		}{
			{"fragmented progress",
				[]string{"prompt text", "\x1b]9;", "4;3\a", "\x1b]9;4;4\x1b\\", "\x1b]9;4;1;50\a", "\x1b]9;4;0\a", "\x1b]9;4;2\a", "\x1b]9;4;0\a"},
				[]protocol.TerminalActivity{none, none, protocol.TerminalActivityWorking, protocol.TerminalActivityWaitingForInput, protocol.TerminalActivityWorking, protocol.TerminalActivityIdle, protocol.TerminalActivityFailed, protocol.TerminalActivityIdle}},
			{"shell marks",
				[]string{"\x1b]133;C\a", "waiting for a task", "\x1b]133;D;1\a", "\x1b]633;C\x1b\\", "\x1b]633;D;0\x1b\\", "\x1b]9;4;3\a", "\x1b]133;A\a", "\x1b]9;4;4\a", "\x1b]633;B\a"},
				[]protocol.TerminalActivity{protocol.TerminalActivityIdle, none, none, none, none, protocol.TerminalActivityWorking, protocol.TerminalActivityIdle, protocol.TerminalActivityWaitingForInput, protocol.TerminalActivityIdle}},
			{"unrelated and cancelled",
				[]string{"9;4;3\x1b]2;9;4;3\a\x1b]9;4;3\x18\a", "\x1b]9;4;garbage\a", "\x1b]9;4\a\x1b]133;AB\a\x1b]1337;A\a", "\x1b]9;4;0\a"},
				[]protocol.TerminalActivity{none, none, none, protocol.TerminalActivityIdle}},
		} {
			var tracker activityTracker
			for index, step := range test.steps {
				activity, changed := tracker.update([]byte(step))
				if !changed {
					activity = none
				}
				if activity != test.want[index] {
					t.Fatalf("%s step %d = %q, want %q", test.name, index, activity, test.want[index])
				}
			}
		}

		var reports []protocol.TerminalActivityReport
		value := &handle{info: protocol.TerminalInfo{Status: protocol.TerminalStatus{Kind: "running"}}}
		value.service = &Service{changed: func(info protocol.TerminalInfo) { reports = append(reports, *info.Activity) }}
		value.track([]byte("\x1b]9;4;3\a"))
		value.track([]byte("still working\x1b]9;4;3\a"))
		value.track([]byte("\x1b]133;D\a"))
		value.info.Status.Kind = "exited"
		value.track([]byte("\x1b]9;4;2\a"))
		if want := []protocol.TerminalActivityReport{{State: protocol.TerminalActivityWorking, Sequence: 1}, {State: protocol.TerminalActivityIdle, Sequence: 2}}; !slices.Equal(reports, want) {
			t.Fatalf("activity reports = %v, want %v", reports, want)
		}
	})

	t.Run("ssh startup output", func(t *testing.T) {
		viewport := protocol.TerminalViewport{Columns: 80, Rows: 24}
		connection, err := OpenSSH(testShellServer(t, []byte("ready")), viewport)
		if err != nil {
			t.Fatal(err)
		}
		output, err := io.ReadAll(connection)
		if err != nil || string(output) != "ready" {
			t.Fatalf("output = %q %v", output, err)
		}
		if status := connection.status(); status.Kind != "exited" || status.Data != (protocol.TerminalStatusExitedPayload{Code: 3}) {
			t.Fatalf("status = %+v", status)
		}
		_ = connection.Close()

		// More than the limit and the channel window together: the server
		// cannot confirm the shell before the limit is seen.
		_, err = OpenSSH(testShellServer(t, make([]byte, 3<<20)), viewport)
		var failure *protocol.Fault
		if !errors.As(err, &failure) || failure.Code != protocol.ErrorCodeOutcomeUnknown || failure.Message != "SSH terminal startup output exceeds the limit" {
			t.Fatalf("flooded opening = %v", err)
		}
	})
}

// testShellServer connects to a loopback SSH server whose shell writes
// startup before confirming the request, then exits with status 3.
func testShellServer(t *testing.T, startup []byte) *cryptossh.Client {
	t.Helper()
	_, key, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	signer, err := cryptossh.NewSignerFromKey(key)
	if err != nil {
		t.Fatal(err)
	}
	config := &cryptossh.ServerConfig{NoClientAuth: true}
	config.AddHostKey(signer)
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	go func() {
		connection, err := listener.Accept()
		if err != nil {
			return
		}
		defer connection.Close()
		_, channels, requests, err := cryptossh.NewServerConn(connection, config)
		if err != nil {
			return
		}
		go cryptossh.DiscardRequests(requests)
		for offered := range channels {
			channel, requests, err := offered.Accept()
			if err != nil {
				return
			}
			for request := range requests {
				if request.Type != "shell" {
					_ = request.Reply(true, nil)
					continue
				}
				_, _ = channel.Write(startup)
				_ = request.Reply(true, nil)
				_, _ = channel.SendRequest("exit-status", false, cryptossh.Marshal(struct{ Status uint32 }{3}))
				_ = channel.Close()
			}
		}
	}()
	client, err := cryptossh.Dial("tcp", listener.Addr().String(), &cryptossh.ClientConfig{User: "test", HostKeyCallback: cryptossh.InsecureIgnoreHostKey()})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = client.Close() })
	return client
}
