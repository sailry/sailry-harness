package store

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func testPluginCardAuthority(t *testing.T) {
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		info := pluginTestInfo("card-fixture", 'c')
		var extension protocol.PluginExtension
		if err := json.Unmarshal([]byte(`{
			"api_version":"v1","actions":["storage.write"],
			"host":{"entry":"dev.sailry.platform/host.js","resources":["dev.sailry.platform/host.js"],"handlers":["connection"]},
			"desktop":{"resources":["dev.sailry.platform/card.js"]},
			"tools":[{"name":"connection","presentation":"content","handler":{"name":"connection","read_only":true,"parameters":{"type":"object","properties":{},"additionalProperties":false}}}],
			"connectors":[{"id":"service","display":{"label":"Service"},"accounts":"single","origins":["https://example.test"],"auth":[{"kind":"mcp_oauth","server":"input"}],"inject":[]}],
			"cards":[{"key":"connection","entry":"dev.sailry.platform/card.js","tool":"connection","mcp_server":"input",
				"props_schema":{"type":"object","additionalProperties":false,"properties":{"configured":{"type":"boolean"}},"required":["configured"]},
				"capabilities":{"browser":[{"name":"login","origins":["https://example.test"],"source":{"kind":"mcp_authorization"},"modes":["external"]}]}}]
		}`), &extension); err != nil {
			return err
		}
		info.Extension = &extension
		info.Mcp = []protocol.PluginMcp{{Name: "input", Transport: protocol.PluginMcpTransportStreamableHttp, Allowed: true, Availability: protocol.PluginMcpAvailabilityAvailable}}
		if err := plugins.ValidateCards(extension); err != nil {
			return err
		}
		info, err := InstallPlugin(ctx, db, info, 0, true)
		if err != nil {
			return err
		}
		reference := plugins.PackageReference(info)
		references, err := encode([]protocol.PluginReference{reference})
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "UPDATE turns SET plugins=?2 WHERE id=?1", turn.String(), references); err != nil {
			return err
		}
		alias, call := plugins.ToolAlias(info.Summary.Name, "connection"), "card-call"
		identity := plugins.ToolCallIdentity(turn, reference, alias, call)
		captured := protocol.DefaultPluginContext()
		captured.Package, captured.Session, captured.Turn, captured.Worktree = reference, &session.ID, &turn, &session.Worktree
		body := plugins.StampCards(json.RawMessage(`{"isError":true,"error":{"code":"not_configured","message":"Connect"},"sailry_content":{"version":1,"blocks":[{"kind":"card","key":"connection","props":{"configured":false},"fallback":{"label":"Connect"}}]}}`), node, captured, info, identity)
		receipt := protocol.PluginCardReceipt{Session: session.ID, Turn: turn, Call: identity, Block: 0}
		if _, _, err := CapturedPluginCard(ctx, db, receipt); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
			return fmt.Errorf("card without canonical evidence was accepted: %v", err)
		}
		canonicalBody, err := encode(agentJSON(body))
		if err != nil {
			return err
		}
		response := canonical.ToolResponse{ID: call, Name: alias, Format: canonical.JSON, Status: canonical.ToolErrorStatus, Revision: fmt.Sprintf("%x", blake3.Sum256(canonicalBody)), Result: canonical.Payload{Value: body}}
		record := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.User, canonical.Block{Index: 0, Kind: "tool_response", Data: &response})), now)
		record.Provenance.Author = "tool"
		if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
			return err
		}
		if _, _, err := CapturedPluginCard(ctx, db, receipt); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
			return fmt.Errorf("unmatched canonical response admitted a card: %v", err)
		}
		record = canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_request", Data: &canonical.ToolRequest{ID: call, Name: alias, Arguments: json.RawMessage(`{}`)}})), now)
		record.Provenance.Author = "sailry"
		if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
			return err
		}
		record = canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.User, canonical.Block{Index: 0, Kind: "tool_response", Data: &response})), now)
		record.Provenance.Author = "tool"
		if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
			return err
		}
		if _, summary, err := CapturedPluginCard(ctx, db, receipt); err != nil || summary.Package != reference || summary.Receipt != receipt {
			return fmt.Errorf("canonical admitted card was unavailable: %v", err)
		}
		scope := protocol.DefaultPluginContext()
		scope.Package, scope.Session, scope.Worktree = reference, &session.ID, &session.Worktree
		scope.Surface = protocol.PluginDesktopSurface{Kind: "card", Data: receipt}
		read := protocol.Command{Kind: "read_plugin_connection", Data: &protocol.CommandReadPluginConnectionPayload{Package: reference, Card: "connection"}}
		for _, command := range []protocol.Command{
			read,
			{Kind: "list_connections"},
			{Kind: "list_connector_types"},
			{Kind: "submit_connector_input"},
			{Kind: "cancel_connector_login"},
		} {
			request := protocol.NewRequest(node, command)
			request.Plugin = &scope
			if err := CheckPluginCardRequest(ctx, db, node, protocol.NodeID{2}, request); err != nil {
				return fmt.Errorf("declared controller card action was refused: %v", err)
			}
		}
		input := protocol.CommandStartConnectorLoginPayload{Package: reference, Connector: "service", Method: protocol.PluginConnectorKindMcpOauth, Input: protocol.DefaultPluginConnectorInput()}
		request := protocol.NewRequest(node, protocol.Command{Kind: "start_connector_login", Data: &input})
		request.Plugin = &scope
		if err := CheckPluginCardRequest(ctx, db, node, node, request); err != nil {
			return fmt.Errorf("declared card authorization was refused: %v", err)
		}
		for _, scenario := range []string{"wrong connector", "wrong method", "fields", "secrets", "registration", "wrong package"} {
			changed := input
			switch scenario {
			case "wrong connector":
				changed.Connector = "other"
			case "wrong method":
				changed.Method = protocol.PluginConnectorKindSecret
			case "fields":
				changed.Input.Fields = map[string]string{"token": "private"}
			case "secrets":
				changed.Input.Secrets = map[string]protocol.Secret{"token": protocol.NewSecret("private")}
			case "registration":
				changed.Input.Registration = true
			case "wrong package":
				changed.Package.Name = "other"
			}
			request.Command.Data = &changed
			if err := CheckPluginCardRequest(ctx, db, node, node, request); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
				return fmt.Errorf("%s escaped the card login fence: %v", scenario, err)
			}
		}
		for _, kind := range []string{"write_plugin_value", "plugin_transaction", "save_plugin_settings", "call_plugin", "read_plugin_callback", "read_plugin_view"} {
			request.Command = protocol.Command{Kind: kind}
			if err := CheckPluginCardRequest(ctx, db, node, node, request); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
				return fmt.Errorf("card gained generic %s authority: %v", kind, err)
			}
		}
		request.Command = read
		for _, scenario := range []string{"turn", "invocation", "session", "worktree", "package", "receipt"} {
			changed := scope
			switch scenario {
			case "turn":
				changed.Turn = &turn
			case "invocation":
				identity := protocol.NewRequestID()
				changed.Invocation = &identity
			case "session":
				changed.Session = nil
			case "worktree":
				other := protocol.NewWorktreeID()
				changed.Worktree = &other
			case "package":
				changed.Package.SettingsRevision++
			case "receipt":
				other := receipt
				other.Call = protocol.NewRequestID()
				changed.Surface.Data = other
			}
			request.Plugin = &changed
			if err := CheckPluginCardRequest(ctx, db, node, node, request); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
				return fmt.Errorf("changed %s retained card authority: %v", scenario, err)
			}
		}
		request.Plugin = &scope
		if err := CheckPluginCardRequest(ctx, db, protocol.NodeID{3}, node, request); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
			return fmt.Errorf("card on another execution Node was accepted: %v", err)
		}
		if _, err := SetPluginEnabled(ctx, db, info.Summary.Name, info.Summary.Revision, false); err != nil {
			return err
		}
		if err := CheckPluginCardRequest(ctx, db, node, node, request); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
			return fmt.Errorf("disabled package retained card authority: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Run("public connection status", testPluginConnectionStatus)
}
