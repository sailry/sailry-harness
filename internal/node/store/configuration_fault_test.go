package store

import (
	"database/sql"
	"errors"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func requireConfigurationReason(t *testing.T, err error, reason protocol.ConfigurationReason) {
	t.Helper()
	var failure *protocol.Fault
	if !errors.As(err, &failure) || failure.Code != protocol.ErrorCodeNotConfigured || failure.ConfigurationReason() != reason {
		t.Fatalf("failure = %#v (%v), want %s", failure, err, reason)
	}
}

func TestTurnConfigurationRefusalReasons(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		name   string
		reason protocol.ConfigurationReason
	}{
		{"disabled", protocol.ConfigurationProviderDisabled},
		{"removed", protocol.ConfigurationProviderRemoved},
		{"missing row", protocol.ConfigurationProviderMissing},
		{"missing definition", protocol.ConfigurationProviderMissing},
		{"missing removed definition", protocol.ConfigurationProviderMissing},
	} {
		t.Run(test.name, func(t *testing.T) {
			storage, node, session, _, _ := agentHistoryFixture(t, protocol.WorkModeCode)
			ctx := t.Context()
			if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				saved, err := readProviderConfiguration(ctx, db, session.Config.Provider)
				if err != nil {
					return err
				}
				switch test.name {
				case "disabled":
					saved.Provider.Enabled = false
				case "removed":
					saved.Listed = false
				case "missing definition":
					saved.Provider = nil
				case "missing removed definition":
					saved.Provider, saved.Listed = nil, false
				case "missing row":
					_, err = db.ExecContext(ctx, "DELETE FROM providers WHERE id=?1", session.Config.Provider.String())
				}
				if test.name != "missing row" {
					err = saveProviderConfiguration(ctx, db, *saved)
				}
				if err != nil {
					return err
				}
				request := protocol.NewRequest(node, protocol.Command{Kind: "submit_turn", Data: &protocol.CommandSubmitTurnPayload{Session: session.ID}})
				_, err = admitAgentTurn(ctx, db, node, node, request, session, protocol.ConversationRunKindTask, true)
				requireConfigurationReason(t, err, test.reason)
				return nil
			}); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestConfigurationCredentialReasons(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	if err := storage.Node.Read(t.Context(), func(db *sql.Conn) error {
		_, err := StartSessionProvider(t.Context(), db, protocol.ConversationStart{})
		requireConfigurationReason(t, err, protocol.ConfigurationDefaultsMissing)
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	reference := protocol.CredentialRef{ID: protocol.NewCredentialID()}
	saved := &providerConfiguration{Authorizations: map[protocol.CredentialID]providerAuthorization{reference.ID: {}}}
	_, err := providerConfigurationKey(saved, reference)
	requireConfigurationReason(t, err, protocol.ConfigurationCredentialMissing)
	_, err = providerConfigurationKey(&providerConfiguration{}, reference)
	requireConfigurationReason(t, err, protocol.ConfigurationCredentialMissing)
	for state, reason := range map[string]protocol.ConfigurationReason{
		"revoked": protocol.ConfigurationCredentialRevoked, "machine_bound": protocol.ConfigurationMachineBound,
		"unavailable": protocol.ConfigurationRuntimeCredentialMissing, "unknown": protocol.ConfigurationRuntimeCredentialMissing,
	} {
		requireConfigurationReason(t, runtimeCredentialReady(protocol.RuntimeCredential{State: state}), reason)
	}
}
