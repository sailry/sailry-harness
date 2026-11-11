//go:build postmerge

package node

import (
	"encoding/json"
	"strconv"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestScheduledTaskTurnInput describes, from the scheduling side, what the
// plugin branch owns in pluginTurnInput. It is excluded from the ordinary
// build because it fails until that branch is merged:
//
//	go test -tags postmerge ./internal/node -run TestScheduledTaskTurnInput
//
// The scheduled-tasks tool stores what its initialize handler staged, and
// the fire starts the session with that team. A conversation that runs a
// named team must therefore hand the team to initialize, as the reference
// does (`{"id":…,"revision":"<revision>"}` for a package that reads the
// conversation), or the task is stored and run with the default team.
func TestScheduledTaskTurnInput(t *testing.T) {
	t.Parallel()
	info := protocol.DefaultPluginInfo()
	info.Summary.Name = "scheduled-tasks"
	info.Extension = new(protocol.DefaultPluginExtension())
	team := protocol.TeamReference{ID: protocol.NewTeamID(), Revision: 3}
	invocation := &store.AgentInvocation{}
	invocation.Turn = protocol.DefaultQueuedTurn()
	invocation.Turn.Config.Mode, invocation.Turn.Config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
	invocation.Turn.Roles.Execution, invocation.Turn.Roles.Team = protocol.ConversationExecutionTeam, &team
	staged := func(t *testing.T) map[string]json.RawMessage {
		t.Helper()
		body, err := json.Marshal(pluginTurnInput(invocation, info, nil))
		if err != nil {
			t.Fatal(err)
		}
		var input map[string]json.RawMessage
		if err := json.Unmarshal(body, &input); err != nil {
			t.Fatal(err)
		}
		return input
	}
	t.Run("a package that reads the conversation is given the team", func(t *testing.T) {
		info.Extension.Actions = []protocol.PluginAction{protocol.PluginActionReadConversation}
		var given struct {
			ID       protocol.TeamID `json:"id"`
			Revision string          `json:"revision"`
		}
		if err := json.Unmarshal(staged(t)["team"], &given); err != nil || given.ID != team.ID || given.Revision != strconv.FormatUint(team.Revision, 10) {
			t.Fatalf("team = %s, %v", staged(t)["team"], err)
		}
	})
	t.Run("a package that does not read the conversation is not", func(t *testing.T) {
		info.Extension.Actions = nil
		if team := string(staged(t)["team"]); team != "null" {
			t.Fatalf("team = %s", team)
		}
	})
}
