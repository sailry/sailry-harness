package desktop

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

func TestOCRModelChoices(t *testing.T) {
	assembly := shell.Assemble(Register)
	catalog := i18n.New(assembly.Locales...)
	s := settings.NewLive()
	s.SetNativePages(assembly.SettingsPages)
	s.Open(settings.OCR)
	s.OCR = settings.LocalModels{Kind: settings.ModelsOCR, Model: -1, Models: []preview.LocalModel{
		{ID: "a", Name: "Alpha", Status: preview.ModelInstalled},
		{ID: "x", Name: "Not installed"},
		{ID: "b", Name: "Beta", Status: preview.ModelInstalled},
	}}
	s.OCR.Enabled.Draft = true
	var saves []int
	s.ModelActions = &settings.ModelActions{Save: func(kind string) {
		if kind != settings.ModelsOCR {
			t.Fatalf("OCR saved another model kind: %q", kind)
		}
		saves = append(saves, s.OCR.Model)
	}}
	view := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, catalog)
		settings.Page(c, s, nil, settings.Look{})
	}, 900, 700, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	view.SetLocale("en")
	view.Frame()
	find := func(role ui.Role, name string) ui.AccessInfo {
		t.Helper()
		x, ok := view.AccessByRole(role, name)
		if !ok {
			t.Fatalf("no %q control: %q", name, view.Texts())
		}
		return x
	}
	click := func(role ui.Role, name string) {
		x := find(role, name)
		view.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
		view.Frame()
		view.Advance(time.Second)
	}
	label, auto := catalog.In("en", "runtime_ocr_model"), catalog.In("en", "search_driver_auto")
	if got := find(ui.RoleButton, label).Value; got != auto {
		t.Fatalf("no model reads %q, want %q", got, auto)
	}
	click(ui.RoleButton, label)
	if _, listed := view.AccessByRole(ui.RoleOption, "Not installed"); listed {
		t.Error("a model that is not installed is offered")
	}
	click(ui.RoleOption, "Beta")
	if got := find(ui.RoleButton, label).Value; s.OCR.Model != 1 || got != "Beta" {
		t.Fatalf("Beta stored %d and showed %q", s.OCR.Model, got)
	}
	click(ui.RoleButton, label)
	click(ui.RoleOption, auto)
	if s.OCR.Model != -1 || !slices.Equal(saves, []int{1, -1}) {
		t.Fatalf("%s stored %d; saved %v", auto, s.OCR.Model, saves)
	}
	s.OCR.Model, s.OCR.Missing = 2, "Gamma"
	view.Frame()
	if got := find(ui.RoleButton, label).Value; got != "Gamma" {
		t.Errorf("the missing model reads %q", got)
	}
	s.OCR.Enabled.Draft = false
	view.Frame()
	if !find(ui.RoleButton, label).Disabled {
		t.Error("the select of a disabled kind is enabled")
	}
}
