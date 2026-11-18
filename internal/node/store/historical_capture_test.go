package store

import (
	"context"
	"database/sql"
	"fmt"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func createAdmittedLocalProviderTurn(ctx context.Context, db *sql.Conn, storage *Store, node, caller protocol.NodeID, id protocol.ProviderID, now time.Time) (protocol.Session, protocol.QueuedTurn, error) {
	provider := providerFixture()
	provider.ID = id
	secret := protocol.NewSecret("local-capture-fixture-key")
	output, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{
		Provider: provider, ExpectedRevision: 0, Secret: &secret,
	}}}, nil)
	if err != nil {
		return protocol.Session{}, protocol.QueuedTurn{}, err
	}
	saved := output.Data.(protocol.ConversationProvider)
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Credential = saved.ID, saved.DefaultModel, saved.Credential
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	output, _, err = createProfiledSession(ctx, db, node, storage.Profile, protocol.CommandCreateSessionPayload{Config: &config}, nil, nil)
	if err != nil {
		return protocol.Session{}, protocol.QueuedTurn{}, err
	}
	session := output.Data.(protocol.Session)
	turn, err := admitCredentialClaim(ctx, db, storage, node, caller, session, now)
	return session, turn, err
}

func installHistoricalRemoteCapture(ctx context.Context, db *sql.Conn, session protocol.Session, turn protocol.TurnID, source protocol.NodeID, provider protocol.ConversationProvider) error {
	config := session.Config
	config.Provider, config.Model, config.Credential = provider.ID, provider.DefaultModel, provider.Credential
	profile := protocol.SessionProfile{Source: source, Provider: provider}
	profileBody, err := encode(profile)
	if err != nil {
		return err
	}
	configBody, err := encode(config)
	if err != nil {
		return err
	}
	providerBody, err := encode(provider)
	if err != nil {
		return err
	}
	result, err := db.ExecContext(ctx, "UPDATE session_revisions SET config=?3,profile=?4 WHERE session=?1 AND revision=?2", session.ID.String(), session.Revision, configBody, profileBody)
	if err != nil {
		return err
	}
	changed, err := result.RowsAffected()
	if err != nil {
		return err
	}
	if changed != 1 {
		return fmt.Errorf("update historical session capture: changed %d rows", changed)
	}
	result, err = db.ExecContext(ctx, "UPDATE agent_runs SET provider=?2 WHERE turn=?1", turn.String(), providerBody)
	if err != nil {
		return err
	}
	changed, err = result.RowsAffected()
	if err != nil {
		return err
	}
	if changed != 1 {
		return fmt.Errorf("update historical turn capture: changed %d rows", changed)
	}
	return nil
}

