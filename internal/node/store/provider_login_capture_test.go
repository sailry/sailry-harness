package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginLoginFixture struct {
	info     protocol.PluginInfo
	provider protocol.ConversationProvider
	grant    providers.AccountGrant
}

func newPluginLoginFixture(ctx context.Context, db *sql.Conn, node protocol.NodeID) (pluginLoginFixture, error) {
	var fixture pluginLoginFixture
	info := pluginTestInfo("captured-account", 'a')
	schema, err := plugins.ValidateSettings([]byte(`{"$schema":"` + plugins.SettingsSchema + `","type":"object","additionalProperties":false,"properties":{"client":{"type":"string"},"label":{"type":"string"}},"required":["client","label"]}`))
	if err != nil {
		return fixture, err
	}
	info.Settings = &schema
	extension := protocol.DefaultPluginExtension()
	extension.ApiVersion = "v1"
	extension.Actions = []protocol.PluginAction{protocol.PluginActionProvideModels, protocol.PluginActionAuthenticateModels, protocol.PluginActionInspectCredentials}
	client := "client"
	declaration := protocol.DefaultPluginModelAuthDeclaration()
	declaration.ID, declaration.Display.Label, declaration.Client.ClientIdSetting = "account", "Account", &client
	declaration.Scopes = []string{"model.invoke"}
	declaration.Flow = protocol.PluginModelAuthFlow{Kind: "script", Data: protocol.PluginModelAuthFlowScriptPayload{
		Script: protocol.PluginModelAdapterScript{Entry: "auth.js", Resources: []string{}}, Refresh: true, Handler: "step", Requests: map[string]protocol.PluginModelAuthEndpoint{},
	}}
	preset := protocol.DefaultConversationPresetsPreset()
	preset.ID, preset.Name = "account", "Account"
	preset.Auth = protocol.ConversationPresetsAuth{Kind: "plugin", Data: protocol.ConversationPresetsAuthPluginPayload{Flow: declaration.ID}}
	preset.Routes = []protocol.ConversationPresetsRoute{{Protocol: protocol.ConversationPresetsProtocolOpenAiResponses, Endpoint: "https://api.example.test/v1"}}
	extension.ModelProviders = []protocol.ConversationPresetsPreset{preset}
	extension.ModelAuth, info.Extension = []protocol.PluginModelAuthDeclaration{declaration}, &extension
	info, err = InstallPlugin(ctx, db, info, 0, true)
	if err != nil {
		return fixture, err
	}
	settings, err := SavePluginSettings(ctx, db, protocol.CommandSavePluginSettingsPayload{Package: plugins.PackageReference(info),
		Values: map[string]json.RawMessage{"client": json.RawMessage(`"fixture-client"`), "label": json.RawMessage(`"original"`)}})
	if err != nil {
		return fixture, err
	}
	current, err := ReadPlugin(ctx, db, info.Summary.Name)
	if err != nil {
		return fixture, err
	}
	fixture.info = *current
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Revision, provider.Enabled = protocol.NewProviderID(), 1, true
	provider.Name, provider.Api, provider.Authentication, provider.Endpoint = "Captured account", protocol.ConversationModelApiResponses, protocol.AuthenticationPlugin, "https://api.example.test/v1"
	provider.Preset = &protocol.ConversationPresetsPresetRef{Source: protocol.ConversationPresetsSource{Kind: "package", Data: protocol.ConversationPresetsSourcePackagePayload{Name: info.Summary.Name}}, ID: preset.ID}
	origin := pluginAccountOrigin(*current)
	binding := &protocol.ConversationPresetsBinding{Package: &settings.Package, Origin: &origin, Auth: &protocol.PluginModelAuthBinding{Flow: declaration.ID}}
	provider.Binding = &binding
	if err := saveProviderConfiguration(ctx, db, providerConfiguration{ID: provider.ID, Listed: true, Provider: &provider}); err != nil {
		return fixture, err
	}
	capture, err := CaptureProviderLogin(ctx, db, provider.ID, provider.Revision, protocol.NewRequestID())
	if err != nil {
		return fixture, err
	}
	issued, expiry, refresh := uint64(time.Now().Add(-time.Hour).UnixMilli()), uint64(time.Now().Add(time.Hour).UnixMilli()), "fixture-refresh"
	fixture.grant = providers.AccountGrant{Kind: protocol.AuthenticationPlugin, Record: &providers.AccountRecord{Target: capture.Target,
		Issued: authorization.Issued{Access: "fixture-access", Refresh: &refresh, IssuedAtMs: &issued, ExpiresAtMs: &expiry, Extra: map[string]string{}}}}
	if _, err := CompleteProviderLogin(ctx, db, node, capture, fixture.grant); err != nil {
		return fixture, err
	}
	saved, err := currentProvider(ctx, db, provider.ID, nil)
	if err == nil {
		fixture.provider = *saved.Provider
	}
	return fixture, err
}

