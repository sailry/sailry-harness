package shell

import (
	"fmt"
	"image/png"
	"math"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// sent records what a composer built alone sends; its other actions are not
// reached by these steps.
type sent struct {
	composer.Actions
	messages []composer.Message
	// keep is the state whose draft Send puts back, as an owner does that
	// holds the draft until the send is confirmed; nil lets it clear.
	keep *composer.State
}

func (s *sent) Send(m composer.Message) {
	s.messages = append(s.messages, m)
	if s.keep != nil {
		s.keep.Draft = m.Text
	}
}

// composerAlone builds the selected preview conversation's composer by
// itself in a window of the given width, so its toolbar folds at the
// composer's own widths (the surface is the window less 48, the toolbar 24
// less again). prepare changes the state and context of every frame.
func composerAlone(t *testing.T, locale string, width int, prepare func(*composer.State, *composer.Context)) (*ui.Tester, *composer.State, *sent) {
	t.Helper()
	a := New(preview.Load())
	cv := a.Data.Find(a.Sidebar.Selected)
	comp := a.Composer(cv.ID)
	comp.Metrics = func(c *ui.Context, compact bool, open *bool) {
		conversation.UsageBar(c, nil, [][]conversation.Metric{conversation.Metrics(c, cv.Session.Usage)}, compact, open)
	}
	comp.Indicator = func(c *ui.Context) { conversation.ContextRing(c, cv.Session.Usage) }
	actions := &sent{}
	tt := ui.NewTesterWith(func(c *ui.Context) {
		ctx := preview.Composer(a.Data, cv, comp.Worktree)
		ctx.Interactions, ctx.Queue = composer.Interactions{}, nil
		if prepare != nil {
			prepare(comp, ctx)
		}
		ui.Column(c).Fill().Justify(ui.End).Children(func() { composer.View(c, comp, ctx, actions) })
	}, width, 420, opaque)
	tt.SetLocale(locale)
	tt.Frame()
	tt.Frame()
	return tt, comp, actions
}

// shot saves the frame under SAILRY_UI_SHOTS for review; without the
// variable it does nothing.
func shot(t *testing.T, tt *ui.Tester, name string) {
	t.Helper()
	dir := os.Getenv("SAILRY_UI_SHOTS")
	if dir == "" {
		return
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	f, err := os.Create(filepath.Join(dir, name+".png"))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if err := png.Encode(f, tt.Image()); err != nil {
		t.Fatal(err)
	}
}

// noModels is a composer whose Node offers no model.
func noModels(_ *composer.State, ctx *composer.Context) { ctx.Models, ctx.Families = nil, nil }

// ring finds the context ring of the selected conversation.
func ring(t *testing.T, a *App, tt *ui.Tester) ui.Rect {
	t.Helper()
	u := a.Data.Find(a.Sidebar.Selected).Session.Usage
	label := fmt.Sprintf("%s %d%%", i18n.In("en", "composer_compression_hint"), int(u.Context*100+0.5))
	r, ok := tt.Find(label)
	if !ok {
		t.Fatalf("no context ring %q: %q", label, tt.Texts())
	}
	return r
}

// The context ring sits in the composer's toolbar directly left of the model
// chip, not after the usage figures under the composer.
func TestContextRingBesideModel(t *testing.T) {
	a, tt := newTester(t)
	r := ring(t, a, tt)
	m := a.Composer(a.Sidebar.Selected).Chosen(a.Data.Models)
	chip, ok := tt.Access(i18n.In("en", "composer_model_settings") + ": " + m.Name)
	if !ok {
		t.Fatalf("no model chip for %q", m.Name)
	}
	b := chip.Bounds
	if r.W != kit.RingSize || r.H != kit.RingSize {
		t.Errorf("ring is %vx%v, want the Kit ring's %v", r.W, r.H, kit.RingSize)
	}
	if gap := b.X - (r.X + r.W); gap < 0 || gap > 12 {
		t.Errorf("ring ends %v before the chip, want it directly left", gap)
	}
	if mid, chipMid := r.Y+r.H/2, b.Y+b.H/2; mid < chipMid-2 || mid > chipMid+2 {
		t.Errorf("ring centre %v is off the chip's row %v", mid, chipMid)
	}
	t.Run("frames", func(t *testing.T) {
		for _, width := range []int{960, 640, 440} {
			tt, _, _ := composerAlone(t, "zh-CN", width, withSession(nil))
			shot(t, tt, fmt.Sprintf("model-%d", width))
			tt, _, _ = composerAlone(t, "zh-CN", width, noModels)
			shot(t, tt, fmt.Sprintf("no-model-%d", width))
		}
	})
	t.Run("session ring", sessionRing)
	t.Run("ring pixels", ringPixels)
	t.Run("ring tooltip", ringTooltip)
	t.Run("ring folds", ringFolds)
	t.Run("send without a model", sendWithoutModel)
}

// ringHint is the hint the statistics package publishes for a measured
// context (composer_context_usage, "%{used} / %{limit}" in every locale).
const ringHint = "50.0k / 200.0k"

// withSession gives the composer a live session's ring in place of the
// preview's, then applies more.
func withSession(more func(*composer.State, *composer.Context)) func(*composer.State, *composer.Context) {
	return func(s *composer.State, ctx *composer.Context) {
		s.Indicator = nil
		ctx.Indicators = []composer.Indicator{{ID: "context", Percent: 25, Hint: ringHint}}
		if more != nil {
			more(s, ctx)
		}
	}
}

// A session's ring is a picture 15 across, 4 and the toolbar's gap of 4
// before the model chip and on its row; a click on it does nothing.
func sessionRing(t *testing.T) {
	tt, comp, _ := composerAlone(t, "en", 960, withSession(nil))
	shot(t, tt, "session-ring-960")
	r, ok := tt.Access(ringHint)
	if !ok {
		t.Fatalf("no ring %q", ringHint)
	}
	if r.Role == ui.RoleButton || r.HasPopup != ui.PopupNone || r.Focusable {
		t.Errorf("the ring is a control: role %v, popup %v, focusable %v", r.Role, r.HasPopup, r.Focusable)
	}
	if r.Bounds.W != 15 || r.Bounds.H != 15 {
		t.Errorf("ring is %vx%v, want 15x15", r.Bounds.W, r.Bounds.H)
	}
	name := i18n.In("en", "composer_model_settings") + ": " + comp.Chosen(preview.Load().Models).Name
	chip, ok := tt.Access(name)
	if !ok {
		t.Fatalf("no model chip %q", name)
	}
	if gap := chip.Bounds.X - (r.Bounds.X + r.Bounds.W); gap != 8 {
		t.Errorf("ring ends %v before the chip, want 8", gap)
	}
	if mid, row := r.Bounds.Y+r.Bounds.H/2, chip.Bounds.Y+chip.Bounds.H/2; mid != row {
		t.Errorf("ring centre %v is off the chip's centre %v", mid, row)
	}
	mic, ok := tt.Access(i18n.In("en", "dictation_start"))
	if !ok || mic.Bounds.X < chip.Bounds.X+chip.Bounds.W {
		t.Errorf("the microphone at %v is not after the chip %v", mic.Bounds, chip.Bounds)
	}
	tt.ClickAt(r.Bounds.X+r.Bounds.W/2, r.Bounds.Y+r.Bounds.H/2)
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if chip, _ := tt.Access(name); chip.Expanded {
		t.Errorf("a click on the ring opened the model controls")
	}
	if _, ok := tt.AccessByRole(ui.RoleDialog, ""); ok {
		t.Errorf("a click on the ring opened a panel")
	}

	// A draft without a session has no ring.
	tt, _, _ = composerAlone(t, "zh-CN", 960, func(s *composer.State, ctx *composer.Context) {
		s.Indicator = nil
		noModels(s, ctx)
	})
	shot(t, tt, "draft-no-session-no-model-960")
	tt, _, _ = composerAlone(t, "en", 960, func(s *composer.State, ctx *composer.Context) { s.Indicator = nil })
	shot(t, tt, "draft-no-session-960")
	if _, ok := tt.Access(ringHint); ok {
		t.Errorf("a ring shows without a session")
	}
	for _, e := range tt.AccessAll() {
		if e.Bounds.W == 15 && e.Bounds.H == 15 {
			t.Errorf("a ring-sized element %q shows without a session", e.Name)
		}
	}
}

// The arc is the part used, clockwise from the top, over a fainter track,
// in the tone's color.
func ringPixels(t *testing.T) {
	// at reads the ring's line at a clock angle, in turns from the top.
	at := func(tt *ui.Tester, turn float64) (r, g, b float64) {
		e, ok := tt.Access(ringHint)
		if !ok {
			t.Fatalf("no ring %q", ringHint)
		}
		const scale = 2
		radius := (15 - 15*0.15) / 2
		a := 2*math.Pi*turn - math.Pi/2
		x := (float64(e.Bounds.X) + 7.5 + radius*math.Cos(a)) * scale
		y := (float64(e.Bounds.Y) + 7.5 + radius*math.Sin(a)) * scale
		c := tt.Image().RGBAAt(int(x), int(y))
		return float64(c.R), float64(c.G), float64(c.B)
	}
	// ink is how far a pixel is from the surface the ring is on.
	ink := func(tt *ui.Tester, turn float64) float64 {
		e, _ := tt.Access(ringHint)
		bg := tt.Image().RGBAAt(int(e.Bounds.X+7.5)*2, int(e.Bounds.Y+7.5)*2)
		r, g, b := at(tt, turn)
		return math.Abs(r-float64(bg.R)) + math.Abs(g-float64(bg.G)) + math.Abs(b-float64(bg.B))
	}
	frame := func(in composer.Indicator) *ui.Tester {
		tt, _, _ := composerAlone(t, "en", 960, func(s *composer.State, ctx *composer.Context) {
			s.Indicator = nil
			in.ID, in.Hint = "context", ringHint
			ctx.Indicators = []composer.Indicator{in}
		})
		tt.SetScale(2)
		tt.Frame()
		return tt
	}

	quarter := frame(composer.Indicator{Percent: 25})
	arc, track := ink(quarter, 0.125), ink(quarter, 0.5)
	if track < 20 {
		t.Errorf("the track does not show: %v from the surface", track)
	}
	if arc < 3*track {
		t.Errorf("at an eighth of a turn the arc is %v from the surface, the track %v: want the arc of 25%%", arc, track)
	}
	for _, turn := range []float64{0.3, 0.5, 0.75, 0.95} {
		if got := ink(quarter, turn); got > 1.5*track {
			t.Errorf("at %v of a turn the ring is %v from the surface, want the track's %v past 25%%", turn, got, track)
		}
	}

	empty := frame(composer.Indicator{})
	for _, turn := range []float64{0.02, 0.125, 0.5} {
		if got, want := ink(empty, turn), ink(quarter, 0.5); math.Abs(got-want) > 12 {
			t.Errorf("an unmeasured ring is %v from the surface at %v of a turn, want the track's %v", got, turn, want)
		}
	}

	half := frame(composer.Indicator{Percent: 50})
	if in, out := ink(half, 0.45), ink(half, 0.55); in < 3*out {
		t.Errorf("50%%: %v before the half turn and %v after it, want the arc to end there", in, out)
	}

	full := frame(composer.Indicator{Percent: 130, Tone: kit.Negative})
	for _, turn := range []float64{0.125, 0.5, 0.875} {
		if r, g, _ := at(full, turn); r < g+60 {
			t.Errorf("a full danger ring is rgb(%v, %v, …) at %v of a turn, want the danger color", r, g, turn)
		}
	}
	warn := frame(composer.Indicator{Percent: 85, Tone: kit.Caution})
	if r, _, b := at(warn, 0.5); r < b+60 {
		t.Errorf("a warning ring is r %v b %v, want the warning color", r, b)
	}
	if r, g, b := at(quarter, 0.125); math.Abs(r-g) > 24 || math.Abs(g-b) > 24 {
		t.Errorf("a muted ring is rgb(%v, %v, %v), want the muted text color", r, g, b)
	}
}

// The tooltip is the package's hint as it is: used / limit, the estimate,
// or the compaction; the preview's ring keeps its own hint.
func ringTooltip(t *testing.T) {
	hover := func(tt *ui.Tester, name string) {
		t.Helper()
		e, ok := tt.Access(name)
		if !ok {
			t.Fatalf("no ring %q", name)
		}
		tt.Move(e.Bounds.X+e.Bounds.W/2, e.Bounds.Y+e.Bounds.H/2)
		tt.Frame()
		tt.Advance(2 * time.Second)
		tt.Frame()
		tt.Frame()
	}
	for locale, hint := range map[string]string{
		"en":    "Estimated context 120 / 1.0k",
		"zh-CN": "估算上下文 120 / 1.0k",
	} {
		tt, _, _ := composerAlone(t, locale, 960, func(s *composer.State, ctx *composer.Context) {
			s.Indicator = nil
			ctx.Indicators = []composer.Indicator{{ID: "context", Percent: 12, Hint: hint}}
		})
		// The ring's own name counts as a text: the tooltip is one more.
		shows := func() int {
			n := 0
			for _, text := range tt.Texts() {
				if text == hint {
					n++
				}
			}
			return n
		}
		before := shows()
		hover(tt, hint)
		shot(t, tt, "ring-tooltip-"+locale)
		if after := shows(); after != before+1 {
			t.Errorf("%s: %q shows %d times at rest and %d under the pointer, want the tooltip: %q", locale, hint, before, after, tt.Texts())
		}
		if e, _ := tt.Access(hint); e.Description != hint && e.Name != hint {
			t.Errorf("%s: the ring is named %q, described %q", locale, e.Name, e.Description)
		}
	}
	// A ring whose value is unknown stays, the track alone, with no tooltip.
	// It has no name, so a named one in its place says where it is, and the
	// toolbar without a ring what it draws.
	alone := func(rings ...composer.Indicator) *ui.Tester {
		tt, _, _ := composerAlone(t, "zh-CN", 960, func(s *composer.State, ctx *composer.Context) {
			s.Indicator = nil
			ctx.Indicators = rings
		})
		return tt
	}
	named, found := alone(composer.Indicator{ID: "context", Hint: "known"}).Access("known")
	if !found {
		t.Fatal("no named ring")
	}
	at := named.Bounds
	drawn := func(tt *ui.Tester) string {
		img := tt.Image()
		scale := float32(img.Bounds().Dx()) / 960
		var b strings.Builder
		for y := int(at.Y * scale); y < int((at.Y+at.H)*scale); y++ {
			for x := int(at.X * scale); x < int((at.X+at.W)*scale); x++ {
				fmt.Fprint(&b, img.At(x, y))
			}
		}
		return b.String()
	}
	tt := alone(composer.Indicator{ID: "context"})
	if drawn(tt) == drawn(alone()) {
		t.Error("the unknown ring is not drawn")
	}
	rest := slices.Clone(tt.Texts())
	tt.Move(at.X+at.W/2, at.Y+at.H/2)
	tt.Frame()
	tt.Advance(2 * time.Second)
	tt.Frame()
	tt.Frame()
	shot(t, tt, "ring-unknown-zh-CN")
	if under := tt.Texts(); !slices.Equal(under, rest) || slices.Contains(under, "0") {
		t.Errorf("the unknown ring shows a tooltip: %q at rest, %q under the pointer", rest, under)
	}
	for _, locale := range []string{"en", "zh-CN"} {
		a, tt := newTester(t)
		tt.SetLocale(locale)
		tt.Frame()
		tt.Frame()
		u := a.Data.Find(a.Sidebar.Selected).Session.Usage
		hint := i18n.In(locale, "composer_compression_hint")
		hover(tt, fmt.Sprintf("%s %d%%", hint, int(u.Context*100+0.5)))
		if !slices.Contains(tt.Texts(), hint) {
			t.Errorf("%s: the preview ring's tooltip lacks %q", locale, hint)
		}
	}
}

// Below 680 the model chip is in the settings popover and the ring stays in
// the toolbar, before the microphone; below 440 the location leaves the
// context bar for that popover.
func ringFolds(t *testing.T) {
	project := preview.Load().Projects[0].Name
	// places are where the composer slot's package controls were asked for.
	var places []composer.ControlPlace
	recorded := withSession(func(s *composer.State, _ *composer.Context) {
		s.SetPluginControls(func(_ *ui.Context, slot string, place composer.ControlPlace) {
			if slot == "composer" {
				places = append(places, place)
			}
		})
	})
	// The wide toolbar has them at its two ends.
	composerAlone(t, "en", 960, recorded)
	if !slices.Contains(places, composer.ControlStart) || !slices.Contains(places, composer.ControlEnd) || slices.Contains(places, composer.ControlMenu) {
		t.Errorf("the wide toolbar places the package controls at %v", places)
	}
	for _, width := range []int{640, 440} {
		tt, comp, _ := composerAlone(t, "en", width, recorded)
		// The first frame is built before the composer knows its width.
		places = nil
		tt.Frame()
		if len(places) != 0 {
			t.Errorf("%d: the compact toolbar keeps the package controls at %v", width, places)
		}
		shot(t, tt, fmt.Sprintf("session-ring-%d", width))
		r, ok := tt.Access(ringHint)
		if !ok {
			t.Fatalf("%d: no ring", width)
		}
		mic, ok := tt.Access(i18n.In("en", "dictation_start"))
		if !ok {
			t.Fatalf("%d: no microphone", width)
		}
		if gap := mic.Bounds.X - (r.Bounds.X + r.Bounds.W); gap != 8 {
			t.Errorf("%d: ring ends %v before the microphone, want 8", width, gap)
		}
		if _, ok := tt.Access(i18n.In("en", "composer_model_settings") + ": " + comp.Chosen(preview.Load().Models).Name); ok {
			t.Errorf("%d: the model chip shows in the compact toolbar", width)
		}
		_, bar := tt.AccessByRole(ui.RoleMenuButton, project)
		if want := width >= 520; bar != want {
			t.Errorf("%d: the project button on the context bar is %v, want %v", width, bar, want)
		}
		settings, ok := tt.AccessByRole(ui.RoleButton, i18n.In("en", "composer_settings")+" · "+i18n.In("en", "composer_mode_code")+" · "+i18n.In("en", "composer_permission_ask"))
		if !ok {
			t.Fatalf("%d: no settings button: %q", width, tt.Texts())
		}
		tt.ClickAt(settings.Bounds.X+settings.Bounds.W/2, settings.Bounds.Y+settings.Bounds.H/2)
		tt.Frame()
		tt.Advance(time.Second)
		tt.Frame()
		shot(t, tt, fmt.Sprintf("settings-%d", width))
		if !slices.Contains(places, composer.ControlMenu) || slices.Contains(places, composer.ControlStart) || slices.Contains(places, composer.ControlEnd) {
			t.Errorf("%d: the settings popover places the package controls at %v", width, places)
		}
		for _, key := range []string{"composer_mode_code", "composer_permission_ask"} {
			if !tt.HasText(i18n.In("en", key)) {
				t.Errorf("%d: the settings popover lacks %s", width, key)
			}
		}
		row, ok := tt.AccessByRole(ui.RoleMenuButton, project)
		if !ok {
			t.Fatalf("%d: no project control with the settings popover open", width)
		}
		if width < 520 {
			// The row in the popover opens the project menu.
			tt.ClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
			tt.Frame()
			tt.Advance(time.Second)
			tt.Frame()
			shot(t, tt, fmt.Sprintf("settings-project-%d", width))
			if !tt.HasText(i18n.In("en", "project_add")) {
				t.Errorf("%d: the project row opened no menu: %q", width, tt.Texts())
			}
		}
	}
	// The review's frames: the session's composer in Chinese at each width.
	if os.Getenv("SAILRY_UI_SHOTS") != "" {
		for _, width := range []int{960, 640, 440} {
			for _, mode := range []string{"light", "dark"} {
				tt, _, _ := composerAlone(t, "zh-CN", width, withSession(nil))
				tt.SetDark(mode == "dark")
				tt.Frame()
				tt.Frame()
				shot(t, tt, fmt.Sprintf("composer-%d-zh-CN-%s", width, mode))
			}
		}
	}
}

// Send looks disabled and does nothing while the draft is empty, no model
// is offered or the owner blocks it; the chip then reads Select model and
// opens the settings.
func sendWithoutModel(t *testing.T) {
	press := func(tt *ui.Tester) ui.AccessInfo {
		t.Helper()
		send, ok := tt.AccessByRole(ui.RoleButton, i18n.In("en", "send"))
		if !ok {
			t.Fatalf("no send button")
		}
		tt.ClickAt(send.Bounds.X+send.Bounds.W/2, send.Bounds.Y+send.Bounds.H/2)
		tt.Frame()
		return send
	}
	enter := func(tt *ui.Tester) {
		t.Helper()
		draft, ok := tt.AccessByRole(ui.RoleTextField, i18n.In("en", "composer"))
		if !ok {
			t.Fatalf("no draft")
		}
		tt.ClickAt(draft.Bounds.X+draft.Bounds.W/2, draft.Bounds.Y+draft.Bounds.H/2)
		tt.Frame()
		tt.Key(0, ui.KeyEnter)
		tt.Frame()
	}
	tt, _, actions := composerAlone(t, "en", 960, nil)
	if send := press(tt); !send.Disabled {
		t.Errorf("send is enabled with an empty draft")
	}
	if len(actions.messages) != 0 {
		t.Errorf("an empty draft was sent")
	}

	selected := 0
	tt, comp, actions := composerAlone(t, "en", 960, func(s *composer.State, ctx *composer.Context) {
		noModels(s, ctx)
		ctx.SelectModel = func() { selected++ }
	})
	comp.Draft = "Continue"
	tt.Frame()
	shot(t, tt, "no-model-draft-960")
	if send := press(tt); !send.Disabled {
		t.Errorf("send is enabled without a model")
	}
	enter(tt)
	if len(actions.messages) != 0 || comp.Draft != "Continue" {
		t.Errorf("without a model %d messages were sent and the draft is %q", len(actions.messages), comp.Draft)
	}
	if tt.HasText(i18n.In("en", "composer_model_empty")) {
		t.Errorf("the chip shows the picker's empty text")
	}
	chip, ok := tt.AccessByRole(ui.RoleButton, i18n.In("en", "composer_select_model"))
	if !ok || chip.Disabled {
		t.Fatalf("no enabled Select model chip: %+v", chip)
	}
	tt.ClickAt(chip.Bounds.X+chip.Bounds.W/2, chip.Bounds.Y+chip.Bounds.H/2)
	tt.Frame()
	if selected != 1 {
		t.Errorf("Select model opened the settings %d times", selected)
	}
	// Without an owner to open the settings the chip is disabled.
	tt, _, _ = composerAlone(t, "zh-CN", 960, noModels)
	if chip, ok := tt.AccessByRole(ui.RoleButton, i18n.In("zh-CN", "composer_select_model")); !ok || !chip.Disabled {
		t.Errorf("the chip without SelectModel: %+v", chip)
	}

	tt, comp, actions = composerAlone(t, "en", 960, func(_ *composer.State, ctx *composer.Context) { ctx.SendBlocked = true })
	comp.Draft = "Continue"
	tt.Frame()
	if send := press(tt); !send.Disabled {
		t.Errorf("send is enabled while the owner blocks it")
	}
	enter(tt)
	if len(actions.messages) != 0 || comp.Draft != "Continue" {
		t.Errorf("a blocked draft was sent")
	}

	tt, comp, actions = composerAlone(t, "en", 960, nil)
	comp.Draft = "Continue"
	tt.Frame()
	if send := press(tt); send.Disabled {
		t.Errorf("send is disabled with a draft and a model")
	}
	if len(actions.messages) != 1 || actions.messages[0].Text != "Continue" || comp.Draft != "" {
		t.Errorf("send delivered %+v and left the draft %q", actions.messages, comp.Draft)
	}
	// Enter sends the same way and clears the draft the owner let go.
	comp.Draft = "Again"
	tt.Frame()
	enter(tt)
	if len(actions.messages) != 2 || actions.messages[1].Text != "Again" || comp.Draft != "" {
		t.Errorf("Enter delivered %+v and left the draft %q", actions.messages, comp.Draft)
	}
	// An owner that keeps the draft until the send is confirmed keeps it on
	// Enter as on the button, in the state and in the editor.
	actions.keep = comp
	comp.Draft = "Held"
	tt.Frame()
	enter(tt)
	tt.Frame()
	field, _ := tt.AccessByRole(ui.RoleTextField, i18n.In("en", "composer"))
	if len(actions.messages) != 3 || comp.Draft != "Held" || field.Value != "Held" {
		t.Errorf("Enter cleared the draft its owner kept: %d messages, draft %q, editor %q", len(actions.messages), comp.Draft, field.Value)
	}
	actions.keep = nil
}

// Below the compact width the usage under the composer is one button after
// the location, not over it; a click holds every figure in a popover. Below
// the icon width the location is in the settings popover, not on the bar.
func TestUsageCompact(t *testing.T) {
	a, tt := newTester(t)
	// 900 leaves the composer between the icon and the compact widths.
	tt.SetSize(900, 820)
	tt.Frame()
	tt.Frame()
	stats, ok := tt.Access(i18n.In("en", "composer_statistics"))
	if !ok {
		t.Fatalf("no compact usage button: %q", tt.Texts())
	}
	u := a.Data.Find(a.Sidebar.Selected).Session.Usage
	if tt.HasText(fmt.Sprint(u.Turns)) {
		t.Errorf("the turns figure shows outside the popover")
	}
	branch, ok := tt.AccessByRole(ui.RoleMenuButton, a.Composer(a.Sidebar.Selected).Worktree)
	if !ok {
		t.Fatalf("no worktree button")
	}
	if end := branch.Bounds.X + branch.Bounds.W; end > stats.Bounds.X {
		t.Errorf("the worktree button ends at %v, over the usage at %v", end, stats.Bounds.X)
	}
	if branch.Bounds.H != 24 {
		t.Errorf("the worktree button is %v high, want Kit's small 24", branch.Bounds.H)
	}
	tt.SetSize(760, 820)
	tt.Frame()
	tt.Frame()
	if stats, ok = tt.Access(i18n.In("en", "composer_statistics")); !ok {
		t.Fatalf("no compact usage button below the icon width: %q", tt.Texts())
	}
	if _, ok := tt.AccessByRole(ui.RoleMenuButton, a.Composer(a.Sidebar.Selected).Worktree); ok {
		t.Errorf("the worktree button stays on the context bar below the icon width")
	}
	b := stats.Bounds
	tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	for _, key := range []string{"composer_tokens", "composer_turns"} {
		if !tt.HasText(i18n.In("en", key)) {
			t.Errorf("the popover lacks %s: %q", key, tt.Texts())
		}
	}
	if !tt.HasText(fmt.Sprint(u.Turns)) {
		t.Errorf("the popover lacks the turns figure")
	}

	// A session's figures are its packages': each has a hover card of its
	// label, its value and its rows, the meters among them; a provider's
	// figures stay on the bar beside the compact button, whose popover
	// lists every group.
	t.Run("rows", func(t *testing.T) {
		// Figures no other text of the composer contains: HasText matches inside a text.
		lead := []conversation.Metric{{ID: "status:usage", Label: "Plan usage", Icon: icons.Gauge, Value: "64%", Rows: []conversation.Row{
			{Label: "Five hours", Value: "64% left", Meter: true, Progress: 64}, {Label: "Resets", Value: "18:00"}}}}
		groups := [][]conversation.Metric{{
			{ID: "statistics:tokens", Label: "Tokens", Icon: icons.ChartPie, Value: "48.2k", Rows: []conversation.Row{
				{Label: "This session", Value: "41.0k"}, {Label: "Subagents", Value: "7.2k"}}},
			{ID: "statistics:turns", Label: "Turns", Icon: icons.MessagesSquare, Value: "77", Rows: []conversation.Row{{Label: "Responses", Value: "35"}}},
		}, {
			{ID: "other:score", Label: "Score", Value: "9.5", Rows: []conversation.Row{{Label: "Best", Value: "12.5"}}},
		}}
		figures := func(s *composer.State, _ *composer.Context) {
			s.Metrics = func(c *ui.Context, compact bool, open *bool) { conversation.UsageBar(c, lead, groups, compact, open) }
		}
		tt, _, _ := composerAlone(t, "en", 960, figures)
		for _, value := range []string{"64%", "48.2k", "77", "9.5"} {
			if !tt.HasText(value) {
				t.Fatalf("the bar lacks the figure %s: %q", value, tt.Texts())
			}
		}
		if tt.HasText("This session") || tt.HasText("Five hours") {
			t.Errorf("a figure's rows show without a hover: %q", tt.Texts())
		}
		hover := func(value string, rows ...string) {
			t.Helper()
			at, ok := tt.Find(value)
			if !ok {
				t.Fatalf("no figure %s", value)
			}
			tt.Move(at.X+at.W/2, at.Y+at.H/2)
			tt.Frame()
			tt.Advance(time.Second)
			tt.Frame()
			for _, row := range rows {
				if !tt.HasText(row) {
					t.Errorf("the card of %s lacks %q: %q", value, row, tt.Texts())
				}
			}
			tt.Move(0, 0)
			tt.Frame()
			tt.Advance(time.Second)
			tt.Frame()
		}
		hover("48.2k", "Tokens", "This session", "41.0k", "Subagents", "7.2k")
		hover("77", "Turns", "Responses", "35")
		hover("9.5", "Score", "Best", "12.5")
		// The meter is named by its row; the row without one comes first.
		hover("64%", "Plan usage", "Five hours", "64% left", "Resets", "18:00")

		tt, _, _ = composerAlone(t, "en", 600, figures)
		stats, ok := tt.Access(i18n.In("en", "composer_statistics"))
		if !ok || !tt.HasText("64%") || !tt.HasText("48.2k") || tt.HasText("77") {
			t.Fatalf("the compact bar: button %v, texts %q", ok, tt.Texts())
		}
		tt.ClickAt(stats.Bounds.X+stats.Bounds.W/2, stats.Bounds.Y+stats.Bounds.H/2)
		tt.Frame()
		tt.Advance(time.Second)
		tt.Frame()
		for _, row := range []string{"Tokens", "This session", "41.0k", "Turns", "77", "Responses", "35", "Score", "Best", "12.5"} {
			if !tt.HasText(row) {
				t.Errorf("the compact popover lacks %q: %q", row, tt.Texts())
			}
		}
		if tt.HasText("Five hours") {
			t.Errorf("the compact popover lists the provider's rows: %q", tt.Texts())
		}
	})
}

// A pending question's history row shows no state text beside its
// prompt: only the composer's capsule says it waits.
func TestQuestionRowQuiet(t *testing.T) {
	_, tt := newTester(t)
	tt.SetSize(760, 820)
	tt.Frame()
	tt.Frame()
	prompt, ok := tt.Find("Keep the 2x3 loader or use Kit's spinner?")
	if !ok {
		t.Fatalf("no question row: %q", tt.Texts())
	}
	if state, ok := tt.Find(i18n.In("en", "question_reply")); ok && state.Y > prompt.Y-4 && state.Y < prompt.Y+prompt.H {
		t.Errorf("a state text at %v sits on the prompt's row", state)
	}
}
