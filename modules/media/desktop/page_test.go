package desktop

import (
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

var testCatalog = i18n.New(Locales)

func TestMediaModelRows(t *testing.T) {
	s := settings.New()
	s.SetNativePages([]settings.NativePage{Page()})
	s.Open(settings.Media)
	s.MediaOptions = []viewmodel.MediaOption{
		{Key: "a/see", Label: "A · see", Kinds: []string{"vision"}},
		{Key: "a/draw", Label: "A · draw", Kinds: []string{"image"}},
		{Key: "a/all", Label: "A · all", Kinds: []string{"vision", "image", "video", "speech"}},
		{Key: "b/gone", Label: "B · gone"},
	}
	s.Media.Saved = [4]string{"", "", "", "b/gone"}
	s.Media.Draft = s.Media.Saved
	s.MediaUnavailable = [4]bool{false, false, false, true}
	var toasts []string
	tt := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, testCatalog)
		settings.Page(c, s, nil, settings.Look{})
		ui.ToastViewportBase(c, func(_ ui.Element, shown []ui.Toast) {
			toasts = toasts[:0]
			for _, toast := range shown {
				toasts = append(toasts, toast.Title)
			}
		})
	}, 900, 1600, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	offered := func(row string) (labels []string) {
		t.Helper()
		click(t, tt, ui.RoleButton, row)
		for _, x := range tt.AccessAll() {
			if x.Role == ui.RoleOption {
				labels = append(labels, x.Name)
			}
		}
		return
	}
	follow := testCatalog.In("en", "media_follow")
	for row, want := range map[string][]string{
		"Vision": {follow, "see", "all"},
		"Image":  {follow, "draw", "all"},
		"Video":  {follow, "all"},
		"Speech": {follow, "all"},
	} {
		if got := offered(row); !slices.Equal(got, want) {
			t.Errorf("%s offers %q, want %q", row, got, want)
		}
		tt.Key(0, ui.KeyEscape)
		tt.Frame()
		tt.Advance(time.Second)
	}
	unavailable := testCatalog.In("en", "media_model_unavailable")
	marked := func() (count int) {
		for _, x := range tt.AccessAll() {
			if x.Role == ui.RoleButton && x.Description == unavailable {
				count++
			}
		}
		return
	}
	if speech, ok := tt.AccessByRole(ui.RoleButton, "Speech"); marked() != 1 || !ok || speech.Value != "gone" || speech.Description != unavailable {
		t.Errorf("%d rows marked unavailable, want the Speech row alone: %+v", marked(), speech)
	}
	offered("Image")
	click(t, tt, ui.RoleOption, "draw")
	if s.Media.Draft != [4]string{"", "a/draw", "", "b/gone"} || s.Media.Saved != s.Media.Draft {
		t.Errorf("chosen image model: draft %q, saved %q", s.Media.Draft, s.Media.Saved)
	}
	if len(toasts) != 0 {
		t.Errorf("a saved choice toasted %q", toasts)
	}
	offered("Speech")
	click(t, tt, ui.RoleOption, follow)
	if marked() != 0 || s.Media.Draft[3] != "" {
		t.Errorf("after Follow conversation model: %d marks, draft %q", marked(), s.Media.Draft)
	}
}

func TestMediaModelSelectorSnapshots(t *testing.T) {
	if os.Getenv("SAILRY_SNAPSHOTS") == "" {
		return
	}
	shots := filepath.Join("..", "..", "..", ".runtime", "ui-shots", "model-select")
	if err := os.MkdirAll(shots, 0o755); err != nil {
		t.Fatal(err)
	}
	page := Page()
	for _, mode := range []theme.Mode{theme.Dark, theme.Light} {
		suffix := map[theme.Mode]string{theme.Light: "light", theme.Dark: "dark"}[mode]
		s := settings.New()
		tt := ui.NewTesterWith(func(c *ui.Context) {
			i18n.Bind(c, testCatalog)
			theme.Apply(c, "", mode)
			ui.Column(c).Fill().Padding(24).Background(theme.Of(c).Background).Children(func() { page.Render(c, s) })
		}, 760, 560, ui.TesterOptions{Dark: mode == theme.Dark, Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.SetScale(2)
		tt.Frame()
		click(t, tt, ui.RoleButton, testCatalog.In("en", "media_image"))
		tt.Frame()
		out, err := os.Create(filepath.Join(shots, "settings-media-"+suffix+".png"))
		if err != nil {
			t.Fatal(err)
		}
		writeErr := png.Encode(out, tt.Image())
		closeErr := out.Close()
		if writeErr != nil {
			t.Fatal(writeErr)
		}
		if closeErr != nil {
			t.Fatal(closeErr)
		}
	}
}

func click(t *testing.T, tt *ui.Tester, role ui.Role, name string) {
	t.Helper()
	item, ok := tt.AccessByRole(role, name)
	if !ok {
		t.Fatalf("no %q: %q", name, tt.Texts())
	}
	tt.ClickAt(item.Bounds.X+item.Bounds.W/2, item.Bounds.Y+item.Bounds.H/2)
	tt.Frame()
	tt.Advance(time.Second)
}
