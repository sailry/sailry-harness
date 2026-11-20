package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"reflect"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func configurationPluginInfo(t *testing.T, name string, digest byte) protocol.PluginInfo {
	t.Helper()
	info := pluginTestInfo(name, digest)
	schema, err := plugins.ValidateSettings([]byte(`{"$schema":"` + plugins.SettingsSchema + `","type":"object","additionalProperties":false,"properties":{"mode":{"type":"string","default":"fast"},"token":{"type":"string","x-sailry-secret":{"origin":"https://api.example.test","header":"Authorization"}}},"required":["mode"]}`))
	if err != nil {
		t.Fatal(err)
	}
	info.Settings = &schema
	return info
}

func configurationItems(report []protocol.ConfigurationImportItem) map[string]protocol.ConfigurationImportItem {
	items := map[string]protocol.ConfigurationImportItem{}
	for _, item := range report {
		items[item.Area+"/"+item.Name] = item
	}
	return items
}

func importConfigurationFixture(ctx context.Context, storage *Store, node protocol.NodeID, document json.RawMessage, preview bool) (ConfigurationImport, error) {
	parsed, err := ParseConfiguration(document)
	if err != nil {
		return ConfigurationImport{}, err
	}
	var result ConfigurationImport
	err = storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := db.ExecContext(ctx, "SAVEPOINT fixture_import"); err != nil {
			return err
		}
		var err error
		result, err = ImportConfiguration(ctx, db, node, parsed, ConfigurationTargets{Plugins: map[string]map[string]string{"shared": {}, "updated": {}}})
		if err != nil || preview {
			if _, rollback := db.ExecContext(ctx, "ROLLBACK TO fixture_import"); rollback != nil {
				return rollback
			}
		}
		if _, release := db.ExecContext(ctx, "RELEASE fixture_import"); release != nil {
			return release
		}
		return err
	})
	return result, err
}

