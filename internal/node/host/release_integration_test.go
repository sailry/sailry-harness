//go:build integration && !windows

package host

import (
	"crypto/sha256"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/wsl"
)

func TestOfficialHostBootstrapFixture(t *testing.T) {
	const version = "4.5.6"
	for _, fixture := range []struct {
		name, binaryVersion, corruptFile, failure string
	}{
		{"official package", version, "", ""},
		{"wrong Host binary", "8.0.0", "", ""},
		{"damaged installer", version, "install-host.sh", "Installer checksum does not match"},
		{"damaged archive", version, "archive.tar.gz", "Host checksum does not match"},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			directory, home, installer, tools := freshHostInstallerForVersion(t, fixture.binaryVersion)
			body, err := os.ReadFile(installer)
			if err != nil {
				t.Fatal(err)
			}
			checksum := fmt.Sprintf("%x  install-host.sh\n", sha256.Sum256(body))
			if err := os.WriteFile(filepath.Join(directory, "installer-checksum"), []byte(checksum), 0o600); err != nil {
				t.Fatal(err)
			}
			if fixture.corruptFile != "" {
				if err := os.WriteFile(filepath.Join(directory, fixture.corruptFile), []byte("damaged"), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			curl := `#!/bin/sh
set -eu
while test "$#" -gt 0; do
  case "$1" in
    --output) destination=$2; shift 2 ;;
    https://*) url=$1; shift ;;
    *) shift ;;
  esac
done
case "$url" in
  https://github.com/sailry/sailry-harness/releases/download/v` + version + `/install-host.sh) source=install-host.sh ;;
  https://github.com/sailry/sailry-harness/releases/download/v` + version + `/SHA256SUMS-installer) source=installer-checksum ;;
  https://github.com/sailry/sailry-harness/releases/download/v` + version + `/sailry-host-x86_64-unknown-linux-gnu.tar.gz) source=archive.tar.gz ;;
  https://github.com/sailry/sailry-harness/releases/download/v` + version + `/SHA256SUMS-x86_64-unknown-linux-gnu) source=checksums ;;
  *) exit 1 ;;
esac
cp "$SAILRY_INSTALL_FIXTURE/$source" "$destination"
printf '%s\n' "$source" >> "$SAILRY_INSTALL_FIXTURE/downloads"
`
			if err := os.WriteFile(filepath.Join(tools, "curl"), []byte(curl), 0o700); err != nil {
				t.Fatal(err)
			}
			script, err := wsl.ReleaseScript(version, false, nil)
			if err != nil {
				t.Fatal(err)
			}
			command := exec.CommandContext(t.Context(), "/bin/sh", "-s")
			command.Stdin = strings.NewReader(script)
			command.Env = append(os.Environ(),
				"PATH="+tools+string(os.PathListSeparator)+os.Getenv("PATH"),
				"SAILRY_INSTALL_FIXTURE="+directory, "SAILRY_INSTALL_FIXTURE_HOME="+home,
				"TMPDIR="+directory,
				"SAILRY_INSTALL_FAIL=", "SAILRY_INSTALL_RETAIN_DATA=false", "SAILRY_INSTALL_CLEANUP_FAILS=false")
			output, err := command.CombinedOutput()
			accept := fixture.binaryVersion == version && fixture.corruptFile == ""
			if (err == nil) != accept || !strings.Contains(string(output), fixture.failure) {
				t.Fatalf("bootstrap output = %s, error = %v", output, err)
			}
			want := "install-host.sh\ninstaller-checksum\n"
			if fixture.corruptFile != "install-host.sh" {
				want += "archive.tar.gz\nchecksums\n"
			}
			downloads, err := os.ReadFile(filepath.Join(directory, "downloads"))
			if err != nil || string(downloads) != want {
				t.Fatalf("installer and archive were not both obtained from the expected official release: %q, %v", downloads, err)
			}
			binary := filepath.Join(home, ".local", "lib", "sailry", "sailry-host")
			profile := filepath.Join(home, ".sailry-host")
			unit := filepath.Join(home, ".config", "systemd", "user", "sailry-host.service")
			if !accept {
				for _, path := range []string{binary, profile, unit} {
					if _, err := os.Stat(path); !errors.Is(err, os.ErrNotExist) {
						t.Fatalf("wrong binary published %s: %v", path, err)
					}
				}
				return
			}
			if output, err := exec.CommandContext(t.Context(), binary, "--version").Output(); err != nil || string(output) != "Sailry Host "+version+"\n" {
				t.Fatalf("installed binary = %s, %v", output, err)
			}
			if info, err := os.Stat(profile); err != nil || !info.IsDir() {
				t.Fatalf("installed profile = %v, %v", info, err)
			}
			if service, err := os.ReadFile(unit); err != nil || !strings.Contains(string(service), `ExecStart="%h/.local/lib/sailry/sailry-host" --data-dir "%h/.sailry-host"`) {
				t.Fatalf("installed service = %s, %v", service, err)
			}
		})
	}
}
