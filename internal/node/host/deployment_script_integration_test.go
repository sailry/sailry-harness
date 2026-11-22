//go:build integration && !windows

package host

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
)

func TestSSHUpdateRecoveryFailure(t *testing.T) {
	source, err := os.ReadFile("deployment/package/install-host.sh")
	if err != nil {
		t.Fatal(err)
	}
	_, functions, found := strings.Cut(strings.ReplaceAll(string(source), "\r\n", "\n"), "task_stop() {\n")
	if !found {
		t.Fatal("installer recovery functions are missing")
	}
	functions, _, found = strings.Cut("task_stop() {\n"+functions, "\ntrap task_cleanup EXIT")
	if !found {
		t.Fatal("installer recovery boundary is missing")
	}
	for _, fixture := range []struct {
		name, state, message string
	}{
		{"rollback", "task_starting=1\ntask_changing=1\n", "Host recovery failed"},
		{"backup restoration", "task_starting=0\ntask_changing=0\n", "Host backup recovery failed"},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			directory := t.TempDir()
			for name, body := range map[string]string{
				"backup/sailry":                "#!/bin/sh\nexit 1\n",
				"backup/marker":                "existing backup",
				"stage/marker":                 "recovery stage",
				"stage/previous-backup/marker": "previous backup",
			} {
				path := filepath.Join(directory, filepath.FromSlash(name))
				if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
					t.Fatal(err)
				}
				if err := os.WriteFile(path, []byte(body), 0o700); err != nil {
					t.Fatal(err)
				}
			}
			// Run the installer's actual recovery functions against isolated
			// files. Its managed stop command refuses before any service call.
			script := "set -euo pipefail\n" + functions + `
task_package=sailry
task_root="$PWD/current"
task_existing="$PWD/previous"
task_backup="$PWD/backup"
task_stage="$PWD/stage"
task_unit="$PWD/unit"
task_command="$PWD/command"
task_migrate=0
task_had_command=0
task_active=0
task_complete=0
task_update=1
` + fixture.state + "trap task_cleanup EXIT\nexit 9\n"
			command := exec.CommandContext(t.Context(), "/bin/bash", "-s")
			command.Dir, command.Env = directory, []string{"PATH=/usr/bin:/bin"}
			command.Stdin = strings.NewReader(script)
			output, err := command.CombinedOutput()
			var exit *exec.ExitError
			if !errors.As(err, &exit) || exit.ExitCode() != 73 || !strings.Contains(string(output), fixture.message) {
				t.Fatalf("unconfirmed recovery became a definite failure: %v, %s", err, output)
			}
			for name, want := range map[string]string{"backup/marker": "existing backup", "stage/marker": "recovery stage", "stage/previous-backup/marker": "previous backup"} {
				body, err := os.ReadFile(filepath.Join(directory, filepath.FromSlash(name)))
				if err != nil || string(body) != want {
					t.Fatalf("recovery evidence %s was changed: %q, %v", name, body, err)
				}
			}
		})
	}
}

