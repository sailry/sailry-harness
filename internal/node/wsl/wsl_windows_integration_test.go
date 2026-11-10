//go:build integration && windows

package wsl

import (
	"context"
	"errors"
	"os"
	"slices"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/sys/windows"
	"golang.org/x/sys/windows/registry"
)

// TestWSLWindowsIntegration runs on a Windows machine only:
//
//	go test -tags integration -run TestWSLWindowsIntegration ./internal/node/wsl/
//
// The registry subtest writes only below a fixed fixture key under
// HKCU\Software and removes it. SAILRY_WSL_DISTRIBUTION names a WSL2
// distribution for the read-only preflight. SAILRY_WSL_INSTALL_DISTRIBUTION
// names a disposable WSL2 distribution that receives a real Host service.
func TestWSLWindowsIntegration(t *testing.T) {
	t.Run("reads registrations", func(t *testing.T) {
		const fixture = `Software\SailryWslIntegrationFixture`
		var created []string
		create := func(suffix string) registry.Key {
			t.Helper()
			path := fixture
			if suffix != "" {
				path += `\` + suffix
			}
			key, _, err := registry.CreateKey(registry.CURRENT_USER, path, registry.ALL_ACCESS)
			if err != nil {
				t.Fatal(err)
			}
			created = append(created, path)
			return key
		}
		t.Cleanup(func() {
			for _, path := range slices.Backward(created) {
				_ = registry.DeleteKey(registry.CURRENT_USER, path)
			}
		})
		open := func(suffix string) (registry.Key, error) {
			return registry.OpenKey(registry.CURRENT_USER, fixture+`\`+suffix, registry.READ)
		}
		expect := func(suffix string, want bool) {
			t.Helper()
			if found, err := registered(open(suffix)); err != nil || found != want {
				t.Fatalf("%s: registered %v, error %v", suffix, found, err)
			}
		}
		create("").Close()
		expect("missing", false)
		create("empty").Close()
		expect("empty", false)
		create("metadata").Close()
		create(`metadata\AppxInstallerCache`).Close()
		create(`metadata\{not-a-guid}`).Close()
		expect("metadata", false)
		create("corrupt").Close()
		create(`corrupt\{16317611-9e46-4076-90d8-ce9e7c9b2141}`).Close()
		expect("corrupt", true)

		create("installed").Close()
		for _, entry := range []struct {
			id, name string
			version  uint32
		}{
			{"{aa6936d0-a384-4493-aa70-12b97cf6c028}", "Ubuntu 资料", 2},
			{"{16317611-9e46-4076-90d8-ce9e7c9b2141}", "Legacy", 1},
		} {
			key := create(`installed\` + entry.id)
			if err := key.SetStringValue("DistributionName", entry.name); err != nil {
				t.Fatal(err)
			}
			if err := key.SetDWordValue("Version", entry.version); err != nil {
				t.Fatal(err)
			}
			key.Close()
		}
		expect("installed", true)
		root, err := open("installed")
		if err != nil {
			t.Fatal(err)
		}
		defer root.Close()
		found, err := metadata(root, []string{"Ubuntu 资料"})
		if err != nil || !slices.Equal(found, []protocol.WslDistribution{{Name: "Ubuntu 资料", Version: 2}}) {
			t.Fatalf("listed distribution %v, error %v", found, err)
		}
		if found, err := metadata(root, []string{"Legacy"}); err != nil || found[0].Version != 1 {
			t.Fatalf("legacy distribution %v, error %v", found, err)
		}
		if _, err := metadata(root, []string{"Removed"}); faultCode(err) != protocol.ErrorCodeUnavailable {
			t.Fatalf("removed distribution: %v", err)
		}
		key, err := registry.OpenKey(registry.CURRENT_USER, fixture+`\installed\{aa6936d0-a384-4493-aa70-12b97cf6c028}`, registry.SET_VALUE)
		if err != nil {
			t.Fatal(err)
		}
		err = key.SetStringValue("Version", "not-a-number")
		key.Close()
		if err != nil {
			t.Fatal(err)
		}
		if _, err := metadata(root, []string{"Ubuntu 资料"}); faultCode(err) != protocol.ErrorCodeUnavailable {
			t.Fatalf("mistyped version: %v", err)
		}

		for _, code := range []error{windows.ERROR_FILE_NOT_FOUND, windows.ERROR_PATH_NOT_FOUND} {
			if found, err := registered(0, code); err != nil || found {
				t.Fatalf("%v: registered %v, error %v", code, found, err)
			}
		}
		var fault *protocol.Fault
		if _, err := registered(0, windows.ERROR_ACCESS_DENIED); !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeUnavailable || fault.Message != "WSL discovery registration read failed (HRESULT 0x80070005)" {
			t.Fatalf("access denied: %v", err)
		}
	})
	t.Run("discovers installed distributions", func(t *testing.T) {
		distributions, err := NewService().List(context.Background(), os.Environ())
		if err != nil {
			t.Fatal(err)
		}
		for _, entry := range distributions {
			t.Logf("distribution %q uses WSL%d", entry.Name, entry.Version)
			if err := validate(entry.Name); err != nil || entry.Version != 1 && entry.Version != 2 {
				t.Errorf("unexpected distribution %+v: %v", entry, err)
			}
		}
		if expected := os.Getenv("SAILRY_WSL_DISTRIBUTION"); expected != "" && deployable(distributions, expected) != nil {
			t.Errorf("%q is not a discovered WSL2 distribution", expected)
		}
	})
	t.Run("runs the preflight", func(t *testing.T) {
		distribution := os.Getenv("SAILRY_WSL_DISTRIBUTION")
		if distribution == "" {
			t.Skip("SAILRY_WSL_DISTRIBUTION is not set")
		}
		found, err := discover(context.Background(), os.Environ())
		if err != nil {
			t.Fatal(err)
		}
		_, err = run(context.Background(), found.executable, launchArguments(distribution), os.Environ(), normalize(preflightSource), 30*time.Second)
		// A refusal names the missing requirement; only an unknown outcome
		// means the launcher itself misbehaved.
		t.Logf("preflight result: %v", err)
		if err != nil && faultCode(err) != protocol.ErrorCodeUnavailable {
			t.Fatal(err)
		}
	})
	t.Run("installs and pairs a Host", func(t *testing.T) {
		distribution := os.Getenv("SAILRY_WSL_INSTALL_DISTRIBUTION")
		if distribution == "" {
			t.Skip("SAILRY_WSL_INSTALL_DISTRIBUTION is not set")
		}
		ctx := context.Background()
		controller, err := link.OpenController(ctx, t.TempDir(), link.NetworkScope{Internet: true})
		if err != nil {
			t.Fatal(err)
		}
		defer controller.Close()
		address, err := controller.Address(ctx)
		if err != nil {
			t.Fatal(err)
		}
		service := NewService()
		request := protocol.NewRequest(address.ID, protocol.Command{Kind: "install_wsl_host", Data: &protocol.CommandInstallWslHostPayload{Distribution: distribution}})
		installation, err := service.Begin(ctx, address.ID, request)
		if err != nil {
			t.Fatal(err)
		}
		defer installation.Close()
		outcome, err := installation.Execute(os.Environ(), controller, address.ID, nil, nil)
		if err != nil {
			t.Fatal(err)
		}
		installed, ok := outcome.Data.(protocol.HostInstallationOutcomeInstalledPayload)
		if outcome.Kind != "installed" || !ok || installed.Node == address.ID {
			t.Fatalf("outcome %+v", outcome)
		}
	})
}
