//go:build integration

package application

import (
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	hostassembly "github.com/sailry/sailry-harness/internal/assembly/host"
	"github.com/sailry/sailry-harness/internal/desktop/live"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/identity"
)

func TestHarnessDesktopDoesNotOfferUpdates(t *testing.T) {
	product := identity.DefaultDesktop()
	backend, err := openBackend(t.Context(), startupOptions{DataDir: filepath.Join(t.TempDir(), "profile")}, product, hostassembly.Harness())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := backend.Close(); err != nil {
			t.Error(err)
		}
	})
	updates := backend.updates(product, nil)
	if updates == nil || updates.Service != nil {
		t.Fatal("default Harness created a desktop update service or lost its About binding")
	}
	runtime, err := live.NewWithAssembly(t.Context(), backend.Client, backend.Preferences, desktop.Harness())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := runtime.Close(); err != nil {
			t.Error(err)
		}
	})
	app := runtime.App
	applyProduct(app, product, product.Name)
	runtime.SetUpdates(updates, nil)
	app.Settings.Open(settings.About)
	app.Navigate(shell.SettingsPage)
	view := ui.NewTester(app.View, 1280, 820)
	view.SetLocale("en")
	view.Frame()
	view.Advance(time.Second)
	view.Frame()
	if !view.HasText(product.Version) || !view.HasText("Apache-2.0") || view.HasText("Check for updates") || view.HasText("Update channel") || app.Settings.UpdateActions != nil {
		t.Fatalf("unexpected Harness About controls: %q", view.Texts())
	}
}
