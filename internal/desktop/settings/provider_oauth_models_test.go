package settings

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestOAuthProviderDiscoversUsingEditedSettings(t *testing.T) {
	m := NewModel(preview.SettingsData())
	m.Live = true
	m.Catalog = preview.Catalog{}
	old := preview.ChannelModel{ID: "saved-model", Name: "Saved model"}
	m.Brands = []preview.Brand{{ID: "fixture", Methods: []preview.Method{{
		ID: "oauth", API: "responses", Auth: preview.AuthOAuth, Login: "provider_access_login", OAuth: [4]string{"0.160.1", "Sailry"},
	}}}}
	channel := preview.Channel{ID: "channel", Name: "OAuth Fixture", Auth: preview.AuthOAuth,
		Method: "oauth", API: "responses", Revision: 2, Models: []preview.ChannelModel{old}}
	editor := &m.provider
	editor.edit(m, &channel)
	editor.oauth = [4]string{"0.171.0", "Custom Client"}
	var queried *ProviderDraft
	m.ProviderActions.Discover = func(draft ProviderDraft, finish func([]preview.ChannelModel)) {
		queried = &draft
		finish([]preview.ChannelModel{{ID: "fresh-model", Name: "Fresh model"}})
	}
	tester := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		providerEditorDialog(c, m)
	}, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
	tester.SetLocale("en")
	tester.Frame()
	tester.Frame()
	label := i18n.In("en", "provider_discover")
	button, ok := tester.AccessByRole(ui.RoleButton, label)
	if !ok || button.Disabled {
		t.Fatalf("missing or disabled model update button %q", label)
	}
	tester.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
	tester.Advance(time.Second)
	tester.Frame()
	if queried == nil || queried.OAuth != editor.oauth || queried.ExpectedRevision != channel.Revision {
		t.Fatalf("model update did not carry the edited OAuth settings: %+v", queried)
	}
	if len(editor.models) != 2 || editor.models[0].id != old.ID || editor.models[1].id != "fresh-model" {
		t.Fatalf("model update did not merge the account response: %+v", editor.models)
	}
}
