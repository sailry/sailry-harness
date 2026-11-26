package application

import (
	"crypto/ed25519"
	"encoding/base64"
	"io"
	"net/http"
	"net/url"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"
	"testing/synctest"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/live"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/updater"
	"github.com/sailry/sailry-harness/internal/identity"
	providers "github.com/sailry/sailry-harness/modules/providers/desktop"
)

func TestDesktopProductPreservesNativeOwnership(t *testing.T) {
	native := new(mygo.Application)
	previousName := native.Name()
	configureMyGo(native, Options{Identity: identity.OfficialDesktop()})
	if native.Name() != previousName || native.Version() != identity.OfficialVersion {
		t.Fatalf("official launch changed its native owner: %q (previously %q), version %q", native.Name(), previousName, native.Version())
	}
	configureMyGo(native, Options{NativeName: "ExampleDesktop", Identity: identity.Desktop{Version: "7.1.0"}})
	if native.Name() != "ExampleDesktop" || native.Version() != "7.1.0" {
		t.Fatalf("fork launch = %q, version %q", native.Name(), native.Version())
	}
}

func TestDesktopProductPresentation(t *testing.T) {
	product := identity.Desktop{
		Name: "ExampleDesktop", Version: "7.1.0-beta.2", Website: "https://example.test/desktop", Feedback: "https://example.test/issues",
		Release: identity.DesktopRelease{Package: identity.DesktopPackage{Name: "ExampleBundle"}},
	}
	app := shell.NewWithAssembly(preview.Load(), shell.Assemble(providers.Register))
	applyProduct(app, product, product.Name)
	profile := t.TempDir()
	backend := &desktopBackend{Path: profile, Client: new(client.Client), Preferences: preferences.Open(filepath.Join(profile, "preferences.json"))}
	updates := backend.updates(product, nil)
	t.Cleanup(updates.Service.Close)
	(&live.Runtime{App: app}).SetUpdates(updates, nil)
	app.Settings.Updater.Failure = &settings.UpdateFailure{Key: "updates_install_failed"}

	menu := app.Menu("en", app.Do)
	if menu[0].Label != "ExampleDesktop" {
		t.Fatalf("application menu = %q", menu[0].Label)
	}
	labels := func(item *mygo.MenuItem) []string {
		var result []string
		for _, child := range item.Submenu {
			result = append(result, child.Label)
		}
		return result
	}
	for _, label := range []string{"About ExampleDesktop", "Hide ExampleDesktop", "Quit ExampleDesktop"} {
		if !slices.Contains(labels(menu[0]), label) {
			t.Errorf("application menu missing %q: %v", label, labels(menu[0]))
		}
	}
	help := labels(menu[len(menu)-1])
	for _, label := range []string{"Website", "Feedback", "Welcome to ExampleDesktop"} {
		if !slices.Contains(help, label) {
			t.Errorf("help menu missing %q: %v", label, help)
		}
	}
	if app.Website != product.Website || app.Feedback != product.Feedback {
		t.Fatalf("help destinations = %q, %q", app.Website, app.Feedback)
	}
	if app.Settings.Channel != int(updater.Experience) {
		t.Fatalf("default channel = %d for %q", app.Settings.Channel, product.Version)
	}
	view := ui.NewTester(app.View, 1280, 820)
	view.SetLocale("en")
	view.Frame()
	if bounds, ok := view.Find(product.Name); !ok || bounds.Y >= shell.HeaderHeight {
		t.Fatalf("window header does not show the product name %q", product.Name)
	}
	about := menu[0].Submenu[0]
	about.Click(about, nil)
	view.Frame()
	view.Advance(time.Second)
	view.Frame()
	for _, text := range []string{"ExampleDesktop", product.Version, "The update did not complete; restart ExampleDesktop"} {
		if !view.HasText(text) {
			t.Errorf("About missing %q: %q", text, view.Texts())
		}
	}

	for _, item := range menu[len(menu)-1].Submenu {
		if item.ID == "help.website" || item.ID == "help.feedback" {
			item.Click(item, nil)
			view.Frame()
		}
	}
	if opened := view.OpenedURLs(); !slices.Equal(opened, []string{product.Website, product.Feedback}) {
		t.Errorf("help menu opened %v", opened)
	}

	product.Website, product.Feedback = "", ""
	applyProduct(app, product, "ExampleDesktop")
	menu = app.Menu("en", app.Do)
	help = labels(menu[len(menu)-1])
	if slices.Contains(help, "Website") || slices.Contains(help, "Feedback") {
		t.Errorf("unconfigured help destinations were offered: %v", help)
	}
}

