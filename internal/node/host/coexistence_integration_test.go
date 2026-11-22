//go:build integration && !windows

package host

import (
	"archive/tar"
	"bufio"
	"bytes"
	"compress/gzip"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/identity"
	"github.com/sailry/sailry-harness/internal/node/wsl"
	"github.com/sailry/sailry-harness/internal/protocol"
	cryptossh "golang.org/x/crypto/ssh"
)

func TestHostCoexistence(t *testing.T) {
	const (
		officialRepo = "sailry/sailry-harness"
		forkRepo     = "stage3-fixture/fork-host"
		forkName     = "fixture"
		forkAgent    = "org.fixture.host"
		official     = "4.5.6"
		first        = "7.8.9"
		second       = "7.9.0"
	)
	fork, err := identity.IndependentHost(forkRepo, forkName, forkAgent)
	if err != nil {
		t.Fatal(err)
	}
	// The SSH identity probe connects to an absolute Unix socket path.
	if runtime.GOOS == "darwin" {
		t.Setenv("TMPDIR", "/tmp")
	}
	root := t.TempDir()
	for _, route := range []struct{ name, platform, arch, triple string }{
		{"ssh-linux", "Linux", "x86_64", "x86_64-unknown-linux-gnu"},
		{"ssh-macos", "Darwin", "arm64", "aarch64-apple-darwin"},
		{"wsl", "Linux", "x86_64", "x86_64-unknown-linux-gnu"},
	} {
		t.Run(route.name, func(t *testing.T) {
			mac := route.platform == "Darwin"
			if mac && runtime.GOOS != "darwin" {
				t.Skip("PlistBuddy is only available on macOS")
			}
			dir := filepath.Join(root, route.name)
			home := filepath.Join(dir, "home")
			writeFixture(t, filepath.Join(dir, "bin", "uname"), fmt.Appendf(nil, "#!/bin/sh\ncase \"$1\" in -s) echo %s;; -m) echo %s;; *) exit 1;; esac\n", route.platform, route.arch), 0o700)
			fixtureTool := readFixture(t, "testdata/coexistence/tool.sh")
			for _, name := range []string{"id", "curl", "systemctl", "loginctl", "launchctl", "sha256sum", "getconf", "cat"} {
				writeFixture(t, filepath.Join(dir, "bin", name), fixtureTool, 0o700)
			}
			if err := os.MkdirAll(home, 0o700); err != nil {
				t.Fatal(err)
			}
			python, err := exec.LookPath("python3")
			if err != nil {
				t.Fatal(err)
			}
			if err := os.Symlink(python, filepath.Join(dir, "bin", "python3")); err != nil {
				t.Fatal(err)
			}
			environment := []string{"HOME=" + home, "PATH=" + filepath.Join(dir, "bin") + ":/usr/bin:/bin", "TMPDIR=" + dir, "SAILRY_FIXTURE=" + dir, "LC_ALL=C"}
			for _, release := range []struct{ repo, name, agent, version string }{
				{officialRepo, "sailry", "ai.sailry.host", official},
				{forkRepo, forkName, forkAgent, first},
				{forkRepo, forkName, forkAgent, second},
			} {
				fixtureHostRelease(t, dir, release.repo, release.name, release.agent, release.version, route.triple)
			}

			run := func(script string) {
				t.Helper()
				command := exec.CommandContext(t.Context(), "/bin/sh", "-s")
				command.Dir, command.Env, command.Stdin = dir, environment, strings.NewReader(script)
				if output, err := command.CombinedOutput(); err != nil {
					t.Fatalf("installer: %v\n%s", err, output)
				}
			}
			runBootstrap := func(release identity.HostRelease, version string, update bool) {
				t.Helper()
				script, err := wsl.ReleaseScriptFor(release, version, update, nil)
				if err != nil {
					t.Fatal(err)
				}
				if !update && route.name == "wsl" {
					script = wsl.PreflightScript(release) + "\n" + script
				}
				run(script)
			}
			runBootstrap(identity.OfficialHost(), official, false)
			officialState := checkFixtureHost(t, home, environment, "sailry", "ai.sailry.host", official, mac)
			writeFixture(t, filepath.Join(home, ".sailry-host", "record"), []byte("official session data\n"), 0o600)
			officialBackup := filepath.Join(home, ".local", "lib", ".sailry-previous", "record")
			writeFixture(t, officialBackup, []byte("official backup\n"), 0o600)
			serviceLog := filepath.Join(dir, "service-log")
			baseline := readFixture(t, serviceLog)
			wrongInstaller := filepath.Join(dir, officialRepo, "releases", "download", "v"+official, "install-host.sh")
			command := exec.CommandContext(t.Context(), "/bin/bash", wrongInstaller, "--expect-package", "fixture/org.fixture.host")
			command.Env = environment
			if output, err := command.CombinedOutput(); err == nil || !strings.Contains(string(output), "Host installer package does not match") {
				t.Fatalf("mismatched installer was not refused: %v: %s", err, output)
			}

			install := func(version string, expected *protocol.NodeID) error {
				runBootstrap(fork, version, expected != nil)
				return nil
			}
			if route.name != "wsl" {
				connection := fixtureHostSSH(t, dir, environment)
				owner := NewInstallations(DeploymentOptions{Release: fork})
				install = func(version string, expected *protocol.NodeID) error {
					return owner.installRelease(t.Context(), connection, protocol.NodeID{}, protocol.RequestID{}, expected, nil, version)
				}
			}
			if err := install(first, nil); err != nil {
				t.Fatal(err)
			}
			forkState := checkFixtureHost(t, home, environment, forkName, forkAgent, first, mac)
			writeFixture(t, filepath.Join(home, ".fixture-host", "record"), []byte("fork session data\n"), 0o600)
			forkNode := protocol.NodeID{7}
			if route.name != "wsl" {
				fixtureHostControl(t, filepath.Join(home, ".fixture-host", "control.sock"), forkNode)
				wrongNode := protocol.NodeID{4}
				err := install(second, &wrongNode)
				var fault *protocol.Fault
				if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeWrongTarget {
					t.Fatalf("independent update accepted another Node: %v", err)
				}
			}
			beforeUpgrade := readFixture(t, serviceLog)
			if err := install(second, &forkNode); err != nil {
				t.Fatal(err)
			}
			calls, ok := strings.CutPrefix(string(readFixture(t, serviceLog)), string(beforeUpgrade))
			stop, start := "--user stop fixture-host.service\n", "--user start fixture-host.service\n"
			if mac {
				stop = "bootout gui/1001/org.fixture.host\n"
				start = "bootstrap gui/1001 " + filepath.Join(home, "Library", "LaunchAgents", "org.fixture.host.plist") + "\n"
			}
			if !ok || strings.Index(calls, stop) == -1 || strings.Index(calls, start) <= strings.Index(calls, stop) {
				t.Fatalf("upgrade did not restart the fork service: %q", calls)
			}
			updated := checkFixtureHost(t, home, environment, forkName, forkAgent, second, mac)
			if updated.unit != forkState.unit || updated.command != forkState.command {
				t.Fatal("upgrade changed the fork service or command identity")
			}
			if backup := readFixture(t, filepath.Join(home, ".local", "lib", ".fixture-previous", "fixture-host")); string(backup) != forkState.binary {
				t.Fatal("upgrade did not preserve the previous fork executable")
			}
			if got := string(readFixture(t, officialBackup)); got != "official backup\n" {
				t.Fatalf("official backup changed: %q", got)
			}
			if got := string(readFixture(t, filepath.Join(home, ".sailry-host", "record"))); got != "official session data\n" {
				t.Fatalf("official profile changed: %q", got)
			}
			if got := string(readFixture(t, filepath.Join(home, ".fixture-host", "record"))); got != "fork session data\n" {
				t.Fatalf("fork profile changed: %q", got)
			}
			serviceCalls, ok := bytes.CutPrefix(readFixture(t, serviceLog), baseline)
			if !ok || strings.Contains(string(serviceCalls), "sailry-host.service") || strings.Contains(string(serviceCalls), "ai.sailry.host") {
				t.Fatalf("fork touched the official service: %s", serviceCalls)
			}
			if completed := checkFixtureHost(t, home, environment, "sailry", "ai.sailry.host", official, mac); completed != officialState {
				t.Fatal("fork install or update changed the official files")
			}
			var expected strings.Builder
			for _, release := range []struct{ repo, name, version string }{
				{officialRepo, "sailry", official}, {forkRepo, forkName, first}, {forkRepo, forkName, second},
			} {
				for _, asset := range fixtureHostAssets(release.name, route.triple) {
					fmt.Fprintf(&expected, "https://github.com/%s/releases/download/v%s/%s\n", release.repo, release.version, asset)
				}
			}
			if actual := string(readFixture(t, filepath.Join(dir, "downloads"))); actual != expected.String() {
				t.Fatalf("wrong repository, release or asset selected:\n%s\nwant:\n%s", actual, expected.String())
			}
		})
	}
}

