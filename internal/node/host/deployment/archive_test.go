//go:build !windows

package deployment

import (
	"crypto/sha256"
	"encoding/json"
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
			directory, stage, environment := hostArchiveFixture(t, repository, builtVersion)
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

func TestInstalledCodeWrapperUpdate(t *testing.T) {
	const repository = "https://github.com/sailry/sailry-code"
	const triple = "x86_64-unknown-linux-gnu"
	const asset = "sailry-host-" + triple + ".tar.gz"
	directory, stage, environment := hostArchiveFixture(t, repository, "0.1.0")
	_, nextStage, _ := hostArchiveFixture(t, repository, "0.1.1")
	for version, source := range map[string]string{"0.1.0": stage, "0.1.1": nextStage} {
		publishInstallerArchive(t, directory, repository, version, source)
	}
	// Installation and updates must use the bundled runtime, not host Python or jq.
	for _, name := range []string{"python3", "jq"} {
		writeInstallerFixture(t, filepath.Join(directory, "bin", name), []byte("#!/bin/sh\necho 'Unexpected system dependency' >&2\nexit 99\n"))
	}
	home := filepath.Join(directory, "home")
	environment = append(environment, "HOME="+home)
	install := exec.CommandContext(t.Context(), "/bin/bash", filepath.Join(stage, "host", "install-host.sh"))
	install.Env = environment
	if output, err := install.CombinedOutput(); err != nil {
		t.Fatalf("initial published installation: %v\n%s", err, output)
	}
	profile := filepath.Join(home, ".sailry-host", "record")
	writeInstallerFixture(t, profile, []byte("preserved profile\n"))
	command := filepath.Join(home, ".local", "bin", "sailry")
	release := func(tag string) map[string]any {
		return map[string]any{"tag_name": tag, "draft": false, "prerelease": false, "assets": []map[string]string{
			{"name": asset, "state": "uploaded"}, {"name": "SHA256SUMS-" + triple, "state": "uploaded"},
		}}
	}
	pages := func(first, second []map[string]any) {
		t.Helper()
		for i, entries := range [][]map[string]any{first, second} {
			body, err := json.Marshal(entries)
			if err != nil {
				t.Fatal(err)
			}
			writeInstallerFixture(t, filepath.Join(directory, fmt.Sprintf("releases-%d.json", i+1)), body)
		}
	}
	first := []map[string]any{release("mobile-v99.0.0"), release("v0.1.0")}
	draft, prerelease, missingAsset := release("v9.0.0"), release("v8.0.0"), release("v7.0.0")
	draft["draft"], prerelease["prerelease"], missingAsset["assets"] = true, true, nil
	first = append(first, draft, prerelease, missingAsset, release("v6.0.0-rc.1"))
	for len(first) < 100 {
		first = append(first, release(fmt.Sprintf("mobile-v1.0.%d", len(first))))
	}
	pages(first, []map[string]any{release("v0.1.1")})
	api := "https://api.github.com/repos/sailry/sailry-code/releases?per_page=100&page="
	assertVersion := func(want string) {
		t.Helper()
		version := exec.CommandContext(t.Context(), command, "version")
		version.Env = environment
		if output, err := version.CombinedOutput(); err != nil || string(output) != "Sailry Host "+want+"\n" {
			t.Fatalf("installed version = %q, %v, want %s", output, err, want)
		}
	}
	runUpdate := func() (string, error) {
		t.Helper()
		if err := os.Remove(filepath.Join(directory, "downloads")); err != nil && !os.IsNotExist(err) {
			t.Fatal(err)
		}
		update := exec.CommandContext(t.Context(), command, "update")
		update.Env = environment
		output, err := update.CombinedOutput()
		return string(output), err
	}
	assertVersion("0.1.0")
	if output, err := runUpdate(); err != nil || !strings.Contains(output, "Host updated;") {
		t.Fatalf("wrapper update: %v\n%s", err, output)
	}
	assertVersion("0.1.1")
	wantDownloads := api + "1\n" + api + "2\n" + repository + "/releases/download/v0.1.1/" + asset + "\n" +
		repository + "/releases/download/v0.1.1/SHA256SUMS-" + triple + "\n"
	if downloads := string(readInstallerFixture(t, filepath.Join(directory, "downloads"))); downloads != wantDownloads {
		t.Fatalf("update downloads = %q, want %q", downloads, wantDownloads)
	}
	backup := filepath.Join(home, ".local", "lib", ".sailry-previous", "sailry-host")
	if body := readInstallerFixture(t, backup); string(body) != string(installerHostVersion("0.1.0")) {
		t.Fatalf("previous Host was not retained: %s", body)
	}
	services := string(readInstallerFixture(t, filepath.Join(directory, "services")))
	if output, err := runUpdate(); err != nil || !strings.Contains(output, "already up to date") || strings.Contains(output, "Host updated;") {
		t.Fatalf("same-version update: %v\n%s", err, output)
	}
	if downloads := string(readInstallerFixture(t, filepath.Join(directory, "downloads"))); downloads != api+"1\n"+api+"2\n" {
		t.Fatalf("same-version update downloaded assets: %s", downloads)
	}
	pages([]map[string]any{release("mobile-v99.0.0"), release("v0.1.0")}, nil)
	if output, err := runUpdate(); err == nil || !strings.Contains(output, "downgrade") || strings.Contains(output, "Host updated;") {
		t.Fatalf("downgrade was not refused: %v\n%s", err, output)
	}
	if downloads := string(readInstallerFixture(t, filepath.Join(directory, "downloads"))); downloads != api+"1\n" {
		t.Fatalf("refused update downloaded assets: %s", downloads)
	}
	assertVersion("0.1.1")
	if got := string(readInstallerFixture(t, filepath.Join(directory, "services"))); got != services {
		t.Fatal("same-version or refused update changed the service")
	}
	if got := string(readInstallerFixture(t, backup)); got != string(installerHostVersion("0.1.0")) {
		t.Fatal("same-version or refused update changed the backup")
	}
	if got := string(readInstallerFixture(t, profile)); got != "preserved profile\n" {
		t.Fatal("wrapper update changed existing profile data")
	}
}

func TestInstalledWrapperVersionCompatibility(t *testing.T) {
	for _, fixture := range []struct {
		name, repository string
		args             []string
	}{
		{"harness-default", "https://github.com/sailry/sailry-harness", nil},
		{"harness-explicit", "https://github.com/sailry/sailry-harness", []string{"--version", "v0.1.1"}},
		{"code-explicit", "https://github.com/sailry/sailry-code", []string{"--version", "v0.1.1"}},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			directory, stage, environment := hostArchiveFixture(t, fixture.repository, "0.1.0")
			_, nextStage, _ := hostArchiveFixture(t, fixture.repository, "0.1.1")
			publishInstallerArchive(t, directory, fixture.repository, "0.1.0", stage)
			publishInstallerArchive(t, directory, fixture.repository, "0.1.1", nextStage)
			writeInstallerFixture(t, filepath.Join(directory, "releases.json"), []byte("[\n  {\"tag_name\": \"v0.1.1\"}\n]\n"))
			home := filepath.Join(directory, "home")
			environment = append(environment, "HOME="+home)
			install := exec.CommandContext(t.Context(), "/bin/bash", filepath.Join(stage, "host", "install-host.sh"), "--version", "0.1.0")
			install.Env = environment
			if output, err := install.CombinedOutput(); err != nil {
				t.Fatalf("initial installation: %v\n%s", err, output)
			}
			if err := os.Remove(filepath.Join(directory, "downloads")); err != nil {
				t.Fatal(err)
			}
			command := filepath.Join(home, ".local", "bin", "sailry")
			update := exec.CommandContext(t.Context(), command, append([]string{"update"}, fixture.args...)...)
			update.Env = environment
			if output, err := update.CombinedOutput(); err != nil || !strings.Contains(string(output), "Host updated;") {
				t.Fatalf("wrapper update: %v\n%s", err, output)
			}
			version := exec.CommandContext(t.Context(), command, "version")
			version.Env = environment
			if output, err := version.CombinedOutput(); err != nil || string(output) != "Sailry Host 0.1.1\n" {
				t.Fatalf("installed version: %q, %v", output, err)
			}
			wantDownloads := fixture.repository + "/releases/download/v0.1.1/sailry-host-x86_64-unknown-linux-gnu.tar.gz\n" +
				fixture.repository + "/releases/download/v0.1.1/SHA256SUMS-x86_64-unknown-linux-gnu\n"
			if len(fixture.args) == 0 {
				wantDownloads = "https://api.github.com/repos/sailry/sailry-harness/releases?per_page=1\n" + wantDownloads
			}
			if downloads := string(readInstallerFixture(t, filepath.Join(directory, "downloads"))); downloads != wantDownloads {
				t.Fatalf("compatibility downloads = %q, want %q", downloads, wantDownloads)
			}
		})
	}
}

