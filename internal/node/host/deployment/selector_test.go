//go:build !windows

package deployment

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

func TestInstalledCodeWrapperDiscoveryBounds(t *testing.T) {
	const repository = "https://github.com/sailry/sailry-code"
	const maxBytes = 8 * 1024 * 1024
	directory, stage, environment := hostArchiveFixture(t, repository, "0.1.0")
	_, nextStage, _ := hostArchiveFixture(t, repository, "0.1.1")
	publishInstallerArchive(t, directory, repository, "0.1.0", stage)
	publishInstallerArchive(t, directory, repository, "0.1.1", nextStage)
	encode := func(releases []map[string]any) []byte {
		t.Helper()
		body, err := json.Marshal(releases)
		if err != nil {
			t.Fatal(err)
		}
		return body
	}
	fullPage := func(version string) []byte {
		entries := []map[string]any{installerReleaseFixture("v" + version)}
		for len(entries) < 100 {
			entries = append(entries, installerReleaseFixture(fmt.Sprintf("mobile-v1.0.%d", len(entries))))
		}
		return encode(entries)
	}
	sizedPage := func(size int) []byte {
		entry := installerReleaseFixture("v0.1.0")
		entry["body"] = ""
		entry["body"] = strings.Repeat("x", size-len(encode([]map[string]any{entry})))
		return encode([]map[string]any{entry})
	}
	currentPage, newerPage := fullPage("0.1.0"), fullPage("0.1.1")
	for _, fixture := range []struct {
		name, failure, version string
		pages                  int
		body                   func(int) []byte
		incompleteStream       bool
	}{
		{"page-limit-current", "page limit", "0.1.0", 10, func(int) []byte { return currentPage }, false},
		{"page-limit-newer", "page limit", "0.1.0", 10, func(int) []byte { return newerPage }, false},
		{"terminal-tenth-page", "", "0.1.1", 10, func(page int) []byte {
			if page == 10 {
				return encode([]map[string]any{installerReleaseFixture("v0.1.1")})
			}
			return currentPage
		}, false},
		{"exact-byte-limit", "", "0.1.0", 1, func(int) []byte { return sizedPage(maxBytes) }, false},
		{"oversized-stream", "metadata exceeds", "0.1.0", 1, func(int) []byte { return sizedPage(maxBytes + 256*1024) }, true},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			for page := 1; page <= 11; page++ {
				body := []byte("[]")
				if page <= fixture.pages {
					body = fixture.body(page)
				}
				writeInstallerFixture(t, filepath.Join(directory, fmt.Sprintf("releases-%d.json", page)), body)
			}
			home := filepath.Join(t.TempDir(), "home")
			env := append(slices.Clone(environment), "HOME="+home)
			install := exec.CommandContext(t.Context(), "/bin/bash", filepath.Join(stage, "host", "install-host.sh"))
			install.Env = env
			if output, err := install.CombinedOutput(); err != nil {
				t.Fatalf("initial installation: %v\n%s", err, output)
			}
			writeInstallerFixture(t, filepath.Join(directory, "mark-release-complete"), nil)
			if err := os.Remove(filepath.Join(directory, "release-stream-complete")); err != nil && !os.IsNotExist(err) {
				t.Fatal(err)
			}
			if err := os.Remove(filepath.Join(directory, "downloads")); err != nil {
				t.Fatal(err)
			}
			services := string(readInstallerFixture(t, filepath.Join(directory, "services")))
			command := filepath.Join(home, ".local", "bin", "sailry")
			update := exec.CommandContext(t.Context(), command, "update")
			update.Env = env
			output, err := update.CombinedOutput()
			if fixture.failure != "" {
				if err == nil || !strings.Contains(string(output), fixture.failure) ||
					strings.Contains(string(output), "already up to date") || strings.Contains(string(output), "Host updated;") {
					t.Fatalf("incomplete discovery did not fail honestly: %v\n%s", err, output)
				}
			} else if err != nil {
				t.Fatalf("bounded discovery: %v\n%s", err, output)
			}
			var downloads strings.Builder
			for page := 1; page <= fixture.pages; page++ {
				fmt.Fprintf(&downloads, "https://api.github.com/repos/sailry/sailry-code/releases?per_page=100&page=%d\n", page)
			}
			if fixture.version != "0.1.0" {
				fmt.Fprintf(&downloads, "%s/releases/download/v%s/sailry-host-x86_64-unknown-linux-gnu.tar.gz\n", repository, fixture.version)
				fmt.Fprintf(&downloads, "%s/releases/download/v%s/SHA256SUMS-x86_64-unknown-linux-gnu\n", repository, fixture.version)
			}
			if got := string(readInstallerFixture(t, filepath.Join(directory, "downloads"))); got != downloads.String() {
				t.Fatalf("discovery crossed its boundary: %q, want %q", got, downloads.String())
			}
			version := exec.CommandContext(t.Context(), command, "version")
			version.Env = env
			if output, err := version.CombinedOutput(); err != nil || string(output) != "Sailry Host "+fixture.version+"\n" {
				t.Fatalf("installed version = %q, %v", output, err)
			}
			if fixture.version == "0.1.0" && string(readInstallerFixture(t, filepath.Join(directory, "services"))) != services {
				t.Fatal("incomplete or same-version discovery changed the service")
			}
			if fixture.incompleteStream {
				if _, err := os.Stat(filepath.Join(directory, "release-stream-complete")); !os.IsNotExist(err) {
					t.Fatalf("oversized stream was fully consumed: %v", err)
				}
			}
		})
	}
}