func TestFreshHostInstallRecovery(t *testing.T) {
	for _, fixture := range []struct {
		name, failure string
		retainData    bool
		cleanupFails  bool
	}{
		{name: "linger refused", failure: "linger"},
		{name: "unit reload refused", failure: "reload"},
		{name: "enable refused", failure: "enable"},
		{name: "activation refused", failure: "active"},
		{name: "started host wrote data", failure: "active", retainData: true},
		{name: "service cleanup refused", failure: "active", cleanupFails: true},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			directory, home, installer, tools := freshHostInstaller(t)
			run := func(failure string, data, cleanup bool) ([]byte, error) {
				command := exec.CommandContext(t.Context(), "/bin/bash", installer, "--version", link.ProductVersion)
				command.Env = append(os.Environ(),
					"PATH="+tools+string(os.PathListSeparator)+os.Getenv("PATH"),
					"SAILRY_INSTALL_FIXTURE="+directory,
					"SAILRY_INSTALL_FIXTURE_HOME="+home,
					"SAILRY_INSTALL_FAIL="+failure,
					fmt.Sprintf("SAILRY_INSTALL_RETAIN_DATA=%t", data),
					fmt.Sprintf("SAILRY_INSTALL_CLEANUP_FAILS=%t", cleanup),
				)
				return command.CombinedOutput()
			}
			output, err := run(fixture.failure, fixture.retainData, fixture.cleanupFails)
			var exited *exec.ExitError
			if !errors.As(err, &exited) {
				t.Fatalf("failed service installation returned %v: %s", err, output)
			}
			root := filepath.Join(home, ".local", "lib", "sailry")
			command := filepath.Join(home, ".local", "bin", "sailry")
			unit := filepath.Join(home, ".config", "systemd", "user", "sailry-host.service")
			profile := filepath.Join(home, ".sailry-host")
			if fixture.cleanupFails {
				if exited.ExitCode() != 73 || !strings.Contains(string(output), "Host recovery failed") {
					t.Fatalf("uncertain cleanup became a definite failure: %v, %s", err, output)
				}
				for _, path := range []string{root, command, unit, profile} {
					if _, err := os.Lstat(path); err != nil {
						t.Fatalf("uncertain cleanup removed %s: %v", path, err)
					}
				}
				stages, err := filepath.Glob(filepath.Join(home, ".local", "lib", ".sailry-install-*"))
				if err != nil || len(stages) != 1 {
					t.Fatalf("recovery stage was not retained: %v, %v", stages, err)
				}
				return
			}
			if exited.ExitCode() != 1 {
				t.Fatalf("installation failure code = %d: %s", exited.ExitCode(), output)
			}
			for _, path := range []string{root, command, unit} {
				if _, err := os.Lstat(path); !errors.Is(err, os.ErrNotExist) {
					t.Fatalf("failed installation left %s: %v", path, err)
				}
			}
			if fixture.retainData {
				body, err := os.ReadFile(filepath.Join(profile, "record"))
				if err != nil || string(body) != "preserved data\n" {
					t.Fatalf("failed installation changed profile data: %q, %v", body, err)
				}
				return
			}
			if _, err := os.Stat(profile); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("failed installation left an empty profile: %v", err)
			}
			if output, err := run("", false, false); err != nil {
				t.Fatalf("retry after confirmed cleanup failed: %v, %s", err, output)
			}
			for _, path := range []string{root, command, unit, profile} {
				if _, err := os.Lstat(path); err != nil {
					t.Fatalf("successful retry did not create %s: %v", path, err)
				}
			}
		})
	}
}

func TestPublishedHostInstallPassesRelayArguments(t *testing.T) {
	directory, home, installer, tools := freshHostInstaller(t)
	command := exec.CommandContext(t.Context(), "/bin/bash", installer, "--version", link.ProductVersion,
		"--relay", deploymentTestTCPRelay, "--relay", deploymentTestQUICRelay)
	command.Env = append(os.Environ(),
		"PATH="+tools+string(os.PathListSeparator)+os.Getenv("PATH"),
		"SAILRY_INSTALL_FIXTURE="+directory,
		"SAILRY_INSTALL_FIXTURE_HOME="+home,
		"SAILRY_INSTALL_FAIL=",
		"SAILRY_INSTALL_RETAIN_DATA=false",
		"SAILRY_INSTALL_CLEANUP_FAILS=false",
	)
	if output, err := command.CombinedOutput(); err != nil {
		t.Fatalf("install with custom relays: %v: %s", err, output)
	}
	unit, err := os.ReadFile(filepath.Join(home, ".config", "systemd", "user", "sailry-host.service"))
	if err != nil {
		t.Fatal(err)
	}
	want := `ExecStart="%h/.local/lib/sailry/sailry-host" --data-dir "%h/.sailry-host" "--relay" "` + deploymentTestTCPRelay +
		`" "--relay" "` + deploymentTestQUICRelay + `" --bootstrap`
	for line := range strings.SplitSeq(string(unit), "\n") {
		if strings.HasPrefix(line, "ExecStart=") {
			if line != want {
				t.Fatalf("service argv = %q, want %q", line, want)
			}
			return
		}
	}
	t.Fatal("Host service has no ExecStart")
}

func TestFreshHostInstallSerializesConcurrentPublication(t *testing.T) {
	directory, home, installer, tools := freshHostInstaller(t)
	command := func() *exec.Cmd {
		value := exec.CommandContext(t.Context(), "/bin/bash", installer, "--version", link.ProductVersion)
		value.Env = append(os.Environ(),
			"PATH="+tools+string(os.PathListSeparator)+os.Getenv("PATH"),
			"SAILRY_INSTALL_FIXTURE="+directory,
			"SAILRY_INSTALL_FIXTURE_HOME="+home,
			"SAILRY_INSTALL_FAIL=",
			"SAILRY_INSTALL_RETAIN_DATA=false",
			"SAILRY_INSTALL_CLEANUP_FAILS=false",
		)
		return value
	}
	root := filepath.Join(home, ".local", "lib", "sailry")
	assertHostInstallExclusive(t, directory, tools, root, command(), command())
	for _, path := range []string{filepath.Join(root, "sailry-host"), filepath.Join(home, ".local", "bin", "sailry"), filepath.Join(home, ".config", "systemd", "user", "sailry-host.service")} {
		if _, err := os.Lstat(path); err != nil {
			t.Fatalf("the first installation lost %s: %v", path, err)
		}
	}
}

