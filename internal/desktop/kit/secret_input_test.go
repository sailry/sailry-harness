package kit

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestSecretInputReveal(t *testing.T) {
	for _, sample := range []struct {
		name, locale string
		mode         theme.Mode
		width        float32
	}{
		{"light-English", "en", theme.Light, 300},
		{"dark-Chinese", "zh-CN", theme.Dark, 220},
	} {
		t.Run(sample.name, func(t *testing.T) {
			value, shown := "sk-test-credential", false
			edited, submitted := false, false
			tt := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", sample.mode)
				ui.Column(c).Fill().Padding(20).Children(func() {
					ui.Column(c).Width(sample.width).Children(func() {
						input := SecretInput(c.Key("credential"), &value, &shown, SecretInputProps{
							Label: "Credential", Placeholder: "Enter credential",
						})
						edited = input.Changed() || edited
						submitted = input.Submitted() || submitted
					})
				})
			}, 380, 160)
			tt.SetLocale(sample.locale)
			tt.Frame()
			field := secretInputAccess(t, tt, ui.RoleTextField, "Credential")
			if !field.Password || field.Value != "" || field.Placeholder != "Enter credential" || tt.HasText(value) {
				t.Fatalf("credential is not masked by default: %+v", field)
			}
			show, hide := i18n.In(sample.locale, "mcp_show_secret"), i18n.In(sample.locale, "mcp_hide_secret")
			eye := secretInputAccess(t, tt, ui.RoleButton, show)
			secretInputEyeInside(t, field.Bounds, eye.Bounds)

			// The returned element keeps native edits and submission while masked.
			tt.ClickAt(field.Bounds.X+12, field.Bounds.Y+field.Bounds.H/2)
			tt.Command("selectAll")
			tt.Type("replacement-credential")
			tt.Key(0, ui.KeyEnter)
			tt.Frame()
			field = secretInputAccess(t, tt, ui.RoleTextField, "Credential")
			if value != "replacement-credential" || !edited || !submitted || !field.Password || field.Value != "" || tt.HasText(value) {
				t.Fatalf("masked input lost native editing: value=%q edited=%v submitted=%v field=%+v", value, edited, submitted, field)
			}

			tt.ClickAt(eye.Bounds.X+eye.Bounds.W/2, eye.Bounds.Y+eye.Bounds.H/2)
			tt.Frame()
			field = secretInputAccess(t, tt, ui.RoleTextField, "Credential")
			if !shown || field.Password || field.Value != value || !field.Focused {
				t.Fatalf("reveal did not show the credential and preserve input focus: shown=%v field=%+v", shown, field)
			}
			eye = secretInputAccess(t, tt, ui.RoleButton, hide)
			secretInputEyeInside(t, field.Bounds, eye.Bounds)
			tt.ClickAt(eye.Bounds.X+eye.Bounds.W/2, eye.Bounds.Y+eye.Bounds.H/2)
			tt.Frame()
			field = secretInputAccess(t, tt, ui.RoleTextField, "Credential")
			if shown || !field.Password || field.Value != "" || tt.HasText(value) {
				t.Fatalf("hide left the credential exposed: shown=%v field=%+v", shown, field)
			}
			secretInputAccess(t, tt, ui.RoleButton, show)
		})
	}
}

func TestSecretInputDisabled(t *testing.T) {
	value, shown, disabled := "stored-credential", false, false
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Padding(20).Children(func() {
			SecretInput(c.Key("credential"), &value, &shown, SecretInputProps{
				Label: "Credential", Disabled: disabled,
			})
		})
	}, 380, 160)
	tt.SetLocale("en")
	tt.Frame()
	field := secretInputAccess(t, tt, ui.RoleTextField, "Credential")
	tt.ClickAt(field.Bounds.X+12, field.Bounds.Y+field.Bounds.H/2)
	disabled = true
	tt.Frame()
	field = secretInputAccess(t, tt, ui.RoleTextField, "Credential")
	eye := secretInputAccess(t, tt, ui.RoleButton, i18n.In("en", "mcp_show_secret"))
	if !field.Disabled || !eye.Disabled {
		t.Fatalf("disabled credential left an active control: field=%+v eye=%+v", field, eye)
	}
	tt.Type("blocked")
	tt.ClickAt(eye.Bounds.X+eye.Bounds.W/2, eye.Bounds.Y+eye.Bounds.H/2)
	tt.Frame()
	if value != "stored-credential" || shown {
		t.Fatalf("disabled credential changed: value=%q shown=%v", value, shown)
	}
}

func TestSecretInputEyeScrollsWithInput(t *testing.T) {
	value, shown := "stored-credential", false
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Scroll(c).Fill().Children(func() {
			ui.Column(c).Padding(20).Children(func() {
				SecretInput(c.Key("credential"), &value, &shown, SecretInputProps{Label: "Credential"})
				ui.Box(c).Height(600)
			})
		})
	}, 380, 160)
	tt.SetLocale("en")
	tt.Frame()
	show := i18n.In("en", "mcp_show_secret")
	field := secretInputAccess(t, tt, ui.RoleTextField, "Credential")
	eye := secretInputAccess(t, tt, ui.RoleButton, show)
	secretInputEyeInside(t, field.Bounds, eye.Bounds)
	tt.Scroll(100, 80, 0, 500)
	tt.Frame()
	field = secretInputAccess(t, tt, ui.RoleTextField, "Credential")
	if !field.Offscreen {
		t.Fatalf("fixture did not scroll the credential offscreen: %+v", field)
	}
	if eye, ok := tt.AccessByRole(ui.RoleButton, show); ok && !eye.Offscreen {
		t.Fatalf("credential eye stayed visible after its input scrolled away: %+v", eye)
	}
}

func secretInputAccess(t *testing.T, tt *ui.Tester, role ui.Role, name string) ui.AccessInfo {
	t.Helper()
	info, ok := tt.AccessByRole(role, name)
	if !ok {
		t.Fatalf("missing credential control %q: %+v", name, tt.AccessAll())
	}
	return info
}

func secretInputEyeInside(t *testing.T, input, eye ui.Rect) {
	t.Helper()
	if eye.W <= 0 || eye.H <= 0 || eye.X < input.X || eye.Y < input.Y || eye.X+eye.W > input.X+input.W || eye.Y+eye.H > input.Y+input.H {
		t.Fatalf("credential eye is outside its input: input=%+v eye=%+v", input, eye)
	}
	if gap := input.X + input.W - eye.X - eye.W; gap < 1 || gap > 8 {
		t.Fatalf("credential eye is not at the trailing edge: gap=%v input=%+v eye=%+v", gap, input, eye)
	}
}
