package node

import (
	"encoding/json"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginTurnInput(t *testing.T) {
	delegate, apply, query, transfer := protocol.ToolOperationDelegateAgent, protocol.ToolOperationApplyRoles, protocol.ToolOperationQueryDatabase, protocol.ToolOperationTransferSsh
	options := json.RawMessage(`{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","properties":{"depth":{"type":"string","default":"quick"},"strict":{"type":"boolean"}},"required":[],"additionalProperties":false}`)
	extension := protocol.DefaultPluginExtension()
	extension.Actions = []protocol.PluginAction{protocol.PluginActionReadConversation, protocol.PluginActionReadMessage}
	extension.ConversationOptions = &options
	extension.Tools = []protocol.ToolDeclaration{
		{Name: "delegate", Handler: &protocol.ToolHandler{Name: "delegate", Operation: &delegate}},
		{Name: "roster", Handler: &protocol.ToolHandler{Name: "roster", Operation: &apply}},
		{Name: "report", Handler: &protocol.ToolHandler{Name: "report", Flow: &protocol.ToolFlow{Operations: []protocol.ToolOperation{query, transfer}}}},
		{Name: "plain", Handler: &protocol.ToolHandler{Name: "plain"}},
	}
	info := protocol.PluginInfo{Summary: protocol.PluginSummary{Name: "review-fixture", Digest: "digest"}, Extension: &extension}
	provider := protocol.NewProviderID()
	withTools, withoutTools := protocol.NewRoleID(), protocol.NewRoleID()
	team := protocol.TeamReference{ID: protocol.NewTeamID(), Revision: 7}
	invocation := func() *store.AgentInvocation {
		value := &store.AgentInvocation{Turn: protocol.DefaultQueuedTurn(), Run: protocol.DefaultConversationRun()}
		value.Turn.Revision = 42
		value.Turn.Config.Mode, value.Turn.Config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
		value.Turn.Config.Options = map[string]map[string]json.RawMessage{
			"review-fixture": {"strict": json.RawMessage("true"), "retired": json.RawMessage(`"x"`)},
			"other":          {"depth": json.RawMessage(`"deep"`)},
		}
		value.Turn.Roles.Team = &team
		value.Turn.Roles.Providers = []protocol.ConversationProvider{{ID: provider, Models: []protocol.ConversationModel{{ID: "tools", Tools: true}, {ID: "plain"}}}}
		value.Turn.Roles.Profiles = []protocol.RoleProfile{
			{ID: withTools, Key: "review", Name: "Review", Description: " ", Instructions: strings.Repeat("字", 1100), ReadOnly: true, Model: &protocol.RoleModel{Provider: provider, Model: "plain"}},
			{ID: withoutTools, Key: "build", Name: "Build", Description: "Builds the task", Model: &protocol.RoleModel{Provider: provider, Model: "plain"}},
			{ID: protocol.NewRoleID(), Key: "lead", Name: "Lead"},
		}
		value.Turn.Roles.Models = map[protocol.RoleID]protocol.RoleModel{withTools: {Provider: provider, Model: "tools"}}
		value.Message.Text = strings.Repeat("记", 4096)
		return value
	}
	names := []string{"delegate", "roster", "report", "plain"}
	build := func(t *testing.T, value *store.AgentInvocation, names []string) map[string]any {
		t.Helper()
		input, err := pluginTurnInput(value, info, names, pluginSelectedOperations(info, names))
		if err != nil {
			t.Fatal(err)
		}
		body, err := json.Marshal(input)
		if err != nil {
			t.Fatal(err)
		}
		var decoded map[string]any
		if err := json.Unmarshal(body, &decoded); err != nil {
			t.Fatal(err)
		}
		return decoded
	}

	t.Run("selected operations include handler and flow operations", func(t *testing.T) {
		selected := pluginSelectedOperations(info, names)
		for _, operation := range []protocol.ToolOperation{delegate, apply, query, transfer} {
			if !slices.Contains(selected, operation) {
				t.Fatalf("%s missing from %v", operation, selected)
			}
		}
		if selected := pluginSelectedOperations(info, []string{"plain"}); len(selected) != 0 {
			t.Fatalf("unselected operations leaked: %v", selected)
		}
	})
	t.Run("captured roster, team, revision and options", func(t *testing.T) {
		input := build(t, invocation(), names)
		roles := input["roles"].([]any)
		if len(roles) != 3 {
			t.Fatalf("roles = %v", roles)
		}
		first := roles[0].(map[string]any)
		if first["key"] != "review" || first["name"] != "Review" || first["read_only"] != true || first["tools"] != true ||
			first["description"] != strings.Repeat("字", 1024) {
			t.Fatalf("first role = %v", first)
		}
		if second := roles[1].(map[string]any); second["description"] != "Builds the task" || second["tools"] != false || second["read_only"] != false {
			t.Fatalf("second role = %v", second)
		}
		if third := roles[2].(map[string]any); third["tools"] != true {
			t.Fatalf("a role following the lead's model can call tools: %v", third)
		}
		if got := input["team"].(map[string]any); got["id"] != team.ID.String() || got["revision"] != "7" {
			t.Fatalf("team = %v", got)
		}
		if input["session_revision"] != "42" || input["automatic"] != false || input["execution"] != string(protocol.ConversationExecutionTeam) || input["resources"] != true {
			t.Fatalf("input = %v", input)
		}
		if got := input["options"].(map[string]any); len(got) != 2 || got["depth"] != "quick" || got["strict"] != true {
			t.Fatalf("options = %v", got)
		}
		message := input["message"].(string)
		if len(message) != 4095 || !strings.HasPrefix(invocation().Message.Text, message) {
			t.Fatalf("message has %d bytes", len(message))
		}
		if got := input["names"].([]any); len(got) != 4 || got[0] != plugins.ToolAlias("review-fixture", "delegate") {
			t.Fatalf("names = %v", got)
		}
	})
	t.Run("roster and revision follow the selected operations", func(t *testing.T) {
		input := build(t, invocation(), []string{"plain", "report"})
		if len(input["roles"].([]any)) != 0 || input["session_revision"] != nil {
			t.Fatalf("input = %v", input)
		}
	})
	t.Run("automatic, notice and delegated turns keep the message private", func(t *testing.T) {
		automatic := invocation()
		automatic.Run.Continuation = &protocol.ConversationContinuation{}
		if input := build(t, automatic, names); input["automatic"] != true || input["notice"] != false || input["message"] != nil {
			t.Fatalf("automatic input = %v %v %v", input["automatic"], input["notice"], input["message"] != nil)
		}
		// The goal loop counts a notice turn as automatic and names it apart.
		notice := invocation()
		notice.Run.Notice = true
		if input := build(t, notice, names); input["automatic"] != true || input["notice"] != true || input["message"] != nil {
			t.Fatalf("notice input = %v %v %v", input["automatic"], input["notice"], input["message"] != nil)
		}
		child := invocation()
		delegation := &protocol.Delegation{}
		child.Session.Delegation = &delegation
		if input := build(t, child, names); input["message"] != nil {
			t.Fatal("delegated input exposed its message")
		}
		blank := invocation()
		blank.Message.Text = " \n\t"
		if input := build(t, blank, names); input["message"] != nil {
			t.Fatal("blank message was exposed")
		}
	})
	t.Run("conversation fields need the conversation action", func(t *testing.T) {
		private := extension
		private.Actions = nil
		limited := protocol.PluginInfo{Summary: info.Summary, Extension: &private}
		input, err := pluginTurnInput(invocation(), limited, names, pluginSelectedOperations(limited, names))
		if err != nil {
			t.Fatal(err)
		}
		for _, key := range []string{"config", "execution", "team", "models", "message"} {
			if input[key] != nil {
				t.Fatalf("%s = %v", key, input[key])
			}
		}
	})
	t.Run("resources belong to the assistant's package", func(t *testing.T) {
		value := invocation()
		value.Turn.Config.Assistant = &protocol.PluginConversationBinding{Package: plugins.PackageReference(info), ID: "assistant"}
		if input := build(t, value, names); input["resources"] != true {
			t.Fatal("the assistant's own package lost its resources")
		}
		value.Turn.Config.Assistant.Package.Name = "another"
		if input := build(t, value, names); input["resources"] != false {
			t.Fatal("an imported package was given the assistant's resources")
		}
	})
	t.Run("options violating the schema fail the turn", func(t *testing.T) {
		value := invocation()
		value.Turn.Config.Options["review-fixture"]["depth"] = json.RawMessage("3")
		if _, err := pluginTurnInput(value, info, names, pluginSelectedOperations(info, names)); err == nil {
			t.Fatal("invalid options were accepted")
		}
	})
}