func TestPublishedSSHUpdateRejectsStaleVersion(t *testing.T) {
	directory, home, installer, tools := freshHostInstaller(t)
	root := filepath.Join(home, ".local", "lib", "sailry")
	if err := os.MkdirAll(root, 0o700); err != nil {
		t.Fatal(err)
	}
	current := []byte("#!/bin/sh\nprintf '%s\\n' 'Sailry Host 99.0.0'\n")
	binary := filepath.Join(root, "sailry-host")
	if err := os.WriteFile(binary, current, 0o700); err != nil {
		t.Fatal(err)
	}
	serviceLog := filepath.Join(directory, "service-called")
	commandPath := filepath.Join(root, "sailry")
	const service = `#!/bin/sh
: > "$SAILRY_INSTALL_FIXTURE/service-called"
exit 1
`
	if err := os.WriteFile(commandPath, []byte(service), 0o700); err != nil {
		t.Fatal(err)
	}
	command := exec.CommandContext(t.Context(), "/bin/bash", installer, "--version", link.ProductVersion, "--update")
	command.Env = append(os.Environ(),
		"PATH="+tools+string(os.PathListSeparator)+os.Getenv("PATH"),
		"SAILRY_INSTALL_FIXTURE="+directory,
		"SAILRY_INSTALL_FIXTURE_HOME="+home,
		"SAILRY_HOST_EXPECT_CURRENT_VERSION=Sailry Host 0.1.0",
	)
	output, err := command.CombinedOutput()
	if err == nil || !strings.Contains(string(output), "Host version changed after verification") {
		t.Fatalf("stale update returned %v: %s", err, output)
	}
	if actual, err := os.ReadFile(binary); err != nil || !bytes.Equal(actual, current) {
		t.Fatalf("stale update touched the installed Host: %q, %v", actual, err)
	}
	if _, err := os.Stat(serviceLog); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("stale update touched the service: %v", err)
	}
}

func assertHostInstallExclusive(t *testing.T, directory, tools, root string, first, second *exec.Cmd) {
	t.Helper()
	ready, release := filepath.Join(directory, "before-publication"), filepath.Join(directory, "allow-publication")
	move := `#!/bin/sh
set -eu
if test "$#" -eq 2 && test "$2" = "$SAILRY_INSTALL_OVERLAP_ROOT"; then
  : > "$SAILRY_INSTALL_OVERLAP_READY"
  while ! test -e "$SAILRY_INSTALL_OVERLAP_RELEASE"; do sleep 0.02; done
fi
exec /bin/mv "$@"
`
	if err := os.WriteFile(filepath.Join(tools, "mv"), []byte(move), 0o700); err != nil {
		t.Fatal(err)
	}
	first.Env = append(first.Env, "SAILRY_INSTALL_OVERLAP_ROOT="+root, "SAILRY_INSTALL_OVERLAP_READY="+ready, "SAILRY_INSTALL_OVERLAP_RELEASE="+release)
	second.Env = append(second.Env, "SAILRY_INSTALL_OVERLAP_ROOT=")
	var firstOutput bytes.Buffer
	first.Stdout, first.Stderr = &firstOutput, &firstOutput
	if err := first.Start(); err != nil {
		t.Fatal(err)
	}
	waited := false
	defer func() {
		if !waited {
			_ = os.WriteFile(release, nil, 0o600)
			_ = first.Wait()
		}
	}()
	for deadline := time.Now().Add(10 * time.Second); ; time.Sleep(5 * time.Millisecond) {
		if _, err := os.Stat(ready); err == nil {
			break
		} else if !errors.Is(err, os.ErrNotExist) || time.Now().After(deadline) {
			_ = os.WriteFile(release, nil, 0o600)
			waitErr := first.Wait()
			waited = true
			t.Fatalf("first installer did not reach publication: %v, %v, %s", err, waitErr, firstOutput.String())
		}
	}
	secondOutput, secondErr := second.CombinedOutput()
	if err := os.WriteFile(release, nil, 0o600); err != nil {
		t.Fatal(err)
	}
	firstErr := first.Wait()
	waited = true
	if firstErr != nil || secondErr == nil || !strings.Contains(string(secondOutput), "Another Host installation or update is in progress") {
		t.Fatalf("overlapping installs returned first=%v (%s), second=%v (%s)", firstErr, firstOutput.String(), secondErr, secondOutput)
	}
}