func TestConfigurationTransfer(t *testing.T) {
	t.Parallel()
	_, sourceNode, source := schemaFixture(t)
	ctx := t.Context()
	keyed := providerFixture()
	keyed.Name = "Keyed"
	keyed.Models[0].Inputs = []string{"text", "image"}
	account := providerFixture()
	account.Name, account.Authentication = "Account", protocol.AuthenticationCopilot
	instructions := "Prefer short answers."
	role := protocol.DefaultRoleProfile()
	role.ID, role.Key, role.Name, role.Instructions = protocol.NewRoleID(), "reviewer", "Reviewer", "Review the change."
	team := protocol.DefaultTeamProfile()
	team.ID, team.Key, team.Name, team.Members = protocol.NewTeamID(), "pair", "Pair", []protocol.RoleID{role.ID}
	var document json.RawMessage
	if err := source.Node.Write(ctx, func(db *sql.Conn) error {
		key := protocol.NewSecret("provider-key")
		output, _, err := ExecuteProviderCommand(ctx, db, sourceNode, protocol.Request{Command: protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: keyed, Secret: &key}}}, nil)
		if err != nil {
			return fmt.Errorf("keyed provider: %w", err)
		}
		keyed = output.Data.(protocol.ConversationProvider)
		if _, _, err := ExecuteProviderCommand(ctx, db, sourceNode, protocol.Request{Command: protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: account}}}, nil); err != nil {
			return fmt.Errorf("account provider: %w", err)
		}
		search, err := ReadSearchSettings(ctx, db)
		if err != nil {
			return err
		}
		credentials := protocol.DefaultSearchCredentials()
		credentials.Key = protocol.PluginSettingsSecretUpdate{Kind: "replace", Data: protocol.NewSecret("search-key")}
		if _, _, err := SaveSearchSettings(ctx, db, protocol.Request{}, protocol.SearchUpdate{Settings: search, Credentials: map[protocol.SearchDriver]protocol.SearchCredentials{protocol.SearchDriverTavily: credentials}}); err != nil {
			return fmt.Errorf("search: %w", err)
		}
		if _, _, err := saveMediaSettings(ctx, db, protocol.MediaSettings{Bindings: map[protocol.MediaKind]*protocol.MediaBinding{protocol.MediaKindVision: {Provider: keyed.ID, Model: "fixture"}}}); err != nil {
			return fmt.Errorf("media: %w", err)
		}
		role.Model = &protocol.RoleModel{Provider: keyed.ID, Model: "fixture"}
		if _, err := putRole(ctx, db, role, 0); err != nil {
			return fmt.Errorf("role: %w", err)
		}
		if _, err := putTeam(ctx, db, team, 0); err != nil {
			return fmt.Errorf("team: %w", err)
		}
		if _, err := SaveSandboxSettings(ctx, db, protocol.SandboxSettings{Enabled: false}); err != nil {
			return err
		}
		for _, info := range []protocol.PluginInfo{configurationPluginInfo(t, "shared", 'a'), configurationPluginInfo(t, "updated", 'b'), configurationPluginInfo(t, "missing", 'c')} {
			info, err := InstallPlugin(ctx, db, info, 0, true)
			if err != nil {
				return err
			}
			if _, err := SavePluginSettings(ctx, db, protocol.CommandSavePluginSettingsPayload{Package: plugins.PackageReference(info), Values: map[string]json.RawMessage{"mode": json.RawMessage(`"careful"`)},
				Secrets: map[string]protocol.PluginSettingsSecretUpdate{"token": {Kind: "replace", Data: protocol.NewSecret("plugin-secret")}}}); err != nil {
				return err
			}
		}
		if _, err := SetPluginEnabled(ctx, db, "shared", mustPlugin(ctx, db, "shared").Summary.Revision, false); err != nil {
			return err
		}
		first, err := exportConfigurationFixture(ctx, db, sourceNode, &instructions)
		if err != nil {
			return err
		}
		// Unchanged configuration exports the same bytes.
		if document, err = exportConfigurationFixture(ctx, db, sourceNode, &instructions); err != nil {
			return err
		}
		if string(first) != string(document) {
			return fmt.Errorf("exports differ:\n%s\n%s", first, document)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	for _, header := range []string{`"format":"sailry-configuration"`, `"version":1`, `"exported_at":"2026-11-20T08:00:00Z"`} {
		if !strings.Contains(string(document), header) {
			t.Fatalf("export lacks %s: %s", header, document)
		}
	}
	for _, secret := range []string{"provider-key", "search-key", "plugin-secret", instructions} {
		if !strings.Contains(string(document), secret) {
			t.Fatalf("export lacks %q: %s", secret, document)
		}
	}
	if strings.Contains(string(document), `"credential":{`) || strings.Contains(string(document), `"owner"`) {
		t.Fatalf("export contains Node-bound provider fields: %s", document)
	}

	t.Run("an absent section leaves its area unchanged", func(t *testing.T) {
		_, node, target := schemaFixture(t)
		partial := json.RawMessage(`{"format":"sailry-configuration","version":1,"exported_at":"2026-11-20T08:00:00Z","node":[` + strings.TrimSuffix(strings.Repeat("0,", 32), ",") + `],"compaction":{"revision":0,"percent":60}}`)
		result, err := importConfigurationFixture(ctx, target, node, partial, false)
		if err != nil {
			t.Fatal(err)
		}
		if len(result.Items) != 1 || result.Items[0].Area != "compaction" || result.Items[0].Action != protocol.ConfigurationImportUpdated {
			t.Fatalf("partial import report = %+v", result.Items)
		}
	})

	t.Run("rejects another format or version", func(t *testing.T) {
		for _, body := range []string{`{"format":"other","version":1}`, `{"format":"sailry-configuration","version":2}`, `[]`} {
			if _, err := ParseConfiguration(json.RawMessage(body)); !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
				t.Fatalf("%s was accepted: %v", body, err)
			}
		}
	})

	t.Run("preview changes nothing", func(t *testing.T) {
		_, node, target := schemaFixture(t)
		result, err := importConfigurationFixture(ctx, target, node, document, true)
		if err != nil {
			t.Fatal(err)
		}
		if item := configurationItems(result.Items)["providers/Keyed"]; item.Action != protocol.ConfigurationImportAdded {
			t.Fatalf("preview report = %+v", result.Items)
		}
		if err := target.Node.Read(ctx, func(db *sql.Conn) error {
			values, err := providerList(ctx, db)
			if err != nil || len(values) != 0 {
				return fmt.Errorf("preview saved providers: %d %v", len(values), err)
			}
			roles, err := ReadRoles(ctx, db)
			if err != nil || slices.ContainsFunc(roles, func(value protocol.RoleProfile) bool { return value.ID == role.ID }) {
				return fmt.Errorf("preview saved roles: %d %v", len(roles), err)
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	})

	t.Run("merges into another profile", func(t *testing.T) {
		_, node, target := schemaFixture(t)
		kept := providerFixture()
		kept.Name = "Kept"
		if err := target.Node.Write(ctx, func(db *sql.Conn) error {
			key := protocol.NewSecret("kept-key")
			if _, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: kept, Secret: &key}}}, nil); err != nil {
				return err
			}
			if _, err := InstallPlugin(ctx, db, configurationPluginInfo(t, "shared", 'a'), 0, true); err != nil {
				return err
			}
			_, err := InstallPlugin(ctx, db, configurationPluginInfo(t, "updated", 'd'), 0, true)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		result, err := importConfigurationFixture(ctx, target, node, document, false)
		if err != nil {
			t.Fatal(err)
		}
		items := configurationItems(result.Items)
		for name, action := range map[string]protocol.ConfigurationImportAction{
			"providers/Keyed": protocol.ConfigurationImportAdded, "providers/Account": protocol.ConfigurationImportAdded,
			"search/": protocol.ConfigurationImportUpdated, "media/": protocol.ConfigurationImportUpdated,
			"roles/Reviewer": protocol.ConfigurationImportAdded, "roles/Explore": protocol.ConfigurationImportUpdated, "teams/Pair": protocol.ConfigurationImportAdded,
			"sandbox/": protocol.ConfigurationImportUpdated, PersonalInstructionsArea + "/": protocol.ConfigurationImportAdded,
			"plugins/shared": protocol.ConfigurationImportUpdated, "plugin_settings/shared": protocol.ConfigurationImportUpdated,
			"plugins/updated": protocol.ConfigurationImportUpdated, "plugin_settings/updated": protocol.ConfigurationImportSkipped,
			"plugins/missing": protocol.ConfigurationImportSkipped,
		} {
			if items[name].Action != action {
				t.Fatalf("%s = %+v, want %s; report %+v", name, items[name], action, result.Items)
			}
		}
		if reason := items["providers/Account"].Reason; reason == nil || !strings.Contains(*reason, "sign in") {
			t.Fatalf("account provider reason = %v", reason)
		}
		if reason := items["plugins/missing"].Reason; reason == nil || !strings.Contains(*reason, "not installed") {
			t.Fatalf("missing plugin reason = %v", reason)
		}
		if reason := items["plugin_settings/updated"].Reason; reason == nil || !strings.Contains(*reason, "different version") {
			t.Fatalf("different version reason = %v", reason)
		}
		if result.Instructions == nil || *result.Instructions != instructions {
			t.Fatalf("instructions to write = %v", result.Instructions)
		}
		if err := target.Node.Read(ctx, func(db *sql.Conn) error {
			values, err := providerList(ctx, db)
			if err != nil {
				return err
			}
			byName := map[string]protocol.ConversationProvider{}
			for _, value := range values {
				byName[value.Name] = value
			}
			if len(values) != 3 || byName["Kept"].ID != kept.ID || byName["Keyed"].ID != keyed.ID {
				return fmt.Errorf("providers after merge: %+v", values)
			}
			imported := byName["Keyed"]
			if imported.Credential == nil || imported.Credential.Node != node {
				return fmt.Errorf("imported key has no local credential: %+v", imported.Credential)
			}
			key, err := resolveProviderKey(ctx, db, node, *imported.Credential, imported.ID)
			if err != nil || key.Expose() != "provider-key" {
				return fmt.Errorf("imported key: %v", err)
			}
			if byName["Account"].Credential != nil {
				return fmt.Errorf("account provider received a credential")
			}
			plan, err := ReadWebSearchPlan(ctx, db)
			if err != nil || plan.Credentials[protocol.SearchDriverTavily].Key == nil || plan.Credentials[protocol.SearchDriverTavily].Key.Expose() != "search-key" {
				return fmt.Errorf("search key: %v", err)
			}
			media, err := ReadMediaSettings(ctx, db)
			if err != nil || media.Bindings[protocol.MediaKindVision] == nil || media.Bindings[protocol.MediaKindVision].Provider != keyed.ID {
				return fmt.Errorf("media: %+v %v", media, err)
			}
			roles, err := ReadRoles(ctx, db)
			if err != nil || len(roles) != 3 {
				return fmt.Errorf("roles: %+v %v", roles, err)
			}
			for _, value := range roles {
				if value.Key == "reviewer" && (value.ID != role.ID || value.Model == nil || value.Model.Provider != keyed.ID) {
					return fmt.Errorf("imported role: %+v", value)
				}
			}
			teams, err := ReadTeams(ctx, db)
			if err != nil || len(teams) != 1 || !reflect.DeepEqual(teams[0].Members, []protocol.RoleID{role.ID}) {
				return fmt.Errorf("teams: %+v %v", teams, err)
			}
			sandbox, err := ReadSandboxSettings(ctx, db)
			if err != nil || sandbox.Enabled {
				return fmt.Errorf("sandbox: %+v %v", sandbox, err)
			}
			shared := mustPlugin(ctx, db, "shared")
			if shared.Summary.Enabled {
				return fmt.Errorf("shared plugin stayed enabled")
			}
			settings, err := ResolvePluginSettings(ctx, db, plugins.PackageReference(shared))
			if err != nil || string(settings.Values["mode"]) != `"careful"` || settings.Secrets["token"].Expose() != "plugin-secret" {
				return fmt.Errorf("shared plugin settings: %+v %v", settings.Values, err)
			}
			if updated := mustPlugin(ctx, db, "updated"); updated.Summary.SettingsRevision != 0 {
				return fmt.Errorf("a different version received settings")
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
		// A second import of the same document updates in place.
		again, err := importConfigurationFixture(ctx, target, node, document, false)
		if err != nil {
			t.Fatal(err)
		}
		if items := configurationItems(again.Items); items["providers/Keyed"].Action != protocol.ConfigurationImportUpdated || items["roles/Reviewer"].Action != protocol.ConfigurationImportUpdated {
			t.Fatalf("second import = %+v", again.Items)
		}
	})

	t.Run("machine settings require an administrator", func(t *testing.T) {
		_, node, target := schemaFixture(t)
		var userCtx context.Context
		if err := target.Node.Write(ctx, func(db *sql.Conn) error {
			user, err := CreateUser(ctx, db, "Member")
			if err != nil {
				return err
			}
			user.Administrator = false
			userCtx = WithUser(ctx, user)
			return setAdministrator(ctx, db, user.ID, false)
		}); err != nil {
			t.Fatal(err)
		}
		result, err := importConfigurationFixture(userCtx, target, node, document, false)
		if err != nil {
			t.Fatal(err)
		}
		items := configurationItems(result.Items)
		for _, area := range []string{"sandbox", "power", "python", PersonalInstructionsArea} {
			if items[area+"/"].Action != protocol.ConfigurationImportSkipped {
				t.Fatalf("%s = %+v", area, items[area+"/"])
			}
		}
		if result.Instructions != nil {
			t.Fatal("a member's import would write the personal instructions")
		}
		if err := target.Node.Read(userCtx, func(db *sql.Conn) error {
			sandbox, err := ReadSandboxSettings(userCtx, db)
			if err != nil || !sandbox.Enabled {
				return fmt.Errorf("sandbox changed: %+v %v", sandbox, err)
			}
			exported, err := exportConfigurationFixture(userCtx, db, node, &instructions)
			var parsed ConfigurationDocument
			if err == nil {
				parsed, err = ParseConfiguration(exported)
			}
			if err != nil || parsed.Sandbox != nil || parsed.Power != nil || parsed.Python != nil || parsed.Instructions != nil {
				return fmt.Errorf("member export contains machine settings: %v", err)
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	})
}

func mustPlugin(ctx context.Context, db *sql.Conn, name string) protocol.PluginInfo {
	info, err := ReadPlugin(ctx, db, name)
	if err != nil || info == nil {
		panic(fmt.Sprintf("plugin %s: %v", name, err))
	}
	return *info
}

func exportConfigurationFixture(ctx context.Context, db *sql.Conn, node protocol.NodeID, instructions *string) (json.RawMessage, error) {
	document, err := ExportConfiguration(ctx, db, node, time.Date(2026, 11, 20, 16, 0, 0, 0, time.FixedZone("CST", 8*3600)), instructions)
	if err != nil {
		return nil, err
	}
	return json.Marshal(document)
}
