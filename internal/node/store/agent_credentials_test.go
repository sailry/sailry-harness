package store

import (
	"context"
	"database/sql"
	"fmt"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func admitCredentialClaim(ctx context.Context, db *sql.Conn, storage *Store, node, caller protocol.NodeID, session protocol.Session, now time.Time) (protocol.QueuedTurn, error) {
	request := protocol.NewRequest(node, protocol.Command{Kind: "submit_turn", Data: &protocol.CommandSubmitTurnPayload{
		Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Use the captured account"},
	}})
	body, err := encode(request)
	if err != nil {
		return protocol.QueuedTurn{}, err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", caller[:], request.ID.String(), body); err != nil {
		return protocol.QueuedTurn{}, err
	}
	output, _, err := ExecuteAgentCommand(ctx, db, node, storage.Profile, caller, request, now)
	if err != nil {
		return protocol.QueuedTurn{}, err
	}
	return output.Data.(protocol.QueuedTurn), nil
}

func TestClaimForwardedTurnRetainsCapturedAPIKey(t *testing.T) {
	t.Parallel()
	for _, fixture := range []struct {
		name string
		code protocol.ErrorCode
	}{
		{"retained", ""},
		{"revoked", protocol.ErrorCodeNotConfigured},
		{"expired", protocol.ErrorCodeExpired},
		{"different provider", protocol.ErrorCodePermissionDenied},
		{"different generation", protocol.ErrorCodePermissionDenied},
		{"different authentication", protocol.ErrorCodePermissionDenied},
		{"missing", protocol.ErrorCodeNotConfigured},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			t.Parallel()
			storage, node, controller, ctx := controllerCacheFixture(t)
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				provider := providerFixture()
				previous := protocol.CredentialRef{Node: controller, ID: protocol.NewCredentialID()}
				current := protocol.CredentialRef{Node: controller, ID: protocol.NewCredentialID()}
				provider.Credential, provider.Revision = &previous, 1
				publishProvider := func() error {
					_, err := applyConfigurationCache(ctx, db, node, controller, protocol.ConfigurationManifest{
						Source: controller, Category: "providers", Revision: provider.Revision,
						Records: []protocol.ConfigurationFollowRecord{followRecord(t, provider.ID.String(), provider.Revision, provider, nil)},
					})
					return err
				}
				publishCredentials := func(revision uint64, credentials ...protocol.RuntimeCredential) error {
					records := make([]protocol.ConfigurationFollowRecord, 0, len(credentials))
					for _, credential := range credentials {
						records = append(records, followRecord(t, credential.CacheID(), credential.Revision, credential, nil))
					}
					_, err := applyConfigurationCache(ctx, db, node, controller, protocol.ConfigurationManifest{
						Source: controller, Category: "runtime_credentials", Revision: revision, Records: records,
					})
					return err
				}
				old := protocol.RuntimeCredential{
					Controller: controller, Kind: "provider", ID: previous.ID.String(), Generation: previous.ID.String(),
					Revision: 1, State: "ready", Provider: &provider.ID, Authentication: protocol.AuthenticationApiKey,
					Access: protocol.NewSecret("captured-key"),
				}
				if err := publishProvider(); err != nil {
					return err
				}
				if err := publishCredentials(1, old); err != nil {
					return err
				}
				bundle := protocol.DefaultSessionImport()
				bundle.Source, bundle.Provider = controller, provider
				bundle.Config.Provider, bundle.Config.Model, bundle.Config.Credential = provider.ID, "fixture", &previous
				bundle.Config.Mode, bundle.Config.Permission = protocol.WorkModeCode, protocol.PermissionProject
				bundle.Roles.Execution = protocol.ConversationExecutionModel
				output, _, err := importSession(ctx, db, node, controller, storage.Profile, bundle)
				if err != nil {
					return err
				}
				session := output.Data.(protocol.Session)
				// A controller key rotation changes the provider's current selection
				// and keeps the previous credential in the runtime snapshot.
				provider.Credential, provider.Revision = &current, 2
				next := old
				next.ID, next.Generation, next.Access = current.ID.String(), current.ID.String(), protocol.NewSecret("new-key")
				if err := publishProvider(); err != nil {
					return err
				}
				if err := publishCredentials(2, old, next); err != nil {
					return err
				}
				now := time.Now()
				turn, err := admitCredentialClaim(ctx, db, storage, node, controller, session, now)
				if err != nil {
					return fmt.Errorf("admit existing session after key rotation: %w", err)
				}
				old.Revision++
				switch fixture.name {
				case "revoked":
					old.State, old.Access = "revoked", protocol.NewSecret("")
				case "expired":
					expiry := uint64(1)
					old.ExpiresAtMs = &expiry
				case "different provider":
					other := protocol.NewProviderID()
					old.Provider = &other
				case "different generation":
					old.Generation = current.ID.String()
				case "different authentication":
					old.Authentication = protocol.AuthenticationClaudeOAuth
				}
				credentials := []protocol.RuntimeCredential{old, next}
				if fixture.name == "missing" {
					credentials = []protocol.RuntimeCredential{next}
				}
				if err := publishCredentials(3, credentials...); err != nil {
					return err
				}
				invocation, _, err := ClaimNext(ctx, db, now)
				if err != nil || invocation == nil || invocation.Run.Turn != turn.ID {
					return fmt.Errorf("claim captured turn: %v", err)
				}
				if invocation.Turn.Config.Credential == nil || *invocation.Turn.Config.Credential != previous ||
					invocation.Provider == nil || invocation.Provider.Credential == nil || *invocation.Provider.Credential != previous {
					return fmt.Errorf("claim replaced the session's captured credential")
				}
				if fixture.code != "" {
					if invocation.Secret != nil || invocation.AuthorizationFailure == nil || invocation.AuthorizationFailure.Code != fixture.code {
						return fmt.Errorf("claim accepted unavailable credential: %v", invocation.AuthorizationFailure)
					}
				} else if invocation.AuthorizationFailure != nil || invocation.Secret == nil || invocation.Secret.Expose() != "captured-key" {
					return fmt.Errorf("claim did not retain the valid captured key: %v", invocation.AuthorizationFailure)
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestClaimLocalTurnRetainsCapturedAPIKey(t *testing.T) {
	t.Parallel()
	for _, fixture := range []struct {
		name string
		code protocol.ErrorCode
	}{
		{"retained", ""},
		{"revoked", protocol.ErrorCodeNotConfigured},
		{"expired", protocol.ErrorCodePermissionDenied},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			t.Parallel()
			_, node, storage := schemaFixture(t)
			ctx := t.Context()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				provider := providerFixture()
				saveKey := func(text string) error {
					secret := protocol.NewSecret(text)
					output, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{
						Provider: provider, ExpectedRevision: provider.Revision, Secret: &secret,
					}}}, nil)
					if err == nil {
						provider = output.Data.(protocol.ConversationProvider)
					}
					return err
				}
				if err := saveKey("captured-key"); err != nil {
					return err
				}
				previous := *provider.Credential
				config := protocol.DefaultSessionConfig()
				config.Provider, config.Model, config.Credential = provider.ID, "fixture", &previous
				config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
				output, _, err := createSession(ctx, db, node, storage.Profile, protocol.CommandCreateSessionPayload{Config: &config})
				if err != nil {
					return err
				}
				session := output.Data.(protocol.Session)
				if err := saveKey("new-key"); err != nil {
					return err
				}
				now := time.Now()
				turn, err := admitCredentialClaim(ctx, db, storage, node, node, session, now)
				if err != nil {
					return err
				}
				saved, err := readProviderConfiguration(ctx, db, provider.ID)
				if err != nil {
					return err
				}
				credential := saved.Authorizations[previous.ID]
				switch fixture.name {
				case "revoked":
					credential.Key = nil
				case "expired":
					expiry := uint64(1)
					credential.ExpiresAtMs = &expiry
				}
				saved.Authorizations[previous.ID] = credential
				if err := saveProviderConfiguration(ctx, db, *saved); err != nil {
					return err
				}
				invocation, _, err := ClaimNext(ctx, db, now)
				if err != nil || invocation == nil || invocation.Run.Turn != turn.ID {
					return fmt.Errorf("claim captured turn: %v", err)
				}
				if invocation.Turn.Config.Credential == nil || *invocation.Turn.Config.Credential != previous {
					return fmt.Errorf("claim replaced the captured credential")
				}
				if fixture.code != "" {
					if invocation.Secret != nil || invocation.AuthorizationFailure == nil || invocation.AuthorizationFailure.Code != fixture.code {
						return fmt.Errorf("claim accepted unavailable credential: %v", invocation.AuthorizationFailure)
					}
				} else if invocation.AuthorizationFailure != nil || invocation.Secret == nil || invocation.Secret.Expose() != "captured-key" {
					return fmt.Errorf("claim did not retain the valid captured key: %v", invocation.AuthorizationFailure)
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}