func freshHostInstaller(t *testing.T) (directory, home, installer, tools string) {
	t.Helper()
	return freshHostInstallerForVersion(t, link.ProductVersion)
}

func freshHostInstallerForVersion(t *testing.T, version string) (directory, home, installer, tools string) {
	t.Helper()
	directory = t.TempDir()
	home = filepath.Join(directory, "home")
	tools = filepath.Join(directory, "bin")
	write := func(path, body string) {
		t.Helper()
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(body), 0o700); err != nil {
			t.Fatal(err)
		}
	}
	script, err := os.ReadFile("deployment/package/install-host.sh")
	if err != nil {
		t.Fatal(err)
	}
	installer = filepath.Join(directory, "install-host.sh")
	// Relocate only the install paths. The process keeps its actual HOME and
	// every service, identity and network command below is a local fixture.
	write(installer, strings.ReplaceAll(string(script), "$HOME", "$SAILRY_INSTALL_FIXTURE_HOME"))
	files := map[string][]byte{
		"sailry-host":           []byte("#!/bin/sh\nprintf '%s\\n' 'Sailry Host " + version + "'\n"),
		"python/bin/python3.12": []byte("#!/bin/sh\nexec python3 \"$@\"\n"),
	}
	for _, name := range []string{"sailry", "service.sh"} {
		body, err := os.ReadFile("deployment/package/" + name)
		if err != nil {
			t.Fatal(err)
		}
		files[name] = []byte(strings.ReplaceAll(string(body), "$HOME", "$SAILRY_INSTALL_FIXTURE_HOME"))
	}
	archive := filepath.Join(directory, "archive.tar.gz")
	file, err := os.Create(archive)
	if err != nil {
		t.Fatal(err)
	}
	compressed := gzip.NewWriter(file)
	tarball := tar.NewWriter(compressed)
	for name, body := range files {
		if err := tarball.WriteHeader(&tar.Header{Name: name, Mode: 0o700, Size: int64(len(body))}); err != nil {
			t.Fatal(err)
		}
		if _, err := tarball.Write(body); err != nil {
			t.Fatal(err)
		}
	}
	if err := errors.Join(tarball.Close(), compressed.Close(), file.Close()); err != nil {
		t.Fatal(err)
	}
	body, err := os.ReadFile(archive)
	if err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256(body)
	write(filepath.Join(directory, "checksums"), hex.EncodeToString(digest[:])+"  sailry-host-x86_64-unknown-linux-gnu.tar.gz\n")
	for name, body := range map[string]string{
		"uname": `case "$1" in -s) echo Linux;; -m) echo x86_64;; *) exit 1;; esac`,
		"id":    `case "$1" in -u) echo 1001;; -un) echo fixture;; *) exit 1;; esac`,
		"curl": `
while test "$#" -gt 0; do
  if test "$1" = --output; then destination=$2; shift 2; else shift; fi
done
case "$destination" in
  *.tar.gz) cp "$SAILRY_INSTALL_FIXTURE/archive.tar.gz" "$destination";;
  *) cp "$SAILRY_INSTALL_FIXTURE/checksums" "$destination";;
esac`,
		"loginctl": `test "$SAILRY_INSTALL_FAIL" != linger`,
		"systemctl": `
case "$*" in
  *"disable --now"*) test "$SAILRY_INSTALL_CLEANUP_FAILS" != true;;
  *"daemon-reload"*) test "$SAILRY_INSTALL_FAIL" != reload;;
  *"enable --now"*) test "$SAILRY_INSTALL_FAIL" != enable;;
  *"is-active"*)
    if test "$SAILRY_INSTALL_RETAIN_DATA" = true; then
      echo 'preserved data' > "$SAILRY_INSTALL_FIXTURE_HOME/.sailry-host/record"
    fi
    test "$SAILRY_INSTALL_FAIL" != active;;
  *) exit 1;;
esac`,
	} {
		write(filepath.Join(tools, name), "#!/bin/sh\nset -eu\n"+body+"\n")
	}
	if _, err := exec.LookPath("sha256sum"); err != nil {
		write(filepath.Join(tools, "sha256sum"), "#!/bin/sh\nexec shasum -a 256 \"$@\"\n")
	}
	return directory, home, installer, tools
}
