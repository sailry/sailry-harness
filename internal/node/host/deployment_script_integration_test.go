//go:build integration && !windows

package host

import (
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
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
