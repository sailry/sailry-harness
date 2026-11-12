package composer_test

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// The composer has no file picker of its own: the attach button and the @
// picker's Attach files entry ask the window for its picker through
// AttachmentCallbacks.Pick, and are off without it or in a read-only
// conversation.
func TestAttachAsksTheWindowPicker(t *testing.T) {
	name := i18n.In("en", "composer_attachment")
	attach := composer.Reference{ID: "attach", Kind: composer.AttachFiles, Label: "composer_attachment", Key: true, Icon: icons.Plus}
	open := func(readonly bool, callbacks *composer.AttachmentCallbacks) (*ui.Tester, *composer.State) {
		state := composer.New(preview.DemoModel)
		state.SetAttachmentCallbacks(callbacks)
		tt := trayOf(state, &owner{}, "en", func() *composer.Context {
			return &composer.Context{Readonly: readonly, References: []composer.Reference{attach}, Activity: composer.Activity{Connected: true}}
		})
		return tt, state
	}
	button := func(tt *ui.Tester) ui.AccessInfo {
		t.Helper()
		info, ok := tt.AccessByRole(ui.RoleButton, name)
		if !ok {
			t.Fatalf("no attach button: %q", tt.Texts())
		}
		return info
	}
	press := func(tt *ui.Tester) {
		b := button(tt).Bounds
		tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
		tt.Frame()
		tt.Frame()
	}

	t.Run("the button asks once and builds no dialog", func(t *testing.T) {
		asked := 0
		tt, state := open(false, &composer.AttachmentCallbacks{Pick: func() { asked++ }})
		if button(tt).Disabled {
			t.Fatal("the attach button is disabled with a window picker")
		}
		press(tt)
		if asked != 1 || len(state.Attachments) != 0 {
			t.Fatalf("the button asked %d times and attached %d files", asked, len(state.Attachments))
		}
		for _, info := range tt.AccessAll() {
			if info.Role == ui.RoleDialog {
				t.Fatalf("the composer built a dialog of its own: %q", info.Name)
			}
		}
	})

	t.Run("the @ entry asks", func(t *testing.T) {
		asked := 0
		tt, state := open(false, &composer.AttachmentCallbacks{Pick: func() { asked++ }})
		if err := tt.Click(i18n.In("en", "composer")); err != nil {
			t.Fatal(err)
		}
		tt.Type("@")
		tt.Frame()
		if _, ok := tt.AccessByRole(ui.RoleOption, name); !ok {
			t.Fatalf("the @ picker has no Attach files entry: %q", tt.Texts())
		}
		tt.Key(0, ui.KeyEnter)
		tt.Frame()
		tt.Frame()
		if asked != 1 || state.Draft != "" {
			t.Fatalf("the entry asked %d times and left the draft %q", asked, state.Draft)
		}
	})

	t.Run("off without a window picker or in a read-only conversation", func(t *testing.T) {
		asked := 0
		for label, tt := range map[string]*ui.Tester{
			"no callbacks": first(open(false, nil)),
			"no picker":    first(open(false, &composer.AttachmentCallbacks{})),
			"read-only":    first(open(true, &composer.AttachmentCallbacks{Pick: func() { asked++ }})),
		} {
			if !button(tt).Disabled {
				t.Errorf("%s: the attach button is enabled", label)
			}
			press(tt)
		}
		if asked != 0 {
			t.Fatalf("a read-only conversation asked for the picker %d times", asked)
		}
	})
}

func first(tt *ui.Tester, _ *composer.State) *ui.Tester { return tt }
