package shell

import (
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// composerPicker opens the model picker of a composer in a window width
// wide, from the model chip or, at the compact width, from the settings
// popover: Claude Opus 5.5 at medium. The frame it leaves is the first of
// the picker's page: no time has passed since the press. calm tells
// that the composer asked for no frame at rest before that, so one asked
// for with the picker open is the picker's.
func composerPicker(t *testing.T, width int, scale float32, prepare func(*composer.State, *composer.Context)) (tt *ui.Tester, comp *composer.State, calm bool) {
	t.Helper()
	tt, comp, _ = composerWith(t, width, prepare)
	comp.Team, comp.Model, comp.Effort, comp.Features = "", "claude-opus-5-5", 2, nil
	tt.SetScale(scale)
	tt.Frame()
	tt.Frame()
	tt.Advance(time.Second)
	calm = !tt.FramePending()
	openControls(t, tt, zh("composer_model_settings")+": ", zh("composer_settings"))
	pressButton(t, tt, "Claude Opus 5.5") // the controls' model, which opens the picker
	return tt, comp, calm
}

// modelPickerAt is composerPicker once the popover is at rest, drawn at
// twice the size for probe.
func modelPickerAt(t *testing.T, width int, prepare func(*composer.State, *composer.Context)) (tt *ui.Tester, comp *composer.State, calm bool) {
	t.Helper()
	tt, comp, calm = composerPicker(t, width, 2, prepare)
	rest(tt)
	return tt, comp, calm
}

// demoPicker is modelPickerAt over the demo's models, one provider with
// capability marks and scores: DeepSeek V4.1 Pro at medium. prepare changes
// the models of each frame.
func demoPicker(t *testing.T, prepare func(models []composer.Model)) *ui.Tester {
	t.Helper()
	tt, comp, _ := composerWith(t, 960, func(_ *composer.State, ctx *composer.Context) {
		ctx.Models, ctx.Families, ctx.Teams = preview.DemoModels(), preview.DemoFamilies, nil
		if prepare != nil {
			prepare(ctx.Models)
		}
	})
	comp.Team, comp.Model, comp.Effort, comp.Features = "", "deepseek-v4.1-pro", 2, nil
	tt.SetScale(2)
	tt.Frame()
	tt.Frame()
	openControls(t, tt, zh("composer_model_settings")+": ", zh("composer_settings"))
	pressButton(t, tt, preview.DemoModel) // the controls' model, which opens the picker
	rest(tt)
	return tt
}

// groupHeader is the picker's group named by its provider: the disclosure
// that folds its models.
func groupHeader(t *testing.T, tt *ui.Tester, provider string) ui.AccessInfo {
	t.Helper()
	header, ok := tt.AccessByRole(ui.RoleDisclosure, provider)
	if !ok {
		t.Fatalf("no group named %q: %q", provider, tt.Texts())
	}
	return header
}

// railTile is the rail's button named name: Back or a family's tile.
func railTile(t *testing.T, tt *ui.Tester, name string) ui.AccessInfo {
	t.Helper()
	tile, ok := tt.AccessByRole(ui.RoleButton, name)
	if !ok {
		t.Fatalf("no rail button %q: %q", name, tt.Texts())
	}
	return tile
}

// effortRow is the effort column's row of a level.
func effortRow(t *testing.T, tt *ui.Tester, level string) ui.AccessInfo {
	t.Helper()
	row, ok := tt.AccessByRole(ui.RoleOption, level)
	if !ok {
		t.Fatalf("no effort row %q: %q", level, tt.Texts())
	}
	return row
}

// clickRest clicks an element at its middle and lets the panel rest.
func clickRest(tt *ui.Tester, e ui.AccessInfo) {
	tt.ClickAt(e.Bounds.X+e.Bounds.W/2, e.Bounds.Y+e.Bounds.H/2)
	rest(tt)
}

// near is whether two lengths are one, within half a DIP.
func near(a, b float32) bool { return a-b <= 0.5 && b-a <= 0.5 }

// openControls opens the model controls' popover from the model chip, whose
// name starts with chip, or without one from the compact settings button,
// and lets its entrance end.
func openControls(t *testing.T, tt *ui.Tester, chip, settings string) {
	t.Helper()
	for _, prefix := range []string{chip, settings} {
		for _, e := range tt.AccessAll() {
			if e.Role == ui.RoleButton && strings.HasPrefix(e.Name, prefix) {
				tt.ClickAt(e.Bounds.X+e.Bounds.W/2, e.Bounds.Y+e.Bounds.H/2)
				tt.Frame()
				tt.Advance(time.Second)
				tt.Frame()
				return
			}
		}
	}
	t.Fatalf("no control %q or %q: %q", chip, settings, tt.Texts())
}

// pickerContained fails unless one popover shows, in the window, and holds
// the effort column named heading, every effort's row and the rows of the
// models named, whole: a press on one past the popover's edge would close
// it. It returns the popover's box.
func pickerContained(t *testing.T, tt *ui.Tester, when, heading string, window ui.Rect, models ...string) ui.Rect {
	t.Helper()
	panels, menus := popups(tt)
	if len(panels) != 1 || menus != 0 {
		t.Fatalf("%s: %d popovers and %d menu elements, want the one panel", when, len(panels), menus)
	}
	panel := panels[0].Bounds
	if !inside(panel, window) {
		t.Errorf("%s: the panel %v leaves the window %v", when, panel, window)
	}
	column, ok := tt.AccessByRole(ui.RoleListBox, heading)
	if !ok {
		t.Fatalf("%s: no effort column named %q: %q", when, heading, tt.Texts())
	}
	if !inside(column.Bounds, panel) || column.Bounds.W != 132 {
		t.Errorf("%s: the effort column is %v, the panel %v: want 132 wide inside it", when, column.Bounds, panel)
	}
	row := func(kind string, e ui.AccessInfo) {
		t.Helper()
		b := e.Bounds
		switch {
		case !inside(b, panel):
			t.Errorf("%s: the %s row %q is %v, past the panel %v", when, kind, e.Name, b, panel)
		case b.H != 32:
			t.Errorf("%s: the %s row %q is %v high, want 32: the panel %v", when, kind, e.Name, b.H, panel)
		}
	}
	efforts := effortRows(tt)
	if len(efforts) == 0 {
		t.Errorf("%s: no effort rows: %q", when, tt.Texts())
	}
	for _, e := range efforts {
		row("effort", e)
		if !inside(e.Bounds, column.Bounds) {
			t.Errorf("%s: the effort row %q is %v, its column %v", when, e.Name, e.Bounds, column.Bounds)
		}
	}
	for _, name := range models {
		model := modelRow(t, tt, name)
		row("model", model)
		if model.Bounds.X+model.Bounds.W > column.Bounds.X {
			t.Errorf("%s: the model row %q is %v, under the effort column %v", when, name, model.Bounds, column.Bounds)
		}
	}
	return panel
}

// atRest fails when the picker asks for frames with nothing moving.
func atRest(t *testing.T, tt *ui.Tester, calm bool, when string) {
	t.Helper()
	tt.Advance(time.Second)
	switch {
	case !tt.FramePending():
	case calm:
		t.Errorf("the picker asks for frames %s", when)
	default:
		t.Errorf("the composer asked for frames at rest before its picker opened, and does %s", when)
	}
}

// rest lets the panel's motion end.
func rest(tt *ui.Tester) {
	tt.Frame()
	tt.Advance(300 * time.Millisecond)
	tt.Frame()
}

// modelRow is the picker's row of the model named name.
func modelRow(t *testing.T, tt *ui.Tester, name string) ui.AccessInfo {
	t.Helper()
	row, ok := tt.AccessByRole(ui.RoleButton, name)
	if !ok {
		t.Fatalf("no model row %q: %q", name, tt.Texts())
	}
	return row
}

// point moves the pointer onto a row: one move there gives the row the
// effort column.
func point(tt *ui.Tester, row ui.AccessInfo) {
	tt.Move(row.Bounds.X+20, row.Bounds.Y+row.Bounds.H/2)
	tt.Frame()
}

// effortRows are the effort column's rows, in order.
func effortRows(tt *ui.Tester) []ui.AccessInfo {
	var rows []ui.AccessInfo
	for _, e := range tt.AccessAll() {
		if e.Role == ui.RoleOption {
			rows = append(rows, e)
		}
	}
	return rows
}

// effortNames are the efforts the column lists and the one it checks.
func effortNames(tt *ui.Tester) (names []string, checked string) {
	for _, e := range effortRows(tt) {
		names = append(names, e.Name)
		if e.Selected {
			checked = e.Name
		}
	}
	return names, checked
}

// pressEffort clicks the effort column's row of a level.
func pressEffort(t *testing.T, tt *ui.Tester, level string) {
	t.Helper()
	row, ok := tt.AccessByRole(ui.RoleOption, level)
	if !ok {
		t.Fatalf("no effort row %q: %q", level, tt.Texts())
	}
	tt.ClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
	rest(tt)
}

// popups are the popovers and menus that show.
func popups(tt *ui.Tester) (panels []ui.AccessInfo, menus int) {
	for _, e := range tt.AccessAll() {
		switch e.Role {
		case ui.RolePopup:
			panels = append(panels, e)
		case ui.RoleMenu, ui.RoleMenuItem, ui.RoleMenuItemCheckBox, ui.RoleMenuItemRadio:
			menus++
		}
	}
	return panels, menus
}

// focusOf is the element with the keyboard focus.
func focusOf(tt *ui.Tester) (ui.Role, string) {
	for _, e := range tt.AccessAll() {
		if e.Focused {
			return e.Role, e.Name
		}
	}
	return ui.RoleNone, ""
}

// inside is whether a box lies in another, within a rounding.
func inside(in, out ui.Rect) bool {
	return in.X >= out.X-0.5 && in.Y >= out.Y-0.5 && in.X+in.W <= out.X+out.W+0.5 && in.Y+in.H <= out.Y+out.H+0.5
}

// composerModels is the model picker's panel: the families' rail, the
// models and the efforts of the model the pointer or the keys are on, three
// columns of one panel with a hairline between them. No menu floats over it.
// A model's row chooses it at the effort its column checks, an effort's row
// at that effort, and the panel goes back to its controls, still open.
// pickerInShell has the picker in the app's window.
func composerModels(t *testing.T) {
	heading := zh("composer_effort")
	none := zh("composer_effort_unavailable")
	anthropic := []string{"Claude Opus 5.5", "Claude Sonnet 5.5", "Claude Haiku 5.5"}

	// The popover has the picker's size in the page's first frame, with no
	// time passed: every row is in it, where a press reaches it.
	for _, width := range []int{960, 440} {
		for _, scale := range []float32{1, 2} {
			t.Run(fmt.Sprintf("panel at once %d at %v", width, scale), func(t *testing.T) {
				tt, comp, _ := composerPicker(t, width, scale, nil)
				window := ui.Rect{W: float32(width), H: 520}
				pickerContained(t, tt, "as the picker opens", heading, window, anthropic...)
				point(tt, modelRow(t, tt, "Claude Sonnet 5.5"))
				pickerContained(t, tt, "with the pointer on a model", heading, window, anthropic...)
				pressEffort(t, tt, "low")
				if comp.Model != "claude-sonnet-5-5" || composer.Efforts[int(comp.Effort)] != "low" {
					t.Fatalf("the effort's row chose %q at effort %v, want claude-sonnet-5-5 at low", comp.Model, comp.Effort)
				}
				if panels, _ := popups(tt); len(panels) != 1 {
					t.Fatalf("%d popovers after choosing an effort, want the panel open", len(panels))
				}
				if _, ok := tt.AccessByRole(ui.RoleSlider, heading); !ok {
					t.Errorf("the panel is not back on its controls: %q", tt.Texts())
				}
				pressButton(t, tt, "Claude Sonnet 5.5")
				rest(tt)
				pickerContained(t, tt, "at rest", heading, window, anthropic...)
			})
		}
	}

	for _, mode := range []string{"dark", "light"} {
		t.Run("effort column "+mode, func(t *testing.T) {
			tt, _, calm := modelPickerAt(t, 960, nil)
			tt.SetDark(mode == "dark")
			rest(tt)
			p := probe{t, tt}
			panel := pickerContained(t, tt, "at rest", heading, ui.Rect{W: 960, H: 520}, anthropic...)
			column, _ := tt.AccessByRole(ui.RoleListBox, heading)
			names, checked := effortNames(tt)
			if !slices.Equal(names, composer.Efforts) || checked != "medium" {
				t.Errorf("the column lists %q with %q checked, want %q with medium", names, checked, composer.Efforts)
			}
			model := modelRow(t, tt, "Claude Opus 5.5")
			if !model.Checked {
				t.Errorf("the chosen model's row is not selected: %+v", model)
			}
			if model.Bounds.W != 236 {
				t.Errorf("the model's row is %v wide, want 236", model.Bounds.W)
			}
			// Rows as the models' are, right of them, inside the panel.
			list := model.Bounds.X + model.Bounds.W
			for _, row := range effortRows(tt) {
				if !inside(row.Bounds, panel) || !inside(row.Bounds, column.Bounds) || row.Bounds.X < list || row.Bounds.H != model.Bounds.H {
					t.Errorf("the effort row %q is %v: the model's row %v, the column %v, the panel %v", row.Name, row.Bounds, model.Bounds, column.Bounds, panel)
				}
			}
			// One hairline between the models and the efforts, down the
			// panel: read above and below the rows.
			for _, y := range []float32{column.Bounds.Y + 4, panel.Y + panel.H - 24} {
				ground, line := p.at(list+1, y), float32(0)
				for x := list + 1; x < column.Bounds.X-1; x += 0.5 {
					if !same(p.at(x, y), ground) {
						line += 0.5
					}
				}
				if line < 0.5 || line > 2 {
					t.Errorf("the line between the models and the efforts is %v DIPs across at y %v, want a hairline", line, y)
				}
			}
			// Nothing moves once the panel is at rest.
			atRest(t, tt, calm, "at rest")
		})
	}

	// The efforts are a column of the panel, not a menu over it: resting on
	// a model with efforts opens nothing.
	t.Run("no effort popup", func(t *testing.T) {
		tt, _, calm := modelPickerAt(t, 960, nil)
		point(tt, modelRow(t, tt, "Claude Sonnet 5.5"))
		tt.Advance(time.Second)
		tt.Frame()
		if panels, menus := popups(tt); len(panels) != 1 || menus != 0 {
			t.Errorf("%d popovers and %d menu elements over a model with efforts, want the one panel", len(panels), menus)
		}
		if names, _ := effortNames(tt); !slices.Equal(names, composer.Efforts) {
			t.Errorf("the column lists %q", names)
		}
		atRest(t, tt, calm, "with the pointer at rest on a model")
	})

	// The column follows the pointer as it moves on a model's row, and
	// checks the effort the row would choose: the one chosen now where the
	// model has it, else the model's default.
	t.Run("pointer on another model", func(t *testing.T) {
		tt, comp, _ := modelPickerAt(t, 960, nil)
		pressButton(t, tt, zh("provider_gemini"))
		rest(tt)
		// The pointer is on the rail: the column is still the chosen model's.
		if names, checked := effortNames(tt); !slices.Equal(names, composer.Efforts) || checked != "medium" {
			t.Errorf("before the pointer is on a model the column lists %q with %q checked", names, checked)
		}
		gemini := modelRow(t, tt, "Gemini 3 Pro")
		point(tt, gemini)
		if names, checked := effortNames(tt); !slices.Equal(names, []string{"low", "high"}) || checked != "high" {
			t.Fatalf("on Gemini 3 Pro the column lists %q with %q checked, want low and high with its default", names, checked)
		}
		// The row chooses the model at that effort.
		tt.ClickAt(gemini.Bounds.X+20, gemini.Bounds.Y+gemini.Bounds.H/2)
		rest(tt)
		if comp.Model != "gemini-3-pro" || comp.Effort != 1 {
			t.Fatalf("the row chose %q at effort %v, want gemini-3-pro at high", comp.Model, comp.Effort)
		}
		// high is one of Claude Opus 5.5's: its row keeps it.
		pressButton(t, tt, "Gemini 3 Pro")
		rest(tt)
		pressButton(t, tt, zh("provider_anthropic"))
		rest(tt)
		opus := modelRow(t, tt, "Claude Opus 5.5")
		point(tt, opus)
		if _, checked := effortNames(tt); checked != "high" {
			t.Errorf("on Claude Opus 5.5 the column checks %q, want the effort chosen now", checked)
		}
		tt.ClickAt(opus.Bounds.X+20, opus.Bounds.Y+opus.Bounds.H/2)
		rest(tt)
		if comp.Model != "claude-opus-5-5" || composer.Efforts[int(comp.Effort)] != "high" {
			t.Errorf("the row chose %q at effort %v, want claude-opus-5-5 at high", comp.Model, comp.Effort)
		}
	})

	// One move onto a model's row gives it the column. A pointer at rest
	// takes nothing: not from the keys, and not for a row the list brings
	// under it. On its way to the column, more to the right than up or down,
	// it crosses the rows between without giving them the column.
	t.Run("pointer at rest", func(t *testing.T) {
		short := []string{"low", "high"}
		tt, _, _ := modelPickerAt(t, 960, func(_ *composer.State, ctx *composer.Context) {
			ctx.Models = slices.Clone(ctx.Models)
			for i := range ctx.Models {
				if ctx.Models[i].ID == "claude-sonnet-5-5" {
					ctx.Models[i].Efforts = short
				}
			}
		})
		column := func(when string, want []string) {
			t.Helper()
			if names, _ := effortNames(tt); !slices.Equal(names, want) {
				t.Fatalf("%s the column lists %q, want %q", when, names, want)
			}
		}
		key := func(k ui.Key) {
			tt.Key(0, k)
			tt.Frame()
		}
		// The list opened under the pointer, which has not moved.
		column("as the picker opens", composer.Efforts)
		sonnet := modelRow(t, tt, "Claude Sonnet 5.5").Bounds
		x, y := sonnet.X+20, sonnet.Y+sonnet.H/2
		tt.Move(x, y)
		tt.Frame()
		column("after one move onto Claude Sonnet 5.5", short)
		// The keys take the column from under the pointer.
		key(ui.KeyDown) // from Claude Opus 5.5, which has the focus
		key(ui.KeyDown)
		column("with the focus on Claude Haiku 5.5", nil)
		tt.Move(x+1, y)
		tt.Frame()
		column("after a move on Claude Sonnet 5.5", short)
		// The group folds and unfolds: the row is under the pointer again.
		key(ui.KeyUp)
		key(ui.KeyUp)
		column("with the focus on Claude Opus 5.5", composer.Efforts)
		key(ui.KeyUp) // the group's name
		key(ui.KeyEnter)
		if _, ok := tt.AccessByRole(ui.RoleButton, "Claude Sonnet 5.5"); ok {
			t.Fatalf("Enter on the group's name left its models shown: %q", tt.Texts())
		}
		key(ui.KeyEnter)
		if row := modelRow(t, tt, "Claude Sonnet 5.5").Bounds; row != sonnet {
			t.Fatalf("the row is %v after the group opened again, it was %v", row, sonnet)
		}
		column("with a row brought under the pointer", composer.Efforts)
		tt.Move(x+2, y)
		tt.Frame()
		column("after a move on that row", short)
		// Towards the column, over the next model's row.
		tt.Move(x+42, y+20)
		tt.Frame()
		column("on the way to the column over Claude Haiku 5.5", short)
		tt.Move(x+42, y+30)
		tt.Frame()
		column("after a move down Claude Haiku 5.5", nil)
	})

	// A model without efforts keeps the column, a muted line in the rows'
	// place: the panel is as wide as before.
	t.Run("model without efforts", func(t *testing.T) {
		tt, _, _ := modelPickerAt(t, 960, nil)
		before, _ := popups(tt)
		column, _ := tt.AccessByRole(ui.RoleListBox, heading)
		if tt.HasText(none) {
			t.Fatalf("the line of a model without efforts shows for Claude Opus 5.5: %q", tt.Texts())
		}
		point(tt, modelRow(t, tt, "Claude Haiku 5.5"))
		rest(tt)
		after, _ := popups(tt)
		if len(before) != 1 || len(after) != 1 || after[0].Bounds != before[0].Bounds {
			t.Fatalf("the panel was %+v and is %+v over a model without efforts", before, after)
		}
		if now, ok := tt.AccessByRole(ui.RoleListBox, heading); !ok || now.Bounds != column.Bounds {
			t.Errorf("the effort column was %v and is %v (shown %v)", column.Bounds, now.Bounds, ok)
		}
		if names, _ := effortNames(tt); len(names) != 0 {
			t.Errorf("a model without efforts lists %q", names)
		}
		line, ok := tt.Find(none)
		if !ok || !inside(line, column.Bounds) {
			t.Errorf("the line of a model without efforts is %v (shown %v), the column %v", line, ok, column.Bounds)
		}
	})

	// An effort's row chooses the column's model at that effort, as the
	// efforts' menu did: the panel stays open, back on its controls.
	t.Run("choosing an effort", func(t *testing.T) {
		tt, comp, _ := modelPickerAt(t, 960, nil)
		point(tt, modelRow(t, tt, "Claude Sonnet 5.5"))
		pressEffort(t, tt, "high")
		if comp.Model != "claude-sonnet-5-5" || composer.Efforts[int(comp.Effort)] != "high" {
			t.Fatalf("the effort's row chose %q at effort %v, want claude-sonnet-5-5 at high", comp.Model, comp.Effort)
		}
		if panels, _ := popups(tt); len(panels) != 1 {
			t.Fatalf("%d popovers after choosing an effort, want the panel open", len(panels))
		}
		if _, ok := tt.AccessByRole(ui.RoleSlider, heading); !ok {
			t.Errorf("the panel is not back on its controls: %q", tt.Texts())
		}
		if _, ok := tt.AccessByRole(ui.RoleListBox, heading); ok {
			t.Error("the effort column shows on the controls")
		}
		// The chosen model's own column chooses among its efforts.
		pressButton(t, tt, "Claude Sonnet 5.5")
		rest(tt)
		if _, checked := effortNames(tt); checked != "high" {
			t.Errorf("the chosen model's column checks %q, want high", checked)
		}
		pressEffort(t, tt, "low")
		if comp.Model != "claude-sonnet-5-5" || composer.Efforts[int(comp.Effort)] != "low" {
			t.Errorf("the effort's row chose %q at effort %v, want claude-sonnet-5-5 at low", comp.Model, comp.Effort)
		}
	})

	// The score measured at an effort is on that effort's row, at its end;
	// the model's own is on the effort it was measured at.
	t.Run("effort scores", func(t *testing.T) {
		tt, _, _ := modelPickerAt(t, 960, func(_ *composer.State, ctx *composer.Context) {
			ctx.Models = slices.Clone(ctx.Models)
			for i := range ctx.Models {
				if ctx.Models[i].ID == "claude-opus-5-5" {
					ctx.Models[i].Scores = map[string]*composer.Score{"low": {Overall: 7.3, Tested: "low"}}
				}
			}
		})
		low, ok := tt.AccessByRole(ui.RoleOption, "low")
		if !ok {
			t.Fatalf("no effort row low: %q", tt.Texts())
		}
		score, ok := tt.Find("7.3")
		if !ok || !inside(score, low.Bounds) || score.X < low.Bounds.X+low.Bounds.W/2 {
			t.Errorf("the score measured at low is %v (shown %v), its row %v", score, ok, low.Bounds)
		}
		// 9.1, measured at high: on the model's row and on high's.
		shown := 0
		for _, text := range tt.Texts() {
			if text == "9.1" {
				shown++
			}
		}
		if shown != 2 {
			t.Errorf("the model's score shows %d times, want its row and the effort it was measured at: %q", shown, tt.Texts())
		}
	})

	// Up and Down move in a column; Right or Tab go from the models to the
	// efforts, Left or Shift+Tab back; Enter chooses and Escape closes.
	t.Run("keys", func(t *testing.T) {
		tt, comp, _ := modelPickerAt(t, 960, nil)
		key := func(mods ui.Modifiers, k ui.Key, role ui.Role, name string) {
			t.Helper()
			tt.Key(mods, k)
			tt.Frame()
			if r, n := focusOf(tt); r != role || n != name {
				t.Fatalf("after key %v with modifiers %v the focus is on %q (role %v), want %q", k, mods, n, r, name)
			}
		}
		if _, name := focusOf(tt); name != "Claude Opus 5.5" {
			t.Fatalf("the picker opens with the focus on %q, want the chosen model", name)
		}
		key(0, ui.KeyRight, ui.RoleOption, "medium")
		key(0, ui.KeyDown, ui.RoleOption, "high")
		key(0, ui.KeyLeft, ui.RoleButton, "Claude Opus 5.5")
		key(0, ui.KeyTab, ui.RoleOption, "medium")
		key(ui.Shift, ui.KeyTab, ui.RoleButton, "Claude Opus 5.5")
		// The column follows the focus down the models.
		key(0, ui.KeyDown, ui.RoleButton, "Claude Sonnet 5.5")
		key(0, ui.KeyDown, ui.RoleButton, "Claude Haiku 5.5")
		if names, _ := effortNames(tt); len(names) != 0 || !tt.HasText(none) {
			t.Errorf("with the focus on a model without efforts the column lists %q: %q", names, tt.Texts())
		}
		// No efforts to go to.
		key(0, ui.KeyRight, ui.RoleButton, "Claude Haiku 5.5")
		key(0, ui.KeyUp, ui.RoleButton, "Claude Sonnet 5.5")
		if names, checked := effortNames(tt); !slices.Equal(names, composer.Efforts) || checked != "medium" {
			t.Errorf("with the focus on Claude Sonnet 5.5 the column lists %q with %q checked", names, checked)
		}
		key(0, ui.KeyRight, ui.RoleOption, "medium")
		key(0, ui.KeyDown, ui.RoleOption, "high")
		tt.Key(0, ui.KeyEnter)
		rest(tt)
		if comp.Model != "claude-sonnet-5-5" || composer.Efforts[int(comp.Effort)] != "high" {
			t.Fatalf("Enter chose %q at effort %v, want claude-sonnet-5-5 at high", comp.Model, comp.Effort)
		}
		if _, ok := tt.AccessByRole(ui.RoleSlider, heading); !ok {
			t.Errorf("the panel is not back on its controls after Enter: %q", tt.Texts())
		}
		tt.Key(0, ui.KeyEscape)
		rest(tt)
		if panels, _ := popups(tt); len(panels) != 0 {
			t.Errorf("Escape left %d popovers open", len(panels))
		}
	})

	// A window too narrow for the three columns keeps the rail and the
	// efforts and narrows the models between them: the panel stays in the
	// window.
	t.Run("narrow window", func(t *testing.T) {
		tt, _, _ := modelPickerAt(t, 440, nil)
		panel := pickerContained(t, tt, "at rest", heading, ui.Rect{W: 440, H: 520}, anthropic...)
		column, ok := tt.AccessByRole(ui.RoleListBox, heading)
		if !ok || column.Bounds.W != 132 || !inside(column.Bounds, panel) {
			t.Errorf("the effort column is %v (shown %v), the panel %v: want 132 wide inside it", column.Bounds, ok, panel)
		}
		model := modelRow(t, tt, "Claude Opus 5.5")
		if model.Bounds.W < 80 || model.Bounds.X+model.Bounds.W > column.Bounds.X {
			t.Errorf("the model's row is %v, the effort column %v", model.Bounds, column.Bounds)
		}
		if names, _ := effortNames(tt); !slices.Equal(names, composer.Efforts) {
			t.Errorf("the column lists %q", names)
		}
	})

	// A group's name folds and unfolds its models, by the keys and by a
	// click, and tells assistive technology which: with one provider too,
	// where the list opened its first group again in the frame after.
	t.Run("group folds", func(t *testing.T) {
		tt := demoPicker(t, nil)
		models := []string{preview.DemoModel, "DeepSeek V4.1", "DeepSeek V4.1 Flash"}
		state := func(when string, open bool) {
			t.Helper()
			shown := 0
			for _, name := range models {
				if _, ok := tt.AccessByRole(ui.RoleButton, name); ok {
					shown++
				}
			}
			want := 0
			if open {
				want = len(models)
			}
			if header := groupHeader(t, tt, preview.DemoProvider); header.Expanded != open || shown != want {
				t.Fatalf("%s the group is expanded %v with %d model rows, want expanded %v with %d: %q", when, header.Expanded, shown, open, want, tt.Texts())
			}
		}
		state("as the picker opens", true)
		// Up from the chosen model's row is the group's name.
		tt.Key(0, ui.KeyUp)
		tt.Frame()
		if role, name := focusOf(tt); role != ui.RoleDisclosure || name != preview.DemoProvider {
			t.Fatalf("Up from the first model is on %q (role %v), want the group's name", name, role)
		}
		tt.Key(0, ui.KeySpace)
		rest(tt)
		state("after Space on the group's name", false)
		if role, name := focusOf(tt); role != ui.RoleDisclosure || name != preview.DemoProvider {
			t.Errorf("the folded group left the focus on %q (role %v)", name, role)
		}
		tt.Key(0, ui.KeyEnter)
		rest(tt)
		state("after Enter on the group's name", true)
		clickRest(tt, groupHeader(t, tt, preview.DemoProvider))
		state("after a click on the group's name", false)
		// The fold holds while the picker shows.
		tt.Advance(time.Second)
		tt.Frame()
		state("a second after the click", false)
		clickRest(tt, groupHeader(t, tt, preview.DemoProvider))
		state("after a second click", true)
	})

	// The rail is Back and a tile for each family, 32 square, 4 apart, 6
	// from the rail's sides: no capability filters and no line under the
	// families (docs/removed-ui.md).
	t.Run("rail", func(t *testing.T) {
		tt := demoPicker(t, nil)
		tt.Move(2, 2)
		rest(tt)
		panels, _ := popups(tt)
		if len(panels) != 1 {
			t.Fatalf("%d popovers, want the one panel", len(panels))
		}
		panel := panels[0].Bounds
		name := zh("provider_deepseek")
		back, tile := railTile(t, tt, zh("composer_model_settings")), railTile(t, tt, name)
		for _, b := range []ui.AccessInfo{back, tile} {
			if b.Bounds.W != 32 || b.Bounds.H != 32 {
				t.Errorf("the rail's %q is %v, want 32 square", b.Name, b.Bounds)
			}
		}
		if !near(tile.Bounds.X, back.Bounds.X) || !near(tile.Bounds.Y, back.Bounds.Y+32+4) {
			t.Errorf("the family's tile is %v under Back %v, want 4 below it", tile.Bounds, back.Bounds)
		}
		if !tile.Checked || tile.Description != name {
			t.Errorf("the chosen family's tile is selected %v with the tooltip %q, want it selected and named %q", tile.Checked, tile.Description, name)
		}
		// The popover's padding and edge, then 6; and 6, the gap, the line
		// and the gap before the models.
		row := modelRow(t, tt, preview.DemoModel)
		if left, right := tile.Bounds.X-panel.X, row.Bounds.X-(tile.Bounds.X+tile.Bounds.W); !near(left, 12+1+6) || !near(right, 6+8+1+8) {
			t.Errorf("the tile is %v from the panel's side and %v before the models, want 19 and 23: tile %v, panel %v, row %v", left, right, tile.Bounds, panel, row.Bounds)
		}
		buttons := 0
		for _, e := range tt.AccessAll() {
			if e.Role != ui.RoleButton || !inside(e.Bounds, panel) {
				continue
			}
			if e.Name == zh("provider_cap_vision") || e.Name == zh("provider_cap_image") {
				t.Errorf("the rail has a capability filter %q", e.Name)
			}
			if e.Bounds.X+e.Bounds.W <= tile.Bounds.X+tile.Bounds.W+0.5 {
				buttons++
			}
		}
		if buttons != 2 {
			t.Errorf("the rail has %d buttons, want Back and the one family", buttons)
		}
		// Nothing is drawn under the last tile: the rail's fill alone.
		p := probe{t, tt}
		x, under := tile.Bounds.X+16, tile.Bounds.Y+32
		ground := p.at(x, under+1)
		for y := under + 1; y < under+16; y += 0.5 {
			if got := p.at(x, y); !same(got, ground) {
				t.Errorf("the rail draws %v at %v under its last tile, its fill is %v: want no line", got, y-under, ground)
				break
			}
		}
	})

	// A family's tile shows that family's groups alone and is the selected
	// one; a group folded stays folded while the rail is on another family,
	// and a family not yet shown opens its first group.
	t.Run("family filter", func(t *testing.T) {
		tt, _, _ := modelPickerAt(t, 960, nil)
		listed := func(when string, want map[string]bool) {
			t.Helper()
			for name, shown := range want {
				if _, ok := tt.AccessByRole(ui.RoleButton, name); ok != shown {
					t.Fatalf("%s the model %q is shown %v, want %v: %q", when, name, ok, shown, tt.Texts())
				}
			}
		}
		selected := func(when, want string) {
			t.Helper()
			for _, key := range []string{"provider_anthropic", "provider_openai", "provider_gemini", "provider_other"} {
				if tile := railTile(t, tt, zh(key)); tile.Checked != (key == want) {
					t.Errorf("%s the tile %q is selected %v", when, tile.Name, tile.Checked)
				}
			}
		}
		listed("as the picker opens", map[string]bool{"Claude Opus 5.5": true, "Claude Haiku 5.5": true, "GPT-5.2": false, "Gemini 3 Pro": false, "Qwen3 Coder": false})
		selected("as the picker opens", "provider_anthropic")
		clickRest(tt, railTile(t, tt, zh("provider_openai")))
		listed("on OpenAI", map[string]bool{"Claude Opus 5.5": false, "Claude Haiku 5.5": false, "GPT-5.2": true, "Gemini 3 Pro": false})
		selected("on OpenAI", "provider_openai")
		clickRest(tt, groupHeader(t, tt, "OpenAI"))
		listed("with OpenAI folded", map[string]bool{"GPT-5.2": false})
		clickRest(tt, railTile(t, tt, zh("provider_anthropic")))
		listed("back on Anthropic", map[string]bool{"Claude Opus 5.5": true, "Claude Sonnet 5.5": true, "GPT-5.2": false})
		clickRest(tt, railTile(t, tt, zh("provider_openai")))
		listed("back on OpenAI", map[string]bool{"Claude Opus 5.5": false, "GPT-5.2": false})
		if header := groupHeader(t, tt, "OpenAI"); header.Expanded {
			t.Errorf("the folded group is expanded after the rail came back to its family")
		}
		clickRest(tt, railTile(t, tt, zh("provider_gemini")))
		listed("on Gemini", map[string]bool{"Gemini 3 Pro": true, "GPT-5.2": false})
		selected("on Gemini", "provider_gemini")
	})

	// What is chosen is told by its fill alone, one neutral gray on the
	// model's row, the effort's row and the family's tile, stronger than
	// the fill of a row under the pointer; no check is drawn, and assistive
	// technology has each as selected.
	for _, mode := range []string{"dark", "light"} {
		t.Run("selection "+mode, func(t *testing.T) {
			tt, _, _ := modelPickerAt(t, 960, nil)
			tt.SetDark(mode == "dark")
			tt.Move(2, 2)
			rest(tt)
			p := probe{t, tt}
			model, effort := modelRow(t, tt, "Claude Opus 5.5"), effortRow(t, tt, "medium")
			tile, other := railTile(t, tt, zh("provider_anthropic")), railTile(t, tt, zh("provider_openai"))
			if !model.Checked || !effort.Selected || !tile.Checked || other.Checked {
				t.Errorf("selected: the model's row %v, the effort's row %v, the family's tile %v, another family's %v", model.Checked, effort.Selected, tile.Checked, other.Checked)
			}
			low := effortRow(t, tt, "low")
			ground := p.edge(modelRow(t, tt, "Claude Haiku 5.5"))
			if got := p.edge(low); !same(got, ground) {
				t.Fatalf("rows at rest are filled %v and %v, want neither filled", ground, got)
			}
			// gray is how far a fill is from what is under it, which every
			// channel is alike for a neutral one.
			gray := func(what string, fill, under [3]int) int {
				t.Helper()
				d := [3]int{fill[0] - under[0], fill[1] - under[1], fill[2] - under[2]}
				far := max(d[0], -d[0])
				if max(d[0], d[1], d[2])-min(d[0], d[1], d[2]) > 3 || far < 6 {
					t.Errorf("%s is filled %v over %v, want a neutral gray", what, fill, under)
				}
				return far
			}
			chosen := gray("the chosen model's row", p.edge(model), ground)
			gray("the chosen effort's row", p.edge(effort), ground)
			if !same(p.edge(model), p.edge(effort)) {
				t.Errorf("the chosen model's row is filled %v, the chosen effort's %v: want one fill", p.edge(model), p.edge(effort))
			}
			at := func(e ui.AccessInfo) [3]int { return p.at(e.Bounds.X+3, e.Bounds.Y+e.Bounds.H/2) }
			gray("the chosen family's tile", at(tile), at(other))
			// No check: the end of the chosen effort's row, which has no
			// score, is its fill.
			fill, b := p.edge(effort), effort.Bounds
		scan:
			for x := b.X + b.W - 28; x <= b.X+b.W-4; x += 0.5 {
				for y := b.Y + 10; y <= b.Y+22; y += 0.5 {
					if got := p.at(x, y); !same(got, fill) {
						t.Errorf("the chosen effort's row draws %v at its end, %v from its start, over its fill %v: want no check", got, x-b.X, fill)
						break scan
					}
				}
			}
			// The row under the pointer is filled, fainter than the chosen.
			p.hover(low)
			if hovered := gray("the effort's row under the pointer", p.edge(low), ground); hovered >= chosen {
				t.Errorf("the row under the pointer is %d from the panel and the chosen row %d: want the chosen one stronger", hovered, chosen)
			}
		})
	}

	// A score is the last thing of its row, ending at the row's padding, on
	// the models' rows and the efforts' alike, so the scores are a column;
	// a model without a score keeps the score's place, so its capability
	// mark is under the others'.
	t.Run("scores at the row's end", func(t *testing.T) {
		vision, image := zh("provider_cap_vision"), zh("provider_cap_image")
		tt := demoPicker(t, func(models []composer.Model) {
			// Pro: the image mark, its score, and scores measured at two
			// efforts. V4.1: its score alone. Flash: the vision mark alone.
			pro, plain, flash := &models[0], &models[1], &models[2]
			flash.Capabilities = pro.Capabilities[:1]
			pro.Capabilities, plain.Capabilities = pro.Capabilities[1:], nil
			pro.Scores = map[string]*composer.Score{"low": {Overall: 7.3, Tested: "low"}, "high": {Overall: 9.4, Tested: "high"}}
		})
		tt.Move(2, 2)
		rest(tt)
		// ends is how far before its row's end the element showing or
		// named text ends.
		ends := func(text string, row ui.AccessInfo) float32 {
			t.Helper()
			box, ok := tt.Find(text)
			if !ok || !inside(box, row.Bounds) {
				t.Fatalf("%q is %v (shown %v), not in the row %q %v: %q", text, box, ok, row.Name, row.Bounds, tt.Texts())
			}
			return row.Bounds.X + row.Bounds.W - (box.X + box.W)
		}
		pro, plain, flash := modelRow(t, tt, preview.DemoModel), modelRow(t, tt, "DeepSeek V4.1"), modelRow(t, tt, "DeepSeek V4.1 Flash")
		low, high := effortRow(t, tt, "low"), effortRow(t, tt, "high")
		if !near(low.Bounds.X+low.Bounds.W, high.Bounds.X+high.Bounds.W) || !near(pro.Bounds.X+pro.Bounds.W, plain.Bounds.X+plain.Bounds.W) {
			t.Fatalf("the rows of a column end apart: the efforts %v and %v, the models %v and %v", low.Bounds, high.Bounds, pro.Bounds, plain.Bounds)
		}
		// Each column's scores end at one place, the rows' padding.
		for _, column := range [][2]float32{{ends("7.3", low), ends("9.4", high)}, {ends("9.2", pro), ends("8.7", plain)}} {
			if !near(column[0], column[1]) || column[0] < 7 || column[0] > 9 {
				t.Errorf("a column's scores end %v and %v before the ends of their rows, want the padding, 8, for both", column[0], column[1])
			}
		}
		// The marks: before the score's place, 8 apart from it.
		scored, unscored := ends(image, pro), ends(vision, flash)
		if !near(scored, unscored) || scored < 8+28+8-1 || scored > 8+28+8+1 {
			t.Errorf("the capability marks end %v before the end of a row with a score and %v of one without, want 44 for both", scored, unscored)
		}
	})

	// A score is text alone: the pointer at rest on one, a model's or an
	// effort's, opens no tooltip and no card (docs/removed-ui.md).
	t.Run("scores open nothing", func(t *testing.T) {
		tt, _, _ := modelPickerAt(t, 960, nil)
		still := func(what string, x, y float32) {
			t.Helper()
			if restTip(tt, x, y) || tt.HasText(zh("model_score_overall")) {
				t.Errorf("the pointer at rest on %s opens a tooltip: %q", what, tt.Texts())
			}
			if panels, menus := popups(tt); len(panels) != 1 || menus != 0 {
				t.Errorf("%d popovers and %d menu elements with the pointer on %s, want the one panel", len(panels), menus, what)
			}
		}
		// Claude Opus 5.5's score was measured at high: it is at that
		// row's end.
		high := effortRow(t, tt, "high").Bounds
		still("an effort's score", high.X+high.W-16, high.Y+high.H/2)
		score, ok := tt.Find("8.6")
		if !ok || !inside(score, modelRow(t, tt, "Claude Sonnet 5.5").Bounds) {
			t.Fatalf("the score of Claude Sonnet 5.5 is %v (shown %v): %q", score, ok, tt.Texts())
		}
		still("a model's score", score.X+score.W/2, score.Y+score.H/2)
	})
}

// pickerInShell is the model picker in the app's window, at its first size
// and its smallest, drawn at one and two pixels a DIP: in the first frame
// of the picker's page, with no time passed, and at rest every row is whole
// in the one popover, the models' list at its width; an effort's row of a
// model the pointer moved onto chooses that model at that effort and leaves
// the popover open on its controls; and Back gives the popover its size
// again. With SAILRY_SNAPSHOTS
// set the picker is saved, dark and light, under .runtime/ui-shots.
func pickerInShell(t *testing.T) {
	heading := i18n.In("en", "composer_effort")
	models := []string{"Claude Opus 5.5", "Claude Sonnet 5.5", "Claude Haiku 5.5"}
	for _, size := range [][2]int{{1280, 820}, {1280, 720}, {760, 820}, {760, 560}} {
		for _, scale := range []float32{1, 2} {
			t.Run(fmt.Sprintf("%dx%d at %v", size[0], size[1], scale), func(t *testing.T) {
				a := New(preview.Load())
				tt := ui.NewTesterWith(a.View, size[0], size[1], opaque)
				tt.SetLocale("en")
				tt.SetScale(scale)
				tt.Frame()
				comp := a.Composer(preview.Big)
				window := ui.Rect{W: float32(size[0]), H: float32(size[1])}
				openControls(t, tt, i18n.In("en", "composer_model_settings")+": ", i18n.In("en", "composer_settings"))
				controls, _ := popups(tt)
				if len(controls) != 1 {
					t.Fatalf("%d popovers for the model controls: %q", len(controls), tt.Texts())
				}
				clickButton(t, tt, "Claude Opus 5.5") // the controls' model, which opens the picker
				pickerContained(t, tt, "as the picker opens", heading, window, models...)
				if row := modelRow(t, tt, "Claude Opus 5.5").Bounds; row.W != 236 {
					t.Errorf("as the picker opens the model's row is %v wide, want 236", row.W)
				}
				point(tt, modelRow(t, tt, "Claude Sonnet 5.5"))
				pickerContained(t, tt, "with the pointer on a model", heading, window, models...)
				pressEffort(t, tt, "low")
				if comp.Model != "claude-sonnet-5-5" || composer.Efforts[int(comp.Effort)] != "low" {
					t.Fatalf("the effort's row chose %q at effort %v, want claude-sonnet-5-5 at low", comp.Model, comp.Effort)
				}
				if panels, _ := popups(tt); len(panels) != 1 {
					t.Fatalf("%d popovers after choosing an effort, want the one open", len(panels))
				}
				if _, ok := tt.AccessByRole(ui.RoleSlider, heading); !ok {
					t.Fatalf("the popover is not back on its controls: %q", tt.Texts())
				}
				clickButton(t, tt, "Claude Sonnet 5.5")
				rest(tt)
				pickerContained(t, tt, "at rest", heading, window, models...)
				if names, checked := effortNames(tt); !slices.Equal(names, composer.Efforts) || checked != "low" {
					t.Errorf("the column lists %q with %q checked, want %q with low", names, checked, composer.Efforts)
				}
				if row := modelRow(t, tt, "Claude Opus 5.5").Bounds; row.W != 236 {
					t.Errorf("the model's row is %v wide, want 236", row.W)
				}
				if size == [2]int{1280, 820} && scale == 2 && os.Getenv("SAILRY_SNAPSHOTS") != "" {
					// The saved frames have the pointer on another model,
					// whose efforts the column lists.
					point(tt, modelRow(t, tt, "Claude Opus 5.5"))
					tt.SetLocale("zh-CN")
					shots := filepath.Join("..", "..", "..", ".runtime", "ui-shots")
					if err := os.MkdirAll(shots, 0o755); err != nil {
						t.Fatal(err)
					}
					for _, mode := range []string{"dark", "light"} {
						tt.SetDark(mode == "dark")
						rest(tt)
						file, err := os.Create(filepath.Join(shots, "composer-model-picker-zh-CN-"+mode+".png"))
						if err != nil {
							t.Fatal(err)
						}
						err = png.Encode(file, tt.Image())
						file.Close()
						if err != nil {
							t.Fatal(err)
						}
					}
					tt.SetLocale("en")
					rest(tt)
				}
				clickButton(t, tt, i18n.In("en", "composer_model_settings")) // Back
				rest(tt)
				after, _ := popups(tt)
				if len(after) != 1 || after[0].Bounds.W != controls[0].Bounds.W || after[0].Bounds.H != controls[0].Bounds.H {
					t.Errorf("the popover was %v on its controls and is %+v back on them", controls[0].Bounds, after)
				}
			})
		}
	}
}
