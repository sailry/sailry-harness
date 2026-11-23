package store

import (
	"context"
	"database/sql"
	"fmt"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestGuideQueuedTurnRevalidatesReferences(t *testing.T) {
	t.Parallel()
	storage, node, session, running, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := context.Background()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if err := queueFixtureInput(ctx, db, session.ID, running, now); err != nil {
			return err
		}

		// Create a role and attach it to the session after the running turn started.
		role := protocol.DefaultRoleProfile()
		role.ID, role.Key, role.Name, role.ReadOnly = protocol.NewRoleID(), "auditor", "Auditor", true
		role.Model = &protocol.RoleModel{Provider: session.Config.Provider, Model: session.Config.Model}
		role, err := putRole(ctx, db, role, 0)
		if err != nil {
			return err
		}
		team := protocol.DefaultTeamProfile()
		team.ID, team.Key, team.Name, team.Instructions, team.Members = protocol.NewTeamID(), "audit", "Audit", "Audit instructions", []protocol.RoleID{role.ID}
		team, err = putTeam(ctx, db, team, 0)
		if err != nil {
			return err
		}
		current, err := ReadSession(ctx, db, session.ID)
		if err != nil {
			return err
		}
		current.Roles, err = CaptureSessionRoles(ctx, db, node, protocol.ConversationExecutionTeam, &protocol.TeamSelection{ID: team.ID, Revision: &team.Revision}, nil)
		if err != nil {
			return err
		}
		revised, _, err := reviseSession(ctx, db, current)
		if err != nil {
			return err
		}
		current = revised.Data.(protocol.Session)

		// Queue an input referencing the new role (valid under current session settings).
		queuedOut, err := queueFixtureCommand(ctx, db, storage, node, "queue_turn", &protocol.CommandQueueTurnPayload{
			Session: session.ID, ExpectedRevision: current.Revision,
			Message: protocol.ConversationInput{
				Text: "Use @auditor",
				References: []protocol.ConversationReference{{
					Label:  "@auditor",
					Target: protocol.ConversationReferenceTarget{Kind: "agent", Data: protocol.RoleReference{ID: role.ID, Revision: role.Revision}},
				}},
			},
		}, now)
		if err != nil {
			return err
		}
		queued := queuedOut.Data.(protocol.QueuedTurn).ID
		_, item, err := queueFixtureItem(ctx, db, session.ID, queued)
		if err != nil || item == nil {
			return fmt.Errorf("queued item missing: %v", err)
		}

		// Promoting the queued input to guide the running turn must reject the
		// reference that is absent from the running turn's frozen roster.
		_, err = queueFixtureCommand(ctx, db, storage, node, "guide_queued_turn", &protocol.CommandGuideQueuedTurnPayload{
			Turn: queued, Recipient: running, ExpectedRevision: item.Revision,
		}, now)
		if err == nil || !strings.Contains(err.Error(), "frozen session roster") {
			return fmt.Errorf("expected invalid reference rejection, got %v", err)
		}

		// The rejected promotion must leave the queued item unaddressed with its own frozen roles.
		_, afterItem, err := queueFixtureItem(ctx, db, session.ID, queued)
		if err != nil || afterItem == nil || afterItem.Guidance != nil {
			return fmt.Errorf("rejected guide mutated queue item: %+v, err=%v", afterItem, err)
		}
		capture, err := readTurnCapture(ctx, db, queued)
		if err != nil || len(capture.Roles.Profiles) != 1 || capture.Roles.Profiles[0].ID != role.ID {
			return fmt.Errorf("rejected guide mutated queued turn capture: %+v, err=%v", capture.Roles, err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
