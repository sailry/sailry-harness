//go:build integration

package live

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"
	"time"

	"github.com/egoist/mygo/plugins/permissionflow"
	"github.com/egoist/mygo/ui"
	hostassembly "github.com/sailry/sailry-harness/internal/assembly/host"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
	computer "github.com/sailry/sailry-harness/modules/computer/desktop"
	"github.com/sailry/sailry-harness/sdk/desktop/native"
)

func TestComputerPermissionsAssemblyLive(t *testing.T) {
	if runtime.GOOS != "darwin" {
		t.Skip("the helper's system grants are used on macOS")
	}
	ctx, cancel := context.WithTimeout(t.Context(), 45*time.Second)
	defer cancel()
	dir := t.TempDir()
	helpers := filepath.Join(dir, "helpers")
	helper := filepath.Join(helpers, "Sailry Computer Use.app", "Contents", "MacOS", "agent-desktop")
	if err := os.MkdirAll(filepath.Dir(helper), 0700); err != nil {
		t.Fatal(err)
	}
	if output, err := exec.CommandContext(ctx, "go", "build", "-o", helper, "github.com/sailry/sailry-harness/internal/node/testdata/computerhelper").CombinedOutput(); err != nil {
		t.Fatalf("build the fixture helper: %v\n%s", err, output)
	}
	if err := os.WriteFile(filepath.Join(filepath.Dir(helper), "state.json"), []byte(`{"accessibility":"granted","screen_recording":"denied"}`), 0600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("SAILRY_HELPERS_DIR", helpers)
	owner, err := node.StartWithOptions(ctx, filepath.Join(dir, "profile"), node.Options{Assembly: hostassembly.Harness()})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = owner.Stop(context.Background()) })
	transport := func() link.Transport { return link.NewLocal(owner.ID(), owner.ID(), owner) }
	setup := client.New(transport())
	t.Cleanup(func() { _ = setup.Close() })
	if err := setup.Start(ctx); err != nil {
		t.Fatal(err)
	}
	out, err := setup.Do(ctx, protocol.Command{Kind: "read_computer_settings"})
	value, ok := detail[protocol.ComputerSettings](out.Data)
	if err != nil || !ok {
		t.Fatalf("read fixture settings: %v %v", out, err)
	}
	if _, err := setup.Do(ctx, protocol.Command{Kind: "save_computer_settings", Data: protocol.ComputerSettings{Revision: value.Revision, Enabled: true}}); err != nil {
		t.Fatal(err)
	}
	for _, test := range []struct {
		name     string
		assembly native.Assembly
		wantRows int
	}{
		{"absent", native.Assemble(), 0},
		{"installed", native.Assemble(computer.Register), 2},
	} {
		t.Run(test.name, func(t *testing.T) {
			trace := &permissionCommandTrace{Transport: transport()}
			controller := client.New(trace)
			defer controller.Close()
			if err := controller.Start(ctx); err != nil {
				t.Fatal(err)
			}
			r, err := NewWithAssembly(ctx, controller, preferences.Open(filepath.Join(t.TempDir(), "preferences.json")), test.assembly)
			if err != nil {
				t.Fatal(err)
			}
			defer r.Close()
			// The window's own grants are injected; the real Node executes only
			// the disposable computer helper above, never an OS grant prompt.
			r.permissions.probe = permissionProbe{goos: "darwin", check: func(permissionflow.Permission) (permissionflow.Status, error) {
				return permissionflow.Granted, nil
			}}
			r.App.Navigate(shell.SettingsPage)
			r.App.Settings.Open(settings.Permissions)
			window := ui.NewTester(r.App.View, 900, 650)
			wait := func(ready func() bool) {
				t.Helper()
				deadline := time.After(8 * time.Second)
				ticker := time.NewTicker(5 * time.Millisecond)
				defer ticker.Stop()
				for {
					window.Frame()
					if ready() {
						return
					}
					select {
					case <-deadline:
						t.Fatalf("Permissions did not settle: %+v, commands %v", r.App.Settings.PermissionActions, trace.sent(""))
					case <-ticker.C:
					}
				}
			}
			wait(func() bool {
				actions := r.App.Settings.PermissionActions
				return actions != nil && !r.permissions.checking && len(actions.HostPermissions) == test.wantRows
			})
			actions := r.App.Settings.PermissionActions
			if test.wantRows == 0 {
				actions.Check()
				r.workers.Wait()
				window.Frame()
				if len(actions.HostPermissions) != 0 || len(trace.sent("")) != 0 {
					t.Fatalf("an enabled Node was probed without a Desktop contribution: rows %v, commands %v", actions.HostPermissions, trace.sent(""))
				}
				return
			}
			if rows := actions.HostPermissions; rows[0].Status != "permission_granted" || rows[1].Status != "permission_required" {
				t.Fatalf("grants from the helper: %v", rows)
			}
			actions.HostRequest("permission_screen")
			wait(func() bool {
				return !r.permissions.pending && !r.permissions.checking && len(actions.HostPermissions) == 2 && actions.HostPermissions[1].Status == "permission_granted"
			})
			if sent := trace.sent("request_computer_permission"); len(sent) != 1 {
				t.Fatalf("expected one grant request through the real Node: %v", sent)
			}
		})
	}
}