func TestDesktopProductDefaultWelcomeMark(t *testing.T) {
	for _, tc := range []struct {
		name    string
		product identity.Desktop
		want    string
	}{
		{name: "official harness", product: identity.OfficialDesktop(), want: "SAILRY HARNESS"},
		{name: "harness id fallback", product: identity.Desktop{ID: "harness"}, want: "SAILRY HARNESS"},
		{name: "code id fallback", product: identity.Desktop{ID: "code"}, want: "SAILRY CODE"},
		{name: "explicit override", product: identity.Desktop{ID: "code", WelcomeMark: "CUSTOM CODE"}, want: "CUSTOM CODE"},
		{name: "cleaned override", product: identity.Desktop{ID: "custom", WelcomeMark: "Caf\u00e9!"}, want: "Caf!"},
		{name: "unknown fork keeps DefaultWordmark fallback", product: identity.Desktop{ID: "custom"}, want: ""},
	} {
		t.Run(tc.name, func(t *testing.T) {
			app := shell.NewWithAssembly(preview.Load(), shell.Assemble(providers.Register))
			applyProduct(app, tc.product, "Sailry")
			if app.Messages.DefaultMark != tc.want {
				t.Fatalf("DefaultMark = %q, want %q", app.Messages.DefaultMark, tc.want)
			}
			if app.Messages.Mark != "" {
				t.Fatalf("applyProduct mutated user Mark = %q", app.Messages.Mark)
			}
		})
	}
}

type releaseTransport func(*http.Request) (*http.Response, error)

func (f releaseTransport) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }

func TestDesktopUpdaterUsesApplicationRelease(t *testing.T) {
	const feed = "https://example.test/desktop/releases"
	public := ed25519.NewKeyFromSeed(make([]byte, ed25519.SeedSize)).Public().(ed25519.PublicKey)
	fork := identity.DesktopRelease{
		Package: identity.DesktopPackage{Name: "ExampleDesktop", ID: "test.example.desktop", Executable: "example-desktop"},
		Source:  feed, Downloads: "https://example.test/desktop/download", PublicKeys: base64.StdEncoding.EncodeToString(public),
	}
	previous := http.DefaultTransport
	var requests []url.URL
	http.DefaultTransport = releaseTransport(func(r *http.Request) (*http.Response, error) {
		requests = append(requests, *r.URL)
		return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader("[]"))}, nil
	})
	t.Cleanup(func() { http.DefaultTransport = previous })
	for _, tc := range []struct {
		name    string
		release identity.DesktopRelease
	}{
		{name: "no release"},
		{name: "no feed", release: identity.DesktopRelease{Package: fork.Package}},
		{name: "fork release", release: fork},
	} {
		t.Run(tc.name, func(t *testing.T) {
			requests = nil
			profile := t.TempDir()
			backend := &desktopBackend{Path: profile, Client: new(client.Client), Preferences: preferences.Open(filepath.Join(profile, "preferences.json"))}
			synctest.Test(t, func(t *testing.T) {
				updates := backend.updates(identity.Desktop{Version: "7.1.0", Release: tc.release}, nil)
				defer updates.Service.Close()
				updates.Service.Check()
				synctest.Wait()
				state, failure := updates.Service.Snapshot()
				switch {
				case tc.release.Source == "":
					if failure == nil || failure.Key != "updates_source_unready" || len(requests) != 0 {
						t.Fatalf("missing release tried another source: requests %v, failure %v", requests, failure)
					}
				case runtime.GOOS == "darwin" || runtime.GOOS == "windows" && runtime.GOARCH == "amd64":
					if failure != nil || state.Kind != updater.Idle || len(requests) != 1 || requests[0].Scheme+"://"+requests[0].Host+requests[0].Path != feed {
						t.Fatalf("fork check: requests %v, state %v, failure %v", requests, state, failure)
					}
				default:
					if failure == nil || failure.Key != "updates_platform" || len(requests) != 0 {
						t.Fatalf("unsupported system reached an update source: requests %v, failure %v", requests, failure)
					}
				}
			})
		})
	}
}
