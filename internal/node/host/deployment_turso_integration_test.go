//go:build integration && !windows

package host

import (
	"bytes"
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
	"github.com/sailry/sailry-harness/internal/shipped"
)

func TestLocalDeploymentNativeRuntimePublication(t *testing.T) {
	for _, test := range []struct {
		name, corrupt string
		serviceFails  bool
		fresh         bool
		cleanupFails  bool
		overlap       bool
	}{
		{name: "publish all runtime files"},
		{name: "service failure restores old runtime", serviceFails: true},
		{name: "damaged Turso is refused", corrupt: "libturso_sync_sdk_kit.so"},
		{name: "damaged seal is refused", corrupt: shipped.SealName},
		{name: "publish new runtime", fresh: true},
		{name: "concurrent publication leaves the first installation intact", fresh: true, overlap: true},
		{name: "new service failure removes owned files", fresh: true, serviceFails: true},
		{name: "new service cleanup failure retains recovery files", fresh: true, serviceFails: true, cleanupFails: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			directory := t.TempDir()
			stage, root, profile := filepath.Join(directory, "stage"), filepath.Join(directory, "current"), filepath.Join(directory, "profile")
			paths := DeploymentOptions{ProgramRoot: root, Profile: profile, ServicePath: filepath.Join(directory, "service"), CommandPath: filepath.Join(directory, "command")}
			write := func(name string, body []byte, mode os.FileMode) {
				t.Helper()
				if err := os.MkdirAll(filepath.Dir(name), 0o700); err != nil {
					t.Fatal(err)
				}
				if err := os.WriteFile(name, body, mode); err != nil {
					t.Fatal(err)
				}
			}
			current := map[string][]byte{
				"sailry-host":              []byte("old Host"),
				"libsailry_iroh.so":        []byte("old transport"),
				"libturso_sync_sdk_kit.so": []byte("old engine"),
				shipped.SealName:           []byte("old seal"),
				"managed-go-host":          []byte("1\n"),
			}
			if !test.fresh {
				for name, body := range current {
					write(filepath.Join(root, name), body, 0o700)
				}
				write(paths.ServicePath, []byte("fixture service"), 0o600)
				if err := os.Symlink(filepath.Join(root, "sailry-host"), paths.CommandPath); err != nil {
					t.Fatal(err)
				}
			}
			// Deployment owns program files only. Existing canonical storage and
			// its journal must survive both publication and service rollback.
			stored := map[string][]byte{
				"storage/node.sqlite":     []byte("existing canonical database"),
				"storage/node.sqlite-wal": []byte("existing canonical journal"),
				"identity":                []byte("existing identity"),
			}
			if !test.fresh {
				for name, body := range stored {
					write(filepath.Join(profile, name), body, 0o600)
				}
			}
			next := map[string][]byte{
				"sailry-host":              []byte("#!/bin/sh\nprintf '%s\\n' 'Sailry Host " + link.ProductVersion + "'\n"),
				"libsailry_iroh.so":        []byte("new transport"),
				"libturso_sync_sdk_kit.so": []byte("new engine"),
			}
			for name, body := range next {
				write(filepath.Join(stage, name), body, 0o700)
			}
			seal := shipped.Seal{Version: 1, Target: "linux/amd64"}
			for _, name := range []string{"libsailry_iroh.so", "libturso_sync_sdk_kit.so"} {
				entry, err := shipped.Record(stage, name)
				if err != nil {
					t.Fatal(err)
				}
				seal.Files = append(seal.Files, entry)
			}
			writeHostSeal(t, stage, seal)
			var err error
			next[shipped.SealName], err = os.ReadFile(filepath.Join(stage, shipped.SealName))
			if err != nil {
				t.Fatal(err)
			}
			service := "true\n"
			if test.serviceFails {
				service = "false\n"
			}
			if test.fresh {
				body, err := os.ReadFile("deployment/service.sh")
				if err != nil {
					t.Fatal(err)
				}
				service = string(body)
			}
			write(filepath.Join(stage, "service.sh"), []byte(service), 0o700)
			artifacts := map[string]*artifact{}
			for name, body := range next {
				digest := sha256.Sum256(body)
				artifacts[name] = &artifact{name: name, digest: hex.EncodeToString(digest[:])}
			}
			script, err := deploymentScript(stage, artifacts["sailry-host"], artifacts["libsailry_iroh.so"], artifacts["libturso_sync_sdk_kit.so"], artifacts[shipped.SealName], paths, !test.fresh, nil)
			if err != nil {
				t.Fatal(err)
			}
			if test.corrupt != "" {
				write(filepath.Join(stage, test.corrupt), []byte("damaged"), 0o600)
			}
			// Keep all service operations inside the fixture on every POSIX host.
			tools := filepath.Join(directory, "bin")
			for name, body := range map[string]string{
				"uname": "#!/bin/sh\nprintf 'Linux\\n'\n",
				"id":    "#!/bin/sh\nprintf '0\\n'\n",
				"systemctl": `#!/bin/sh
case "$*" in
  *"disable --now"*) test "$SAILRY_FIXTURE_CLEANUP_FAILS" != true;;
  *"is-active"*) test "$SAILRY_FIXTURE_SERVICE_FAILS" != true;;
  *) exit 0;;
esac
`,
			} {
				write(filepath.Join(tools, name), []byte(body), 0o700)
			}
			if _, err := exec.LookPath("sha256sum"); err != nil {
				write(filepath.Join(tools, "sha256sum"), []byte("#!/bin/sh\nexec shasum -a 256 \"$@\"\n"), 0o700)
			}
			command := func() *exec.Cmd {
				value := exec.CommandContext(t.Context(), "/bin/sh", "-s")
				value.Dir = directory
				value.Env = []string{
					"PATH=" + tools + string(os.PathListSeparator) + os.Getenv("PATH"),
					fmt.Sprintf("SAILRY_FIXTURE_SERVICE_FAILS=%t", test.serviceFails),
					fmt.Sprintf("SAILRY_FIXTURE_CLEANUP_FAILS=%t", test.cleanupFails),
				}
				value.Stdin = strings.NewReader(script)
				return value
			}
			var output []byte
			if test.overlap {
				assertHostInstallExclusive(t, directory, tools, root, command(), command())
			} else {
				output, err = command().CombinedOutput()
			}
			want := next
			if test.serviceFails || test.corrupt != "" {
				want = current
				var exit *exec.ExitError
				if !errors.As(err, &exit) || test.serviceFails && (!test.fresh || test.cleanupFails) && exit.ExitCode() != 73 {
					t.Fatalf("failed update returned %v: %s", err, output)
				}
			} else if err != nil {
				t.Fatalf("publication: %v: %s", err, output)
			}
			if test.fresh && test.serviceFails {
				for _, path := range []string{root, profile, paths.ServicePath, paths.CommandPath} {
					_, err := os.Lstat(path)
					if test.cleanupFails {
						if err != nil {
							t.Fatalf("uncertain cleanup removed %s: %v", path, err)
						}
					} else if !errors.Is(err, os.ErrNotExist) {
						t.Fatalf("failed new installation left %s: %v", path, err)
					}
				}
				if test.cleanupFails {
					if _, err := os.Stat(stage); err != nil {
						t.Fatalf("uncertain cleanup removed the recovery stage: %v", err)
					}
				}
				return
			}
			for name, body := range want {
				actual, err := os.ReadFile(filepath.Join(root, name))
				if err != nil || !bytes.Equal(actual, body) {
					t.Fatalf("published runtime %s = %q, %v", name, actual, err)
				}
			}
			if !test.fresh {
				for name, body := range stored {
					actual, err := os.ReadFile(filepath.Join(profile, name))
					if err != nil || !bytes.Equal(actual, body) {
						t.Fatalf("existing profile %s changed: %q, %v", name, actual, err)
					}
				}
			}
		})
	}
}
