package live

import (
	"testing"

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
