//go:build !windows

package deployment

import (
	"crypto/sha256"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

func TestPublishedInstallerDefaultVersion(t *testing.T) {
	const builtVersion = "4.5.6-rc.1"
	const selectedVersion = "7.8.9"
	const triple = "x86_64-unknown-linux-gnu"
	for _, repository := range []string{"", "https://github.com/sailry/sailry-harness", "https://github.com/sailry/sailry-code"} {
		name := filepath.Base(repository)
		if repository == "" {
			name = "default-harness"
		}
		t.Run(name, func(t *testing.T) {
			directory := t.TempDir()
			tools := filepath.Join(directory, "bin")
			python, err := exec.LookPath("python3")
			if err != nil {
				t.Fatal(err)
			}
			tool := readInstallerFixture(t, "testdata/archive-tool.sh")
			for _, name := range []string{"go", "git", "jq", "uname", "id", "curl", "systemctl", "loginctl"} {
				writeInstallerFixture(t, filepath.Join(tools, name), tool)
			}
			if _, err := exec.LookPath("sha256sum"); err != nil {
				writeInstallerFixture(t, filepath.Join(tools, "sha256sum"), []byte("#!/bin/sh\nexec shasum -a 256 \"$@\"\n"))
			}
			writeInstallerFixture(t, filepath.Join(directory, "built-host"), installerHostVersion(builtVersion))
			writeInstallerFixture(t, filepath.Join(directory, "runtime-python"), []byte("#!/bin/sh\nexec \"$SAILRY_INSTALL_FIXTURE_PYTHON\" \"$@\"\n"))
			writeInstallerFixture(t, filepath.Join(directory, "rust-licenses", "dependencies.json"), []byte("[]\n"))
			product := filepath.Join(directory, "product")
			writeInstallerFixture(t, filepath.Join(product, "go.mod"), []byte("module example.com/host-fixture\n"))
			writeInstallerFixture(t, filepath.Join(product, "go.sum"), nil)
			if err := os.MkdirAll(filepath.Join(product, "cmd", "sailry-host"), 0o700); err != nil {
				t.Fatal(err)
			}
			environment := append(os.Environ(),
				"PATH="+tools+string(os.PathListSeparator)+os.Getenv("PATH"),
				"HOME="+filepath.Join(directory, "build-home"),
				"TMPDIR="+directory,
				"SAILRY_INSTALL_FIXTURE="+directory,
				"SAILRY_INSTALL_FIXTURE_PYTHON="+python,
				"SAILRY_RELEASE_REPOSITORY="+repository,
				"SAILRY_RUST_LICENSES_CACHE="+filepath.Join(directory, "rust-licenses"),
				"SAILRY_HOST_COMMAND=./cmd/sailry-host",
				"SAILRY_REVISION=fixture",
			)
			archiveScript, err := filepath.Abs("archive.sh")
			if err != nil {
				t.Fatal(err)
			}
			archive := exec.CommandContext(t.Context(), "/bin/bash", archiveScript)
			archive.Dir, archive.Env = product, environment
			if output, err := archive.CombinedOutput(); err != nil {
				t.Fatalf("archive entry point: %v\n%s", err, output)
			}
			stage := filepath.Join(product, ".runtime", "host", triple)
			installer := filepath.Join(stage, "host", "install-host.sh")
			code := repository == "https://github.com/sailry/sailry-code"
			if repository == "" {
				repository = "https://github.com/sailry/sailry-harness"
			}
			releases := filepath.Join(directory, strings.TrimPrefix(repository, "https://github.com/"), "releases", "download")
			asset := "sailry-host-" + triple + ".tar.gz"
			publish := func(version string, body []byte) {
				t.Helper()
				base := filepath.Join(releases, "v"+version)
				writeInstallerFixture(t, filepath.Join(base, asset), body)
				writeInstallerFixture(t, filepath.Join(base, "SHA256SUMS-"+triple), fmt.Appendf(nil, "%x  %s\n", sha256.Sum256(body), asset))
			}
			publish(builtVersion, readInstallerFixture(t, filepath.Join(stage, asset)))
			writeInstallerFixture(t, filepath.Join(stage, "host", "sailry-host"), installerHostVersion(selectedVersion))
			other := filepath.Join(directory, "selected.tar.gz")
			pack := exec.CommandContext(t.Context(), "tar", "-czf", other, "-C", filepath.Join(stage, "host"), ".")
			if output, err := pack.CombinedOutput(); err != nil {
				t.Fatalf("fixture archive: %v\n%s", err, output)
			}
			publish(selectedVersion, readInstallerFixture(t, other))
			latest := "v" + selectedVersion
			if code {
				latest = "mobile-v99.0.0"
			}
			writeInstallerFixture(t, filepath.Join(directory, "releases.json"), fmt.Appendf(nil, "[\n  {\"tag_name\": \"%s\"}\n]\n", latest))
			for _, fixture := range []struct {
				name string
				args []string
				want string
			}{
				{"no-version", nil, map[bool]string{true: builtVersion, false: selectedVersion}[code]},
				{"explicit-version", []string{"--version", "v" + selectedVersion}, selectedVersion},
			} {
				t.Run(fixture.name, func(t *testing.T) {
					home := filepath.Join(t.TempDir(), "home")
					if err := os.Remove(filepath.Join(directory, "downloads")); err != nil && !os.IsNotExist(err) {
						t.Fatal(err)
					}
					// Execute the generated installer unchanged; all remote and service
					// boundaries are local fixtures, and HOME is isolated.
					install := exec.CommandContext(t.Context(), "/bin/bash", append([]string{installer}, fixture.args...)...)
					install.Env = append(slices.Clone(environment), "HOME="+home)
					if output, err := install.CombinedOutput(); err != nil {
						t.Fatalf("published installer: %v\n%s", err, output)
					}
					wantDownloads := repository + "/releases/download/v" + fixture.want + "/" + asset + "\n" +
						repository + "/releases/download/v" + fixture.want + "/SHA256SUMS-" + triple + "\n"
					if !code && len(fixture.args) == 0 {
						wantDownloads = "https://api.github.com/repos/sailry/sailry-harness/releases?per_page=1\n" + wantDownloads
					}
					if downloads := string(readInstallerFixture(t, filepath.Join(directory, "downloads"))); downloads != wantDownloads {
						t.Fatalf("downloads = %q, want %q", downloads, wantDownloads)
					}
					version := exec.CommandContext(t.Context(), filepath.Join(home, ".local", "bin", "sailry"), "version")
					version.Env = install.Env
					if output, err := version.CombinedOutput(); err != nil || string(output) != "Sailry Host "+fixture.want+"\n" {
						t.Fatalf("installed version = %q, %v", output, err)
					}
					unit := readInstallerFixture(t, filepath.Join(home, ".config", "systemd", "user", "sailry-host.service"))
					if !strings.Contains(string(unit), `ExecStart="%h/.local/lib/sailry/sailry-host"`) {
						t.Fatalf("installed service = %s", unit)
					}
				})
			}
		})
	}
}

func installerHostVersion(version string) []byte {
	return fmt.Appendf(nil, "#!/bin/sh\ntest \"$1\" = --version || exit 1\nprintf 'Sailry Host %s\\n'\n", version)
}

func readInstallerFixture(t *testing.T, path string) []byte {
	t.Helper()
	body, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return body
}

func writeInstallerFixture(t *testing.T, path string, body []byte) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, body, 0o700); err != nil {
		t.Fatal(err)
	}
}