func fixtureHostSSH(t *testing.T, dir string, environment []string) *cryptossh.Client {
	t.Helper()
	connection, _ := deploymentSSHExec(t, "", func(command string, channel cryptossh.Channel, request *cryptossh.Request) {
		if command != "sh -s" {
			_ = request.Reply(false, nil)
			return
		}
		_ = request.Reply(true, nil)
		shell := exec.CommandContext(t.Context(), "/bin/sh", "-s")
		shell.Dir, shell.Env, shell.Stdin = dir, environment, channel
		var stdout, stderr bytes.Buffer
		shell.Stdout, shell.Stderr = &stdout, &stderr
		status := uint32(0)
		if err := shell.Run(); err != nil {
			var exit *exec.ExitError
			if errors.As(err, &exit) && exit.ExitCode() >= 0 {
				status = uint32(exit.ExitCode())
			} else {
				status = 1
			}
			t.Logf("SSH fixture shell: %v: %s", err, stderr.String())
		}
		deploymentSSHResult(channel, stdout.String(), stderr.String(), status)
	})
	return connection
}

func fixtureHostAssets(name, triple string) []string {
	return []string{"install-host.sh", "SHA256SUMS-installer", name + "-host-" + triple + ".tar.gz", "SHA256SUMS-" + triple}
}