func TestHistoricalRemoteSessionRejectsNewTurnAndKeepsAdmittedTurn(t *testing.T) {
	t.Parallel()
	storage, node, controller, ctx := controllerCacheFixture(t)
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		provider := providerFixture()
		reference := protocol.CredentialRef{Node: controller, ID: protocol.NewCredentialID()}
		provider.Credential = &reference
		runtime := protocol.RuntimeCredential{
			Controller: controller, Kind: "provider", ID: reference.ID.String(), Generation: reference.ID.String(),
			Revision: 1, State: "ready", Provider: &provider.ID, Authentication: protocol.AuthenticationApiKey,
			Access: protocol.NewSecret("historical-runtime-key"),
		}
		if err := seedHistoricalRuntimeCredentials(ctx, db, runtime); err != nil {
			return err
		}
		session, turn, err := createAdmittedLocalProviderTurn(ctx, db, storage, node, controller, provider.ID, time.Now())
		if err != nil {
			return err
		}
		if err := installHistoricalRemoteCapture(ctx, db, session, turn.ID, controller, provider); err != nil {
			return err
		}
		historical, err := ReadSession(ctx, db, session.ID)
		if err != nil {
			return err
		}
		request := protocol.NewRequest(node, protocol.Command{Kind: "submit_turn", Data: &protocol.CommandSubmitTurnPayload{
			Session: historical.ID, ExpectedRevision: historical.Revision,
			Message: protocol.ConversationInput{Text: "This must not use a remote account"},
		}})
		if _, err := admitAgentTurn(ctx, db, node, controller, request, historical, protocol.ConversationRunKindTask, true); !providerFixtureFault(err, protocol.ErrorCodeNotConfigured) {
			return fmt.Errorf("historical remote session accepted a new turn: %v", err)
		}
		var count int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM turns WHERE session=?1", session.ID.String()).Scan(&count); err != nil {
			return err
		}
		if count != 1 {
			return fmt.Errorf("rejected turn changed the session history: %d turns", count)
		}
		invocation, _, err := ClaimNext(ctx, db, time.Now())
		if err != nil || invocation == nil || invocation.Turn.ID != turn.ID {
			return fmt.Errorf("previously admitted turn did not continue: %v", err)
		}
		if invocation.AuthorizationFailure != nil || invocation.Secret == nil || invocation.Secret.Expose() != runtime.Access.Expose() {
			return fmt.Errorf("previously admitted turn lost its captured credential: %v", invocation.AuthorizationFailure)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestCreateSessionRejectsProviderMissingOnExecutionNode(t *testing.T) {
	t.Parallel()
	profile, node, storage := schemaFixture(t)
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = protocol.NewProviderID(), "remote-only-model"
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
	err := storage.Node.Write(t.Context(), func(db *sql.Conn) error {
		_, _, err := createProfiledSession(t.Context(), db, node, profile, protocol.CommandCreateSessionPayload{Config: &config}, nil, nil)
		if !providerFixtureFault(err, protocol.ErrorCodeNotConfigured) {
			return fmt.Errorf("session accepted a missing local model channel: %v", err)
		}
		var count int
		if err := db.QueryRowContext(t.Context(), "SELECT count(*) FROM sessions").Scan(&count); err != nil {
			return err
		}
		if count != 0 {
			return fmt.Errorf("failed session creation left %d sessions", count)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestHistoricalRemoteSessionRejectsModelReassignmentAndFork(t *testing.T) {
	t.Parallel()
	storage, node, session, turn, _ := agentHistoryFixture(t, protocol.WorkModeCode)
	controller := protocol.NodeID{82}
	err := storage.Node.Write(t.Context(), func(db *sql.Conn) error {
		saved, err := readProviderConfiguration(t.Context(), db, session.Config.Provider)
		if err != nil {
			return err
		}
		if saved == nil || saved.Provider == nil {
			return fmt.Errorf("local provider fixture disappeared")
		}
		if err := installHistoricalRemoteCapture(t.Context(), db, session, turn, controller, *saved.Provider); err != nil {
			return err
		}
		current, err := ReadSession(t.Context(), db, session.ID)
		if err != nil {
			return err
		}
		request := protocol.NewRequest(node, protocol.Command{Kind: "set_session_config", Data: &protocol.CommandSetSessionConfigPayload{
			Session: current.ID, ExpectedRevision: current.Revision, Config: current.Config,
		}})
		if _, _, err := ExecuteAgentCommand(t.Context(), db, node, storage.Profile, node, request, time.Now()); !providerFixtureFault(err, protocol.ErrorCodeNotConfigured) {
			return fmt.Errorf("legacy remote session accepted local model reassignment: %v", err)
		}
		if _, _, err := forkConversation(t.Context(), db, protocol.CommandForkConversationPayload{
			Session: current.ID, Through: turn, ExpectedRevision: current.Revision,
		}); !providerFixtureFault(err, protocol.ErrorCodeNotConfigured) {
			return fmt.Errorf("legacy remote session created a new fork: %v", err)
		}
		var count int
		if err := db.QueryRowContext(t.Context(), "SELECT count(*) FROM sessions").Scan(&count); err != nil {
			return err
		}
		if count != 1 {
			return fmt.Errorf("rejected operations left %d sessions", count)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
