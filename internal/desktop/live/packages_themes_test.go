package live

import (
	"bytes"
	"encoding/json"
	"image"
	"image/png"
	"os"
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestThemeCatalogRowsKeepExternalThemesOptional(t *testing.T) {
	defer theme.SetInstalled(nil)
	coast := &theme.Package{ID: "coast", Name: "Coast", Release: "1.0.0", Light: &theme.Variant{}, Dark: &theme.Variant{}}
	local := &theme.Package{ID: "local", Name: "Local", Light: &theme.Variant{}, Dark: &theme.Variant{}}
	theme.SetInstalled([]*theme.Package{coast, local})
	r := &Runtime{App: &shell.App{Theme: coast.ID}, Preferences: &preferences.Store{}, packages: newPackageState()}
	r.packages.themeCatalog = theme.CatalogListing{Version: 1, Themes: []theme.CatalogEntry{
		{ID: coast.ID, Name: coast.Name, Version: "1.1.0", Modes: []string{"light", "dark"}},
		{ID: "tide", Name: "Tide", Version: "1.0.0", Modes: []string{"light", "dark"}},
	}}
	rows := r.themeCatalogRows()
	if len(rows) != 3 {
		t.Fatalf("catalog rows = %+v", rows)
	}
	if row := rows[0]; row.ID != coast.ID || !row.Installed || !row.Current || !row.Update || row.Builtin || !row.Light || !row.Dark {
		t.Fatalf("installed catalog theme = %+v", row)
	}
	if row := rows[1]; row.ID != "tide" || row.Installed || row.Builtin || row.Current || row.Update {
		t.Fatalf("uninstalled catalog theme = %+v", row)
	}
	if row := rows[2]; row.ID != local.ID || !row.Installed || row.Builtin || !row.Light || !row.Dark {
		t.Fatalf("installed theme outside the catalog = %+v", row)
	}
	r.packages.themeCatalog = theme.CatalogListing{Version: 1, Unavailable: true}
	if rows := r.themeCatalogRows(); len(rows) != 2 || rows[0].ID != coast.ID || !rows[0].Current || rows[1].ID != local.ID {
		t.Fatalf("unavailable catalog hid installed themes: %+v", rows)
	}
	theme.SetInstalled(nil)
	if rows := r.themeCatalogRows(); len(rows) != 0 || r.App.Theme != coast.ID {
		t.Fatalf("an unavailable theme changed the saved selection: %+v, %q", rows, r.App.Theme)
	}
}

func TestExportThemeReloadsFromDisk(t *testing.T) {
	defer theme.SetInstalled(nil)
	profile := theme.NewProfile(t.TempDir())
	themeDir := filepath.Join(profile.Root, "tide")
	if err := os.MkdirAll(filepath.Join(themeDir, "assets"), 0o700); err != nil {
		t.Fatal(err)
	}
	var encoded bytes.Buffer
	if err := png.Encode(&encoded, image.NewRGBA(image.Rect(0, 0, 2, 2))); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"assets/light.png", "assets/dark.png"} {
		if err := os.WriteFile(filepath.Join(themeDir, filepath.FromSlash(name)), encoded.Bytes(), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	writeManifest := func(description string) {
		variant := func(mode string) map[string]any {
			return map[string]any{
				"theme": map[string]any{"mode": mode, "colors": map[string]any{"background": "#edf5fb", "group_box.background": "#e0eef5"}},
				"assets": map[string]any{
					"background.content": map[string]any{"path": "assets/" + mode + ".png", "fit": "cover", "opacity": 0.8, "alignment": []any{0.0, 0.5}},
				},
			}
		}
		manifest, err := json.Marshal(map[string]any{
			"version": 1, "id": "tide", "name": "Tide", "description": description, "package_version": "0.1.0",
			"license": map[string]any{"author": "Sailry", "name": "CC0-1.0", "text": "Original test artwork"},
			"light":   variant("light"), "dark": variant("dark"),
		})
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(themeDir, "theme.json"), manifest, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	writeManifest("A coastal theme")

	outPath := filepath.Join(t.TempDir(), "tide.zip")
	r := &Runtime{
		ctx:         t.Context(),
		App:         &shell.App{},
		Preferences: &preferences.Store{},
		packages:    newPackageState(),
		attachments: &attachmentState{native: NativeFileActions{
			SaveFile: func(_ string, reply func(string)) { reply(outPath) },
		}},
	}
	r.packages.profile = profile
	tester := ui.NewTester(func(c *ui.Context) {
		r.mu.Lock()
		queue := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, fn := range queue {
			fn(c)
		}
	}, 320, 200)

	r.readInstalledThemes()
	r.workers.Wait()
	tester.Frame()

	stored, exists := r.packages.themes["tide"]
	if !exists || stored.Package == nil || stored.Digest == "" {
		t.Fatalf("installed theme metadata not loaded: %+v", stored)
	}
	if stored.Files != nil {
		t.Fatal("installed theme retained raw Files bytes in runtime state")
	}

	r.exportTheme(stored.Package)
	r.workers.Wait()
	tester.Frame()
	exported, err := theme.ReadSnapshot(t.Context(), outPath)
	if err != nil || exported.Digest != stored.Digest {
		t.Fatalf("exported ZIP = %q, %v (want digest %q)", exported.Digest, err, stored.Digest)
	}

	// Mutating the installed theme on disk must fail export with a conflict
	// rather than silently exporting mismatched files.
	writeManifest("Changed on disk")
	conflictPath := filepath.Join(t.TempDir(), "conflict.zip")
	r.attachments.native.SaveFile = func(_ string, reply func(string)) { reply(conflictPath) }
	r.exportTheme(stored.Package)
	r.workers.Wait()
	tester.Frame()
	if _, err := os.Stat(conflictPath); !os.IsNotExist(err) {
		t.Fatalf("stale export should not publish archive, stat err = %v", err)
	}
}