func fixtureHostRelease(t *testing.T, dir, repo, name, agent, version, triple string) {
	t.Helper()
	// A fork publishes its own package scripts and executable name. Only those
	// build-time identities differ from the official installer and service scripts.
	stamp := strings.NewReplacer("task_package=sailry\n", "task_package="+name+"\n",
		"task_label=ai.sailry.host\n", "task_label="+agent+"\n",
		"task_repository=https://github.com/sailry/sailry-harness\n", "task_repository=https://github.com/"+repo+"\n")
	files := map[string][]byte{
		name + "-host":          []byte("#!/bin/sh\ntest \"$1\" = --version || exit 1\nprintf '%s\\n' 'Sailry Host " + version + "'\n"),
		"python/bin/python3.12": []byte("#!/bin/sh\nexec python3 \"$@\"\n"),
	}
	for _, file := range []string{"install-host.sh", "sailry", "service.sh"} {
		body := stamp.Replace(string(readFixture(t, filepath.Join("deployment", "package", file))))
		if file == "sailry" {
			file = name
		}
		files[file] = []byte(body)
	}
	var buffer bytes.Buffer
	compressed := gzip.NewWriter(&buffer)
	archive := tar.NewWriter(compressed)
	for file, body := range files {
		if err := archive.WriteHeader(&tar.Header{Name: file, Mode: 0o700, Size: int64(len(body))}); err != nil {
			t.Fatal(err)
		}
		if _, err := archive.Write(body); err != nil {
			t.Fatal(err)
		}
	}
	if err := errors.Join(archive.Close(), compressed.Close()); err != nil {
		t.Fatal(err)
	}
	base := filepath.Join(dir, repo, "releases", "download", "v"+version)
	assets := fixtureHostAssets(name, triple)
	for i, body := range [][]byte{files[assets[0]], buffer.Bytes()} {
		asset := assets[i*2]
		writeFixture(t, filepath.Join(base, asset), body, 0o600)
		writeFixture(t, filepath.Join(base, assets[i*2+1]), fmt.Appendf(nil, "%x  %s\n", sha256.Sum256(body), asset), 0o600)
	}
}