func publishInstallerArchive(t *testing.T, directory, repository, version, stage string) {
	t.Helper()
	base := filepath.Join(directory, strings.TrimPrefix(repository, "https://github.com/"), "releases", "download", "v"+version)
	for _, name := range []string{"sailry-host-x86_64-unknown-linux-gnu.tar.gz", "SHA256SUMS-x86_64-unknown-linux-gnu"} {
		writeInstallerFixture(t, filepath.Join(base, name), readInstallerFixture(t, filepath.Join(stage, name)))
	}
}

func hostArchiveFixture(t *testing.T, repository, version string) (directory, stage string, environment []string) {
	t.Helper()
	directory = t.TempDir()
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
	writeInstallerFixture(t, filepath.Join(directory, "built-host"), installerHostVersion(version))
	writeInstallerFixture(t, filepath.Join(directory, "runtime-python"), []byte("#!/bin/sh\nprintf '%s\\n' \"$0\" >> \"$SAILRY_INSTALL_FIXTURE/bundled-python-called\"\nexec \"$SAILRY_INSTALL_FIXTURE_PYTHON\" \"$@\"\n"))
	writeInstallerFixture(t, filepath.Join(directory, "rust-licenses", "dependencies.json"), []byte("[]\n"))
	product := filepath.Join(directory, "product")
	writeInstallerFixture(t, filepath.Join(product, "go.mod"), []byte("module example.com/host-fixture\n"))
	writeInstallerFixture(t, filepath.Join(product, "go.sum"), nil)
	if err := os.MkdirAll(filepath.Join(product, "cmd", "sailry-host"), 0o700); err != nil {
		t.Fatal(err)
	}
	environment = append(os.Environ(),
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
	stage = filepath.Join(product, ".runtime", "host", "x86_64-unknown-linux-gnu")
	return directory, stage, environment
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
