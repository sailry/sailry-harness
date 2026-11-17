//go:build integration && !windows

package host

import (
	"archive/tar"
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

func freshHostInstaller(t *testing.T) (directory, home, installer, tools string) {
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
		"sailry-host": []byte("#!/bin/sh\nprintf '%s\\n' 'Sailry Host " + link.ProductVersion + "'\n"),
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