func TestPublishedInstallerBootstrapDoesNotUseSystemPython(t *testing.T) {
	const repository = "https://github.com/sailry/sailry-code"
	directory, stage, environment := hostArchiveFixture(t, repository, "0.1.0")
	publishInstallerArchive(t, directory, repository, "0.1.0", stage)
	tools := filepath.Join(directory, "bin")
	for _, name := range []string{"bash", "cp", "tar", "mkdir", "dirname", "mktemp", "awk", "find", "chmod", "rm", "mv", "ln", "cat", "basename", "readlink", "rmdir"} {
		path, err := exec.LookPath(name)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.Symlink(path, filepath.Join(tools, name)); err != nil {
			t.Fatal(err)
		}
	}
	checksum := "sha256sum"
	if _, err := exec.LookPath(checksum); err != nil {
		checksum = "shasum"
	}
	path, err := exec.LookPath(checksum)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(path, filepath.Join(tools, checksum)); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"python", "python2", "python3", "python3.12", "jq"} {
		writeInstallerFixture(t, filepath.Join(tools, name), []byte("#!/bin/sh\n: > \"$SAILRY_INSTALL_FIXTURE/system-dependency-called\"\nexit 99\n"))
	}
	home := filepath.Join(directory, "home")
	install := exec.CommandContext(t.Context(), "/bin/bash", filepath.Join(stage, "host", "install-host.sh"))
	install.Env = append(environment, "HOME="+home, "PATH="+tools)
	if output, err := install.CombinedOutput(); err != nil {
		t.Fatalf("bootstrap without system Python: %v\n%s", err, output)
	}
	if _, err := os.Stat(filepath.Join(directory, "system-dependency-called")); !os.IsNotExist(err) {
		t.Fatalf("installer used a system interpreter or jq: %v", err)
	}
	paths := strings.Split(strings.TrimSpace(string(readInstallerFixture(t, filepath.Join(directory, "bundled-python-called")))), "\n")
	if len(paths) != 1 || !strings.HasPrefix(paths[0], filepath.Join(home, ".local", "lib", ".sailry-install-")) ||
		!strings.HasSuffix(paths[0], "/package/python/bin/python3.12") {
		t.Fatalf("bootstrap did not use only the extracted package runtime: %q", paths)
	}
}

func installerReleaseFixture(tag string) map[string]any {
	return map[string]any{"tag_name": tag, "draft": false, "prerelease": false, "assets": []map[string]string{
		{"name": "sailry-host-x86_64-unknown-linux-gnu.tar.gz", "state": "uploaded"},
		{"name": "SHA256SUMS-x86_64-unknown-linux-gnu", "state": "uploaded"},
	}}
}
