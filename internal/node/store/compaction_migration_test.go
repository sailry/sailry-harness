package store

import (
	"database/sql"
	"fmt"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestV026CompactionUpgradeAndRecovery(t *testing.T) {
	t.Parallel()
	fixture := newV026CompactionFixture(t)
	legacy, custom := protocol.NewUserID(), protocol.NewUserID()
	schemaExecute(t, fixture.db, "INSERT INTO users(id,name,administrator) VALUES(?1,'Storage owner',1),(?2,'Settings user',0)", legacy.String(), custom.String())
	schemaExecute(t, fixture.db, "INSERT INTO identity_storage_origin VALUES(1,?1)", legacy.String())
	schemaExecute(t, fixture.db, "INSERT INTO compaction_settings VALUES(1,?1)", v026CompactionJSON(t, protocol.ConversationCompactionSettings{Revision: 1, Percent: 80}))
	schemaExecute(t, fixture.db, "INSERT INTO user_personal_settings VALUES(?1,'compaction',?2)", custom.String(), v026CompactionJSON(t, protocol.ConversationCompactionSettings{Revision: 2, Percent: 67}))

	running := fixture.addRun(t, fixture.owner, protocol.ConversationStatusRunning, 80)
	ordinary := fixture.addRun(t, fixture.owner, protocol.ConversationStatusQueued, 80)
	explicit := fixture.addRun(t, legacy, protocol.ConversationStatusQueued, 80)
	nonDefault := fixture.addRun(t, custom, protocol.ConversationStatusQueued, 67)
	changed := fixture.addRun(t, custom, protocol.ConversationStatusCompleted, 80)
	unattributed := fixture.addRun(t, protocol.UserID{}, protocol.ConversationStatusCompleted, 80)
	missingOrigin := fixture.addRun(t, fixture.owner, protocol.ConversationStatusCompleted, 80)
	mismatchedOrigin := fixture.addRun(t, fixture.owner, protocol.ConversationStatusCompleted, 80)
	inherited := fixture.addRun(t, fixture.owner, protocol.ConversationStatusCompleted, 80)
	schemaExecute(t, fixture.db, "DELETE FROM identity_turn_origins WHERE turn=?1", missingOrigin.String())
	schemaExecute(t, fixture.db, "UPDATE identity_turn_origins SET user=?2 WHERE turn=?1", mismatchedOrigin.String(), legacy.String())
	schemaExecute(t, fixture.db, "UPDATE agent_runs SET body=CAST(json_set(body,'$.guidance',?2) AS BLOB) WHERE turn=?1", inherited.String(), explicit.String())

	ctx := t.Context()
	now := time.Date(2026, 11, 10, 0, 0, 0, 0, time.UTC)
	if err := fixture.db.Write(ctx, func(db *sql.Conn) error {
		if err := queueFixtureInput(ctx, db, fixture.session.ID, running, now.Add(-time.Second)); err != nil {
			return err
		}
		if err := tagGuidance(ctx, db, explicit, running); err != nil {
			return err
		}
		queue, err := ReadQueue(ctx, db, fixture.session.ID)
		if err != nil {
			return err
		}
		if len(queue.Items) != 3 || queue.Items[0].Turn != explicit || queue.Items[0].Guidance == nil || *queue.Items[0].Guidance != running {
			return fmt.Errorf("fixture did not put guidance ahead of ordinary input: %+v", queue)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	fixture.close(t)

	type expectation struct {
		turn   protocol.TurnID
		name   string
		policy CompactionPolicy
	}
	queued := []struct {
		expectation
		user    protocol.UserID
		trigger uint64
	}{
		{expectation{explicit, "explicit 80 offered as guidance", CompactionPolicy{Percent: 80}}, legacy, 800_000},
		{expectation{ordinary, "ordinary queued default", CompactionPolicy{Percent: 80, UseDefault: true}}, fixture.owner, 800_000},
		{expectation{nonDefault, "explicit 67 queued", CompactionPolicy{Percent: 67}}, custom, 670_000},
	}
	history := []expectation{
		{running, "default running", CompactionPolicy{Percent: 80, UseDefault: true}},
		{changed, "80 preceding a settings change", CompactionPolicy{Percent: 80}},
		{unattributed, "80 without an attributable user", CompactionPolicy{Percent: 80}},
		{missingOrigin, "80 without a capture origin", CompactionPolicy{Percent: 80}},
		{mismatchedOrigin, "80 with a different capture origin", CompactionPolicy{Percent: 80}},
		{inherited, "80 inherited from another user", CompactionPolicy{Percent: 80}},
	}
	checkPolicies := func(db *Handle) {
		t.Helper()
		for _, run := range history {
			checkV026CompactionPolicy(t, db, run.turn, run.name, run.policy)
		}
		for _, run := range queued {
			checkV026CompactionPolicy(t, db, run.turn, run.name, run.policy)
		}
	}
	upgraded := reopenSchemaFixture(t, fixture.profile, fixture.node)
	checkPolicies(upgraded.Node)
	if err := upgraded.Node.Write(ctx, func(db *sql.Conn) error {
		if _, _, err := saveCompaction(ctx, db, protocol.ConversationCompactionSettings{Percent: 71}); err != nil {
			return err
		}
		legacyCtx := WithUser(ctx, protocol.User{ID: legacy})
		_, _, err := saveCompaction(legacyCtx, db, protocol.ConversationCompactionSettings{Revision: 1, Percent: 64})
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := upgraded.Close(); err != nil {
		t.Fatal(err)
	}

	reopened := reopenSchemaFixture(t, fixture.profile, fixture.node)
	checkPolicies(reopened.Node)
	if err := reopened.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := RecoverRuns(ctx, db, now); err != nil {
			return err
		}
		interrupted, err := ReadAgentRun(ctx, db, running)
		if err != nil {
			return err
		}
		if interrupted.Status != protocol.ConversationStatusInterrupted || interrupted.FinishedMs == nil || interrupted.Error == nil || interrupted.Error.Code != protocol.ErrorCodeOutcomeUnknown {
			return fmt.Errorf("in-flight turn was not interrupted: %+v", interrupted)
		}
		queue, err := ReadQueue(ctx, db, fixture.session.ID)
		if err != nil {
			return err
		}
		if !queue.Paused || queue.PauseReason == nil || *queue.PauseReason != protocol.ConversationQueuePauseInterrupted || len(queue.Items) != len(queued) {
			return fmt.Errorf("recovery did not hold only the queued input: %+v", queue)
		}
		for i, item := range queue.Items {
			if item.Turn != queued[i].turn || item.Guidance != nil || !item.Ready {
				return fmt.Errorf("recovered queue item %d = %+v, want ready turn %s without a recipient", i, item, queued[i].turn)
			}
		}
		if next, _, err := ClaimNext(ctx, db, now); err != nil || next != nil {
			return fmt.Errorf("paused queue was not idle: %v", err)
		}
		if _, err := queueFixtureCommand(ctx, db, reopened, fixture.node, "set_queue_paused", &protocol.CommandSetQueuePausedPayload{Session: fixture.session.ID, ExpectedRevision: queue.Revision, Paused: false}, now); err != nil {
			return err
		}
		for _, run := range queued {
			inv, _, err := ClaimNext(ctx, db, now)
			if err != nil {
				return err
			}
			if inv == nil || inv.Run.Turn != run.turn || inv.Message.Text != "Fixture input" {
				return fmt.Errorf("%s did not start with its preserved input", run.name)
			}
			if inv.User.ID != run.user || inv.StorageUser != run.user {
				return fmt.Errorf("%s claimed users = %s/%s, want %s", run.name, inv.User.ID, inv.StorageUser, run.user)
			}
			if inv.Compaction != run.policy {
				return fmt.Errorf("%s claimed policy = %+v, want %+v", run.name, inv.Compaction, run.policy)
			}
			if inv.Provider == nil || len(inv.Provider.Models) != 1 || inv.Provider.Models[0].ID != inv.Session.Config.Model {
				return fmt.Errorf("%s did not keep its captured model", run.name)
			}
			budget := AgentCompactionBudget(inv.Provider.Models[0], inv.Compaction)
			if budget == nil || budget.TriggerTokens != run.trigger {
				return fmt.Errorf("%s claimed budget = %+v, want trigger %d", run.name, budget, run.trigger)
			}
			if err := queueFixtureInput(ctx, db, fixture.session.ID, run.turn, now); err != nil {
				return err
			}
			finished, _, err := FinishTurn(ctx, db, run.turn, protocol.ConversationStatusCompleted, nil, now)
			if err != nil {
				return err
			}
			if finished.Status != protocol.ConversationStatusCompleted || finished.FinishedMs == nil || finished.Error != nil {
				return fmt.Errorf("%s did not complete: %+v", run.name, finished)
			}
		}
		if next, _, err := ClaimNext(ctx, db, now); err != nil || next != nil {
			return fmt.Errorf("settled queue was not idle: %v", err)
		}
		var pending int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE t.session=?1", fixture.session.ID.String()).Scan(&pending); err != nil {
			return err
		}
		if pending != 0 {
			return fmt.Errorf("settled queue still has %d pending rows", pending)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestV026CompactionRestore(t *testing.T) {
	t.Parallel()
	fixture := newV026CompactionFixture(t)
	turn := fixture.addRun(t, protocol.UserID{}, protocol.ConversationStatusQueued, 80)
	path := fixture.db.Path()
	fixture.close(t)
	ctx := t.Context()
	if _, err := PrepareRestored(ctx, fixture.profile, fixture.node, fixture.profile, t.TempDir()); err != nil {
		t.Fatal(err)
	}
	staged, err := OpenHandle(ctx, path)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := staged.Close(); err != nil {
			t.Error(err)
		}
	})
	checkV026CompactionPolicy(t, staged, turn, "restored queued default", CompactionPolicy{Percent: 80, UseDefault: true})
}

func TestCompactionUpgradePreservesExistingColumn(t *testing.T) {
	t.Parallel()
	fixture := newV026CompactionFixture(t)
	custom := fixture.addRun(t, fixture.owner, protocol.ConversationStatusCompleted, 80)
	defaulted := fixture.addRun(t, fixture.owner, protocol.ConversationStatusCompleted, 80)
	schemaExecute(t, fixture.db, "ALTER TABLE agent_runs ADD COLUMN compaction_default INTEGER NOT NULL DEFAULT 0 CHECK(compaction_default IN (0,1))")
	schemaExecute(t, fixture.db, "UPDATE agent_runs SET compaction_default=1 WHERE turn=?1", defaulted.String())
	fixture.close(t)

	upgraded := reopenSchemaFixture(t, fixture.profile, fixture.node)
	if got := schemaInteger(t, upgraded.Node, "SELECT count(*) FROM sqlite_schema WHERE name='model_catalog_model'"); got != 1 {
		t.Fatal("missing index was not upgraded")
	}
	checkV026CompactionPolicy(t, upgraded.Node, custom, "existing explicit 80", CompactionPolicy{Percent: 80})
	checkV026CompactionPolicy(t, upgraded.Node, defaulted, "existing default 80", CompactionPolicy{Percent: 80, UseDefault: true})
}

type v026CompactionFixture struct {
	db       *Handle
	profile  string
	node     protocol.NodeID
	owner    protocol.UserID
	session  protocol.Session
	provider []byte
}

func newV026CompactionFixture(t *testing.T) v026CompactionFixture {
	t.Helper()
	directory := databaseDirectory(t)
	fixture := v026CompactionFixture{profile: filepath.Dir(directory), node: protocol.NodeID{26}, owner: protocol.NewUserID(), session: protocol.DefaultSession()}
	// The SQL fixture is unmodified from v0.2.6 (a285ba78a45b).
	schema, err := os.ReadFile(filepath.Join("testdata", "v0.2.6-node-schema.sql"))
	if err != nil {
		t.Fatal(err)
	}
	ctx := t.Context()
	fixture.db, err = OpenHandle(ctx, filepath.Join(directory, "node.turso"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { fixture.close(t) })
	schemaExecute(t, fixture.db, string(schema))
	schemaExecute(t, fixture.db, "PRAGMA application_id=0x5341494c; PRAGMA user_version=1; INSERT INTO counters VALUES('sessions',1)")
	schemaExecute(t, fixture.db, "INSERT INTO node(singleton,identity,plugin_defaults,tool_defaults,tool_metadata_names) VALUES(1,?1,1,1,1)", fixture.node[:])
	schemaExecute(t, fixture.db, "INSERT INTO users(id,name,administrator,local) VALUES(?1,'Local user',1,1)", fixture.owner.String())

	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Revision, provider.Name = protocol.NewProviderID(), 1, "Fixture"
	provider.Api, provider.Authentication = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey
	provider.Endpoint, provider.Enabled, provider.DefaultModel = "http://fixture.invalid/v1", true, "fixture"
	model := protocol.DefaultConversationModel()
	model.ID, model.Context, model.Output, model.Tools = provider.DefaultModel, 1_000_000, 8_000, true
	provider.Models = []protocol.ConversationModel{model}
	fixture.provider = v026CompactionJSON(t, provider)
	project := protocol.NewProjectID()
	path := filepath.Join(fixture.profile, "project")
	appearance := v026CompactionJSON(t, protocol.DefaultProjectsAppearance())
	session := &fixture.session
	session.ID, session.Project, session.Worktree, session.Revision = protocol.NewSessionID(), &project, protocol.NewWorktreeID(), 1
	session.Config.Provider, session.Config.Model = provider.ID, model.ID
	session.Config.Mode, session.Config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
	session.Roles.Execution = protocol.ConversationExecutionModel
	schemaExecute(t, fixture.db, "INSERT INTO projects(id,name,path,appearance,owner,visibility) VALUES(?1,'Fixture',?2,?3,?4,'shared')", project.String(), path, appearance, fixture.owner.String())
	schemaExecute(t, fixture.db, "INSERT INTO worktrees(id,project,owner,path,main) VALUES(?1,?2,?3,?4,1)", session.Worktree.String(), project.String(), fixture.owner.String(), path)
	schemaExecute(t, fixture.db, "INSERT INTO sessions(rowid,id,project,owner,worktree,revision) VALUES(1,?1,?2,?3,?4,1)", session.ID.String(), project.String(), fixture.owner.String(), session.Worktree.String())
	if err := fixture.db.Write(ctx, func(db *sql.Conn) error {
		if err := initializeLocalModels(ctx, db); err != nil {
			return err
		}
		return insertSessionRevision(ctx, db, *session)
	}); err != nil {
		t.Fatal(err)
	}
	return fixture
}

func (fixture v026CompactionFixture) close(t *testing.T) {
	t.Helper()
	if err := fixture.db.Close(); err != nil {
		t.Fatal(err)
	}
}

func (fixture v026CompactionFixture) addRun(t *testing.T, user protocol.UserID, status protocol.ConversationStatus, percent uint8) protocol.TurnID {
	t.Helper()
	turn := protocol.NewTurnID()
	message := protocol.ConversationInput{Text: "Fixture input"}
	request := protocol.NewRequest(fixture.node, protocol.Command{Kind: "submit_turn", Data: &protocol.CommandSubmitTurnPayload{Session: fixture.session.ID, ExpectedRevision: fixture.session.Revision, Message: message}})
	requestBody := v026CompactionJSON(t, request)
	roles := v026CompactionJSON(t, fixture.session.Roles)
	ctx := t.Context()
	if err := fixture.db.Write(ctx, func(db *sql.Conn) error {
		sequence, err := NextCounter(ctx, db, CounterTurns)
		if err != nil {
			return err
		}
		run := protocol.DefaultConversationRun()
		run.Turn, run.Session, run.Worktree = turn, fixture.session.ID, fixture.session.Worktree
		run.Kind, run.Status, run.Revision, run.Sequence = protocol.ConversationRunKindTask, status, fixture.session.Revision, sequence
		var input []byte
		if status != protocol.ConversationStatusQueued {
			run.StartedMs = new(int64(1000))
			input, err = encode(message)
			if err != nil {
				return err
			}
		}
		if status == protocol.ConversationStatusCompleted {
			run.FinishedMs = new(int64(2000))
		}
		body, err := encode(run)
		if err != nil {
			return err
		}
		principal := sql.NullString{String: user.String(), Valid: user != (protocol.UserID{})}
		if _, err = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", fixture.node[:], request.ID.String(), requestBody); err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "INSERT INTO turns(rowid,id,kind,plugins,session,revision,request,caller,user,roles) VALUES(?1,?2,'task',X'5B5D',?3,?4,?5,?6,?7,?8)", sequence, turn.String(), run.Session.String(), run.Revision, request.ID.String(), fixture.node[:], principal, roles); err != nil {
			return err
		}
		if principal.Valid {
			if _, err = db.ExecContext(ctx, "INSERT INTO identity_turn_origins VALUES(?1,?2)", turn.String(), principal.String); err != nil {
				return err
			}
		}
		active := status != protocol.ConversationStatusCompleted
		if _, err = db.ExecContext(ctx, "INSERT INTO agent_runs(turn,ready,body,provider,hooks,compaction_percent,input) VALUES(?1,?2,?3,?4,?5,?6,?7)", turn.String(), active, body, fixture.provider, defaultHooksColumn, percent, input); err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "INSERT INTO conversation_turns VALUES(?1,?2)", run.Session.String(), turn.String()); err != nil {
			return err
		}
		if !active {
			return nil
		}
		if _, err = db.ExecContext(ctx, "INSERT INTO agent_pending(turn,position,revision,request,caller) VALUES(?1,?2,1,?3,?4)", turn.String(), sequence, request.ID.String(), fixture.node[:]); err != nil {
			return err
		}
		return bumpQueue(ctx, db, run.Session)
	}); err != nil {
		t.Fatal(err)
	}
	return turn
}

func v026CompactionJSON(t *testing.T, value any) []byte {
	t.Helper()
	body, err := encode(value)
	if err != nil {
		t.Fatal(err)
	}
	return body
}

func checkV026CompactionPolicy(t *testing.T, handle *Handle, turn protocol.TurnID, name string, want CompactionPolicy) {
	t.Helper()
	ctx := t.Context()
	if err := handle.Read(ctx, func(db *sql.Conn) error {
		settings, err := readTurnSettings(ctx, db, turn)
		if err != nil {
			return err
		}
		if settings.compaction != want {
			return fmt.Errorf("stored policy = %+v, want %+v", settings.compaction, want)
		}
		return nil
	}); err != nil {
		t.Fatalf("%s: %v", name, err)
	}
}