func TestPluginProviderAuthorizationRetainsCapture(t *testing.T) {
	t.Parallel()
	for _, change := range []string{"package update", "unrelated settings"} {
		t.Run(change, func(t *testing.T) {
			_, node, storage := schemaFixture(t)
			ctx := t.Context()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				fixture, err := newPluginLoginFixture(ctx, db, node)
				if err != nil {
					return err
				}
				update := func(index int) error {
					info, err := ReadPlugin(ctx, db, fixture.info.Summary.Name)
					if err != nil {
						return err
					}
					if change == "package update" {
						info.Summary.Digest = strings.Repeat(string(rune('b'+index)), 64)
						version := fmt.Sprintf("0.1.%d", index+1)
						info.Summary.Version = &version
						_, err = InstallPlugin(ctx, db, *info, info.Summary.Revision, true)
					} else {
						label, _ := json.Marshal(fmt.Sprintf("label-%d", index))
						_, err = SavePluginSettings(ctx, db, protocol.CommandSavePluginSettingsPayload{Package: plugins.PackageReference(*info),
							Values: map[string]json.RawMessage{"client": json.RawMessage(`"fixture-client"`), "label": label}})
					}
					return err
				}
				if err := update(0); err != nil {
					return err
				}
				secret, err := ReadProviderAuthorization(ctx, db, node, fixture.provider, *fixture.provider.Credential)
				if err != nil || secret == nil {
					return fmt.Errorf("read after %s: %v", change, err)
				}
				renewal, err := BeginProviderRenewal(ctx, db, node, fixture.provider, *fixture.provider.Credential, time.Now())
				if err != nil || renewal == nil {
					return fmt.Errorf("renewal after %s: %v", change, err)
				}
				if !reflect.DeepEqual(renewal.Capture.Target, fixture.grant.Record.Target) {
					return fmt.Errorf("renewal replaced the captured package or client settings")
				}
				// Another update while the refresh is in flight must not discard
				// its result and leave a rotating credential permanently pending.
				if err := update(1); err != nil {
					return err
				}
				rotated := fixture.grant
				record := *rotated.Record
				record.Issued.Access = "rotated-access"
				rotated.Record = &record
				if _, err := CompleteProviderRenewal(ctx, db, node, *renewal, rotated); err != nil {
					return fmt.Errorf("complete renewal after %s: %w", change, err)
				}
				secret, err = ReadProviderAuthorization(ctx, db, node, fixture.provider, *fixture.provider.Credential)
				if err != nil {
					return err
				}
				stored, err := providers.DecodeAccount(secret, protocol.AuthenticationPlugin)
				if err != nil || stored.Record == nil || stored.Record.Issued.Access != "rotated-access" || stored.RenewalPending {
					return fmt.Errorf("captured authorization did not retain the completed renewal: %v", err)
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestPluginProviderAuthorizationRejectsRevokedAccess(t *testing.T) {
	t.Parallel()
	for _, change := range []string{"disabled", "removed", "another user"} {
		t.Run(change, func(t *testing.T) {
			_, node, storage := schemaFixture(t)
			ctx := t.Context()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				fixture, err := newPluginLoginFixture(ctx, db, node)
				if err != nil {
					return err
				}
				renewal, err := BeginProviderRenewal(ctx, db, node, fixture.provider, *fixture.provider.Credential, time.Now())
				if err != nil || renewal == nil {
					return fmt.Errorf("capture renewal: %v", err)
				}
				access := ctx
				switch change {
				case "disabled":
					_, err = SetPluginEnabled(ctx, db, fixture.info.Summary.Name, fixture.info.Summary.Revision, false)
				case "removed":
					_, err = RemovePlugin(ctx, db, fixture.info.Summary.Name, fixture.info.Summary.Revision)
				case "another user":
					user, createErr := CreateUser(ctx, db, "Other account owner")
					if createErr != nil {
						return createErr
					}
					access = WithUser(ctx, user)
					_, err = InstallPlugin(access, db, fixture.info, 0, true)
				}
				if err != nil {
					return err
				}
				if _, err := ReadProviderAuthorization(access, db, node, fixture.provider, *fixture.provider.Credential); err == nil {
					return fmt.Errorf("%s still read the captured credential", change)
				}
				if _, err := CompleteProviderRenewal(access, db, node, *renewal, fixture.grant); err == nil {
					return fmt.Errorf("%s still completed the captured renewal", change)
				}
				// Clear the in-flight marker to test admission independently of
				// the existing prohibition against a second rotating refresh.
				saved, err := currentProvider(ctx, db, fixture.provider.ID, nil)
				if err != nil {
					return err
				}
				body, err := json.Marshal(fixture.grant)
				if err != nil {
					return err
				}
				entry := saved.Authorizations[fixture.provider.Credential.ID]
				secret := protocol.NewSecret(string(body))
				entry.Key = &secret
				saved.Authorizations[fixture.provider.Credential.ID] = entry
				if err := saveProviderConfiguration(ctx, db, *saved); err != nil {
					return err
				}
				if _, err := BeginProviderRenewal(access, db, node, fixture.provider, *fixture.provider.Credential, time.Now()); err == nil {
					return fmt.Errorf("%s still began a captured renewal", change)
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestPluginProviderAuthorizationRejectsChangedTarget(t *testing.T) {
	t.Parallel()
	for _, change := range []string{"client", "scopes", "flow", "endpoint"} {
		t.Run(change, func(t *testing.T) {
			_, node, storage := schemaFixture(t)
			ctx := t.Context()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				fixture, err := newPluginLoginFixture(ctx, db, node)
				if err != nil {
					return err
				}
				info := fixture.info
				if change == "client" {
					settings, err := SavePluginSettings(ctx, db, protocol.CommandSavePluginSettingsPayload{Package: plugins.PackageReference(info),
						Values: map[string]json.RawMessage{"client": json.RawMessage(`"different-client"`), "label": json.RawMessage(`"original"`)}})
					if err != nil {
						return err
					}
					info.Summary.SettingsRevision = settings.Package.SettingsRevision
				} else if change != "endpoint" {
					info.Summary.Digest = strings.Repeat("b", 64)
					version := "0.1.1"
					info.Summary.Version = &version
					if change == "scopes" {
						info.Extension.ModelAuth[0].Scopes = []string{"different-scope"}
					} else {
						flow := *info.Extension.ModelAuth[0].Flow.Data.(*protocol.PluginModelAuthFlowScriptPayload)
						flow.Handler = "different-handler"
						info.Extension.ModelAuth[0].Flow.Data = flow
					}
					info, err = InstallPlugin(ctx, db, info, info.Summary.Revision, true)
					if err != nil {
						return err
					}
				}
				// Existing captures still use the original account target. A new
				// binding must not apply that grant to a different target.
				if _, err := ReadProviderAuthorization(ctx, db, node, fixture.provider, *fixture.provider.Credential); err != nil {
					return fmt.Errorf("old capture changed with live configuration: %w", err)
				}
				reference := plugins.PackageReference(info)
				binding := **fixture.provider.Binding
				binding.Package = &reference
				pointer := &binding
				fixture.provider.Binding = &pointer
				if change == "endpoint" {
					fixture.provider.Endpoint = "https://different.example.test/v1"
				}
				for _, operation := range []func() error{
					func() error {
						_, err := ReadProviderAuthorization(ctx, db, node, fixture.provider, *fixture.provider.Credential)
						return err
					},
					func() error {
						_, err := BeginProviderRenewal(ctx, db, node, fixture.provider, *fixture.provider.Credential, time.Now())
						return err
					},
				} {
					var failure *protocol.Fault
					if err := operation(); !errors.As(err, &failure) || failure.ConfigurationReason() != protocol.ConfigurationAuthenticationChanged {
						return fmt.Errorf("new %s target reused an old account: %v", change, err)
					}
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}
