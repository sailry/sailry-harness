package node

import (
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func testPluginMessageCommandOutputs(t *testing.T) {
	t.Run("queued result retains the captured session", func(t *testing.T) {
		session := protocol.NewSessionID()
		child := protocol.NewRequest(protocol.NodeID{1}, protocol.Command{Kind: "call_plugin"})
		child.Plugin = &protocol.PluginContext{Session: &session}
		turn := protocol.DefaultQueuedTurn()
		turn.ID, turn.Session, turn.Request = protocol.NewTurnID(), session, child.ID
		turn.Kind = protocol.ConversationRunKindTask
		turn.Config.Provider, turn.Config.Model = protocol.NewProviderID(), "fixture"
		turn.Config.Mode, turn.Config.Permission = protocol.WorkModeCode, protocol.PermissionProject
		body, err := json.Marshal(protocol.Output{Kind: "queued_turn", Data: turn})
		if err != nil {
			t.Fatal(err)
		}
		for _, value := range []any{json.RawMessage(body), new(json.RawMessage(body))} {
			output, err := pluginMessageCommandOutput(child, protocol.Output{Kind: "plugin_result", Data: value})
			if err != nil || output.Kind != "queued_turn" {
				t.Fatalf("valid command turn: %#v %v", output, err)
			}
			captured, ok := dispatchPayload[protocol.QueuedTurn](output.Data)
			if !ok || captured.Session != session || captured.Request != child.ID {
				t.Fatalf("command turn capture changed: %#v", output)
			}
		}
		other := protocol.NewSessionID()
		child.Plugin.Session = &other
		if _, err := pluginMessageCommandOutput(child, protocol.Output{Kind: "plugin_result", Data: json.RawMessage(body)}); !pluginMessageFault(err, protocol.ErrorCodeWrongTarget) {
			t.Fatalf("cross-session command turn: %v", err)
		}
		child.Plugin = nil
		if _, err := pluginMessageCommandOutput(child, protocol.Output{Kind: "plugin_result", Data: json.RawMessage(body)}); !pluginMessageFault(err, protocol.ErrorCodeWrongTarget) {
			t.Fatalf("unscoped command turn: %v", err)
		}
		if _, err := pluginMessageCommandOutput(child, protocol.Output{Kind: "plugin_result", Data: json.RawMessage(`{"kind":"queued_turn","data":{}}`)}); !pluginMessageFault(err, protocol.ErrorCodeInvalidRequest) {
			t.Fatalf("malformed command turn: %v", err)
		}
		if _, err := pluginMessageCommandOutput(child, protocol.Output{Kind: "snapshot"}); !pluginMessageFault(err, protocol.ErrorCodeInternal) {
			t.Fatalf("unexpected callback output: %v", err)
		}
		value := json.RawMessage(`{"intent":"open-goal","value":{"id":"fixture"}}`)
		output, err := pluginMessageCommandOutput(child, protocol.Output{Kind: "plugin_result", Data: value})
		if err != nil || output.Kind != "plugin_result" || string(output.Data.(json.RawMessage)) != string(value) {
			t.Fatalf("ordinary plugin result changed: %#v %v", output, err)
		}
	})
	t.Run("durable replay precedes live package and session resolution", func(t *testing.T) {
		node := nodeFixture(t)
		session := sessionFixture(t, node)
		entry := "dev.sailry.platform/host/main.js"
		source := t.TempDir()
		for name, content := range map[string]string{
			"plugin.json": `{"$schema":"` + plugins.ManifestSchema + `","name":"message-command-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["storage.read","storage.write"],"tools":[{"name":"command","description":"Count a run","handler":{"name":"command","parameters":{"type":"object"}}}],"host":{"entry":"` + entry + `","resources":["` + entry + `"],"handlers":["command"],"commands":[{"name":"fixture","handler":"command"}]}}}}`,
			entry: `import {getValue,setValue,completeRequest} from "sailry/sdk";
export async function command(input){const current=await getValue("runs");const count=current.present?current.value+1:1;const saved=await completeRequest(setValue("runs",count,current.revision));if(saved.Err)throw new Error(saved.Err.message);return {count,arguments:input.arguments,message:input.message};}`,
		} {
			filename := filepath.Join(source, filepath.FromSlash(name))
			if err := os.MkdirAll(filepath.Dir(filename), 0o700); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filename, []byte(content), 0o600); err != nil {
				t.Fatal(err)
			}
		}
		info, err := node.plugins.manager.Install(t.Context(), source, "", "message-command-fixture")
		if err != nil {
			t.Fatal(err)
		}
		if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
			var err error
			info, err = store.InstallPlugin(t.Context(), db, info, 0, true)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		message := protocol.DefaultConversationInput()
		message.Text = "\u3000/fixture\u2003 original input \n"
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
			Session: session.ID, ExpectedRevision: session.Revision, Message: message,
		}})
		first := dispatchFixture(t, node, node.ID(), request)
		if first.Fault != nil || first.Output.Kind != "plugin_result" {
			t.Fatalf("slash command did not run without a model provider: %#v", first)
		}
		body, err := json.Marshal(first.Output.Data)
		if err != nil {
			t.Fatal(err)
		}
		var captured struct {
			Count     int                        `json:"count"`
			Arguments string                     `json:"arguments"`
			Message   protocol.ConversationInput `json:"message"`
		}
		if json.Unmarshal(body, &captured) != nil || captured.Count != 1 || captured.Arguments != "original input" || captured.Message.Text != message.Text {
			t.Fatalf("command callback did not receive the original message: %s", body)
		}
		replay := func(label string) {
			t.Helper()
			repeated := dispatchFixture(t, node, node.ID(), request)
			encoded, err := json.Marshal(repeated.Output.Data)
			if err != nil || repeated.Fault != nil || repeated.Output.Kind != "plugin_result" || string(encoded) != string(body) {
				t.Fatalf("%s lost original durable result: %#v %v", label, repeated, err)
			}
		}
		commandFixture(t, node, protocol.Command{Kind: "rename_session", Data: protocol.CommandRenameSessionPayload{
			Session: session.ID, ExpectedRevision: session.Revision, Title: "Changed after command",
		}})
		replay("changed session revision")
		if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
			var err error
			info, err = store.SetPluginEnabled(t.Context(), db, info.Summary.Name, info.Summary.Revision, false)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		replay("disabled package")
		if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
			_, err := store.RemovePlugin(t.Context(), db, info.Summary.Name, info.Summary.Revision)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		replay("removed package")
		changed := request
		message.Text = "/fixture changed"
		changed.Command.Data = protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: message}
		if _, err := node.Dispatch(t.Context(), node.ID(), changed); !pluginMessageFault(err, protocol.ErrorCodeConflict) {
			t.Fatalf("same identifier accepted another request body: %v", err)
		}
		var count int
		if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
			value, err := store.ReadPluginValue(t.Context(), db, plugins.PackageReference(info), "runs")
			if err != nil {
				return err
			}
			return json.Unmarshal(value.Value, &count)
		}); err != nil || count != 1 {
			t.Fatalf("durable command replay repeated a side effect: %d %v", count, err)
		}
		pending := request
		pending.ID = protocol.NewRequestID()
		pendingBody, err := json.Marshal(pending)
		if err != nil {
			t.Fatal(err)
		}
		if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
			_, err := admission.Admit(t.Context(), db, node.ID(), pending, pendingBody)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		unsettled := dispatchFixture(t, node, node.ID(), pending)
		if unsettled.Fault == nil || unsettled.Fault.Code != protocol.ErrorCodeOutcomeUnknown {
			t.Fatalf("admitted command was rerouted after package removal: %#v", unsettled)
		}
	})
}

func pluginMessageFault(err error, code protocol.ErrorCode) bool {
	var rejected *protocol.Fault
	return errors.As(err, &rejected) && rejected.Code == code
}