type fixtureHostState struct{ binary, unit, command string }

func checkFixtureHost(t *testing.T, home string, environment []string, name, agent, version string, mac bool) fixtureHostState {
	t.Helper()
	program := filepath.Join(home, ".local", "lib", name)
	command := filepath.Join(home, ".local", "bin", name)
	binary := filepath.Join(program, name+"-host")
	profile := filepath.Join(home, "."+name+"-host")
	if info, err := os.Stat(profile); err != nil || !info.IsDir() {
		t.Fatalf("missing profile %s: %v", profile, err)
	}
	link, err := os.Readlink(command)
	if err != nil || link != filepath.Join(program, name) {
		t.Fatalf("command points to %q: %v", link, err)
	}
	for _, invocation := range []struct {
		path, argument, want string
	}{
		{binary, "--version", "Sailry Host " + version + "\n"},
		{command, "version", "Sailry Host " + version + "\n"},
		{command, "status", ""},
	} {
		probe := exec.CommandContext(t.Context(), invocation.path, invocation.argument)
		probe.Env = environment
		if output, err := probe.CombinedOutput(); err != nil || string(output) != invocation.want {
			t.Fatalf("%s %s: %q, %v", invocation.path, invocation.argument, output, err)
		}
	}
	unit := filepath.Join(home, ".config", "systemd", "user", name+"-host.service")
	want := `ExecStart="%h/.local/lib/` + name + `/` + name + `-host" --data-dir "%h/.` + name + `-host"`
	if mac {
		unit = filepath.Join(home, "Library", "LaunchAgents", agent+".plist")
		for key, value := range map[string]string{"Label": agent, "ProgramArguments:0": binary, "ProgramArguments:1": "--data-dir", "ProgramArguments:2": profile} {
			print := exec.CommandContext(t.Context(), "/usr/libexec/PlistBuddy", "-c", "Print :"+key, unit)
			if output, err := print.CombinedOutput(); err != nil || string(output) != value+"\n" {
				t.Fatalf("LaunchAgent %s: %q, %v", key, output, err)
			}
		}
	} else if body := string(readFixture(t, unit)); !strings.Contains(body, want+" ") {
		t.Fatalf("unit %s did not use executable and profile %s: %s", unit, want, body)
	}
	return fixtureHostState{string(readFixture(t, binary)), string(readFixture(t, unit)), link}
}

func fixtureHostControl(t *testing.T, path string, node protocol.NodeID) {
	t.Helper()
	listener, err := net.Listen("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(path, 0o600); err != nil {
		_ = listener.Close()
		t.Fatal(err)
	}
	response, err := json.Marshal(map[string]any{"version": 1, "body": map[string]string{"kind": "hello"}, "node": node})
	if err != nil {
		_ = listener.Close()
		t.Fatal(err)
	}
	response = append(response, '\n')
	done := make(chan struct{})
	go func() {
		defer close(done)
		for {
			connection, err := listener.Accept()
			if err != nil {
				return
			}
			_ = connection.SetDeadline(time.Now().Add(5 * time.Second))
			if request, err := bufio.NewReader(connection).ReadString('\n'); err == nil && request == "{\"command\":\"node_hello\",\"version\":1}\n" {
				_, _ = connection.Write(response)
			}
			_ = connection.Close()
		}
	}()
	t.Cleanup(func() { _ = listener.Close(); <-done })
}

func readFixture(t *testing.T, path string) []byte {
	t.Helper()
	body, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return body
}

func writeFixture(t *testing.T, path string, body []byte, mode os.FileMode) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, body, mode); err != nil {
		t.Fatal(err)
	}
}
