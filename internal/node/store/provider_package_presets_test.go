package store

import (
	"database/sql"
	"fmt"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestPackagePresetWarning lists a package's presets: the entry's
// login_warning is the warning of the preset's sign-in, else the preset's
// own, and the listed preset does not repeat it.
func TestPackagePresetWarning(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	flowWarning := &protocol.PluginModelAuthWarning{Text: "The service may suspend this account", Locales: map[string]string{"zh-CN": "该服务可能封禁此账号"}}
	presetWarning := &protocol.PluginModelAuthWarning{Text: "Heavy use may count against this account", Locales: map[string]string{"zh-CN": "大量使用可能计入此账号的额度"}}
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		info := pluginTestInfo("warning-fixture", 'a')
		extension := protocol.DefaultPluginExtension()
		extension.ApiVersion = "v1"
		extension.Actions = []protocol.PluginAction{protocol.PluginActionProvideModels, protocol.PluginActionAuthenticateModels, protocol.PluginActionInspectCredentials}
		flow := func(id string, warning *protocol.PluginModelAuthWarning) protocol.PluginModelAuthDeclaration {
			declaration := protocol.DefaultPluginModelAuthDeclaration()
			declaration.ID, declaration.Display.Label, declaration.Warning = id, id, warning
			declaration.Flow = protocol.PluginModelAuthFlow{Kind: "script", Data: protocol.PluginModelAuthFlowScriptPayload{
				Script: protocol.PluginModelAdapterScript{Entry: "auth.js", Resources: []string{}}, Handler: "step", Requests: map[string]protocol.PluginModelAuthEndpoint{},
			}}
			return declaration
		}
		extension.ModelAuth = []protocol.PluginModelAuthDeclaration{flow("account", flowWarning), flow("quiet", nil)}
		preset := func(id, sign string, warning *protocol.PluginModelAuthWarning) protocol.ConversationPresetsPreset {
			value := protocol.DefaultConversationPresetsPreset()
			value.ID, value.Name, value.Offered, value.Warning = id, id, true, warning
			value.Routes = []protocol.ConversationPresetsRoute{{Protocol: protocol.ConversationPresetsProtocolPlugin, Endpoint: "https://example.test"}}
			value.Auth = protocol.ConversationPresetsAuth{Kind: "none"}
			if sign != "" {
				value.Auth = protocol.ConversationPresetsAuth{Kind: "plugin", Data: protocol.ConversationPresetsAuthPluginPayload{Flow: sign}}
			}
			return value
		}
		extension.ModelProviders = []protocol.ConversationPresetsPreset{
			preset("local", "", presetWarning), preset("plain", "", nil),
			preset("signed", "account", presetWarning), preset("fallback", "quiet", presetWarning), preset("silent", "quiet", nil),
		}
		info.Extension = &extension
		if _, err := InstallPlugin(ctx, db, info, 0, true); err != nil {
			return err
		}
		listing, err := appendPackagePresets(ctx, db, protocol.DefaultConversationPresetsListing())
		if err != nil {
			return err
		}
		want := map[string]struct {
			warning *protocol.PluginModelAuthWarning
			login   bool
		}{
			"local": {presetWarning, false}, "plain": {nil, false},
			"signed": {flowWarning, true}, "fallback": {presetWarning, true}, "silent": {nil, true},
		}
		if len(listing.Entries) != len(want) {
			return fmt.Errorf("listed %d entries", len(listing.Entries))
		}
		for _, entry := range listing.Entries {
			expected, known := want[entry.Preset.ID]
			if !known || !reflect.DeepEqual(entry.LoginWarning, expected.warning) || (entry.Login != nil) != expected.login {
				return fmt.Errorf("entry %q: warning %+v, sign-in %v", entry.Preset.ID, entry.LoginWarning, entry.Login != nil)
			}
			if entry.Preset.Warning != nil {
				return fmt.Errorf("entry %q repeats the warning in its preset", entry.Preset.ID)
			}
		}
		// The declaration itself keeps its warning for the next listing.
		stored, err := ReadPlugin(ctx, db, "warning-fixture")
		if err != nil || stored == nil || !reflect.DeepEqual(stored.Extension.ModelProviders[0].Warning, presetWarning) {
			return fmt.Errorf("stored declaration lost its warning: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
