package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"
	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type pluginConversationScript struct {
	fantasy.LanguageModel
	mcp, native, arguments string
	step                   atomic.Int32
}

func (script *pluginConversationScript) Stream(_ context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	step := script.step.Add(1)
	if step > 4 {
		return nil, errors.New("unexpected extra plugin acceptance model request")
	}
	if step >= 2 {
		// The model reads the declared names; the aliases stay internal.
		for _, name := range []string{"mcp__server__echo", declaredToolName(script.native)} {
			count := 0
			for _, tool := range call.Tools {
				if tool.GetName() == name {
					count++
				}
			}
			if count != 1 {
				return nil, fmt.Errorf("captured plugin alias %s appeared %d times", name, count)
			}
		}
	}
	return func(yield func(fantasy.StreamPart) bool) {
		part := fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall}
		switch step {
		case 1:
			arguments, _ := json.Marshal(map[string]any{"query": "select:" + script.mcp + "," + script.native})
			part.ID, part.ToolCallName, part.ToolCallInput = "discover", "tool_search", string(arguments)
		case 2:
			part.ID, part.ToolCallName, part.ToolCallInput = "mcp-call", script.mcp, `{"text":"captured"}`
		case 3:
			part.ID, part.ToolCallName, part.ToolCallInput = "native-call", script.native, script.arguments
		case 4:
			part = fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Completed the captured tools"}
		}
		if yield(part) {
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
		}
	}, nil
}

func pluginMCPConversationAcceptance(t *testing.T) {
	script := &pluginConversationScript{mcp: mcp.Alias("mcp-conversation", "server", "echo"), native: operations.Alias("native-approval-fixture", "write")}
	var mcpCalls atomic.Int32
	options := &mcp.Options{Open: func(ctx context.Context, launch mcp.Launch, elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error) {
		if launch.URL != "https://fixture.test/mcp" {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture MCP service is unavailable"}
		}
		server := sdk.NewServer(&sdk.Implementation{Name: "conversation-fixture", Version: "1"}, nil)
		sdk.AddTool(server, &sdk.Tool{Name: "echo", Annotations: &sdk.ToolAnnotations{ReadOnlyHint: true}}, func(_ context.Context, _ *sdk.CallToolRequest, input struct {
			Text string `json:"text"`
		}) (*sdk.CallToolResult, any, error) {
			mcpCalls.Add(1)
			return &sdk.CallToolResult{Content: []sdk.Content{&sdk.TextContent{Text: "mcp:" + input.Text}}}, nil, nil
		})
		clientWire, serverWire := sdk.NewInMemoryTransports()
		serverSession, err := server.Connect(ctx, serverWire, nil)
		if err != nil {
			return nil, err
		}
		t.Cleanup(func() { _ = serverSession.Close() })
		client := sdk.NewClient(&sdk.Implementation{Name: "conversation-client", Version: "1"}, &sdk.ClientOptions{ElicitationHandler: elicit})
		return client.Connect(ctx, clientWire, nil)
	}}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{
		MCP: options, Environment: &environment, Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil },
		Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }, Run: func(context.Context, process.Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error) {
			return protocol.ProcessCompletion{}, errors.New("plugin acceptance must not launch a process")
		}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Stop(context.Background())
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	before := "before\n"
	if err := os.WriteFile(filepath.Join(root, "note.txt"), []byte(before), 0o600); err != nil {
		t.Fatal(err)
	}
	revision := fmt.Sprintf("%x", blake3.Sum256([]byte(before)))
	arguments, _ := json.Marshal(map[string]any{"path": "note.txt", "text": "after\n", "expected_revision": revision})
	script.arguments = string(arguments)
	project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Plugin acceptance", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}}).Data.(protocol.Project)
	commandFixture(t, node, protocol.Command{Kind: "install_mcp", Data: protocol.CommandInstallMcpPayload{Name: "mcp-conversation",
		Definition: protocol.PluginMcpDefinition{Kind: "streamable-http", Data: &protocol.PluginMcpDefinitionStreamableHttpPayload{Url: "https://fixture.test/mcp", Headers: []string{}}}}})
	native := installNativeApprovalFixture(t, node)
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Revision, provider.Enabled, provider.Name = protocol.NewProviderID(), 1, true, "Fixture"
	provider.Api, provider.Authentication, provider.Endpoint, provider.DefaultModel = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, "http://fixture.invalid/v1", "fixture"
	selected := protocol.DefaultConversationModel()
	selected.ID, selected.Context, selected.Output, selected.Tools = "fixture", 32000, 8000, true
	provider.Models = []protocol.ConversationModel{selected}
	if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		body, err := json.Marshal(map[string]any{"id": provider.ID, "provider": provider, "listed": true, "authorizations": map[string]any{}, "quota": nil})
		if err != nil {
			return err
		}
		_, err = db.ExecContext(t.Context(), "INSERT INTO providers(id,body) VALUES(?1,?2)", provider.ID.String(), body)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Mode, config.Permission = provider.ID, "fixture", protocol.WorkModeCode, protocol.PermissionAsk
	session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
	subscription, err := node.Subscribe(t.Context(), node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Use the captured MCP tool and write note.txt"}}})
	var turn protocol.TurnID
	var approvalID protocol.ApprovalID
	for {
		var history protocol.ConversationHistory
		if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
			var err error
			history, err = store.ReadConversation(t.Context(), db, session.ID, nil, 100)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		for _, approval := range history.Page.Approvals {
			if approval.State != protocol.ConversationApprovalStatePending {
				continue
			}
			body, err := os.ReadFile(filepath.Join(root, "note.txt"))
			if err != nil || string(body) != before {
				t.Fatalf("script wrapper changed the file before approval: %q, %v", body, err)
			}
			if approval.Changes == nil || len(approval.Changes.Files) != 1 || !strings.Contains(approval.Changes.Files[0].Diff, "+prepared:after") {
				t.Fatalf("approval did not preview prepared native arguments: %+v", approval.Changes)
			}
			approvalID = approval.ID
			commandFixture(t, node, protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{Session: session.ID, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "approve"}}})
		}
		completed := false
		for _, run := range history.Page.Runs {
			turn = run.Turn
			switch run.Status {
			case protocol.ConversationStatusCompleted:
				completed = true
			case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
				t.Fatalf("plugin conversation failed: %+v", run)
			}
		}
		if completed {
			break
		}
		if _, err := subscription.Next(t.Context()); err != nil {
			t.Fatal(err)
		}
	}
	if script.step.Load() != 4 || mcpCalls.Load() != 1 || approvalID == (protocol.ApprovalID{}) {
		t.Fatalf("plugin tool loop skipped capture or approval: steps=%d MCP calls=%d approval=%s", script.step.Load(), mcpCalls.Load(), approvalID)
	}
	body, err := os.ReadFile(filepath.Join(root, "note.txt"))
	if err != nil || string(body) != "prepared:after\n" {
		t.Fatalf("native executor ignored script preparation: %q, %v", body, err)
	}
	var state store.AgentState
	var request protocol.Request
	var requestStatus string
	var claimed bool
	var receipt protocol.Result[protocol.Output, protocol.Fault]
	if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		var err error
		state, err = store.LoadAgentState(t.Context(), db, session.ID)
		if err != nil {
			return err
		}
		var requestBody, resultBody []byte
		if err := db.QueryRowContext(t.Context(), `SELECT r.body,r.status,r.result,a.claimed FROM agent_approvals a JOIN turns t ON t.id=a.turn JOIN requests r ON r.caller=t.caller AND r.id=a.request WHERE a.id=?1 AND a.turn=?2`, approvalID.String(), turn.String()).Scan(&requestBody, &requestStatus, &resultBody, &claimed); err != nil {
			return err
		}
		if err := json.Unmarshal(requestBody, &request); err != nil {
			return err
		}
		return json.Unmarshal(resultBody, &receipt)
	}); err != nil {
		t.Fatal(err)
	}
	write, ok := request.Command.Data.(*protocol.CommandWriteFilePayload)
	if !ok || write.Path != "note.txt" || write.Text != "prepared:after\n" || request.Plugin == nil || request.Plugin.Package != plugins.PackageReference(native) ||
		request.Plugin.Invocation != nil || !claimed || requestStatus != "completed" || receipt.Err != nil || receipt.Ok == nil {
		t.Fatalf("script wrapper lost its exact native durable receipt: %+v, claimed=%v status=%s receipt=%+v", request, claimed, requestStatus, receipt)
	}
	// Each of the three one-call batches announces its completion once, in
	// the transaction that stores its last result, so a controller's settled
	// cursor and a plugin's status follow tool effects inside a running turn.
	settled := 0
	if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		settled = 0
		for after := uint64(0); ; {
			_, page, err := store.EventPage(t.Context(), db, node.ID(), after, 0)
			if err != nil || len(page) == 0 {
				return err
			}
			for _, envelope := range page {
				after = envelope.Cursor
				if envelope.Event.Kind != "tools_completed" {
					continue
				}
				if value, ok := envelope.Event.Data.(*protocol.EventToolsCompletedPayload); !ok || value.Session != session.ID {
					return fmt.Errorf("tool completion names another session: %+v", envelope.Event.Data)
				}
				settled++
			}
		}
	}); err != nil {
		t.Fatal(err)
	}
	if settled != 3 {
		t.Fatalf("tool batches announced %d completions, want 3", settled)
	}
	results := map[string]*canonical.ToolResponse{}
	for _, record := range state.Records {
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				if response, ok := block.Data.(*canonical.ToolResponse); ok {
					results[response.ID] = response
				}
			}
		}
	}
	if response := results["mcp-call"]; response == nil || response.Name != script.mcp || response.Status != canonical.ToolSuccess || response.Format != canonical.MCP || !strings.Contains(string(response.Result.Value), "mcp:captured") {
		t.Fatalf("MCP result did not return through canonical history: %+v", response)
	}
	if response := results["native-call"]; response == nil || response.Name != script.native || response.Status != canonical.ToolSuccess || !strings.Contains(string(response.Result.Value), `"wrapped":true`) {
		t.Fatalf("script result formatter did not return through canonical history: %+v", response)
	}
	checkpoints := commandFixture(t, node, protocol.Command{Kind: "list_file_checkpoints", Data: protocol.CommandListFileCheckpointsPayload{Session: session.ID, Turn: turn, Limit: 50}}).Data.(protocol.ConversationCheckpointPage)
	if len(checkpoints.Files) != 1 || checkpoints.Files[0].Before == nil {
		t.Fatalf("native wrapper lost its pre-effect checkpoint: %+v", checkpoints)
	}
	requestBody, _ := json.Marshal(request)
	digest := fmt.Sprintf("%x", blake3.Sum256(requestBody))
	inspected := commandFixture(t, node, protocol.Command{Kind: "inspect_request", Data: protocol.CommandInspectRequestPayload{ID: request.ID, Digest: digest}}).Data.(protocol.OutputRequestOutcomePayload)
	outcome := inspected.Outcome
	expectedReceipt, _ := json.Marshal(receipt)
	actualReceipt, _ := json.Marshal(outcome.Data)
	if outcome.Kind != "completed" || string(expectedReceipt) != string(actualReceipt) {
		t.Fatalf("native receipt inspection changed its exact result: %+v, %s, %s", outcome, expectedReceipt, actualReceipt)
	}
	t.Run("expired turn preserves receipts and refuses fresh authority", func(t *testing.T) {
		replayed := dispatchFixture(t, node, node.ID(), request)
		replayedReceipt, _ := json.Marshal(protocol.Result[protocol.Output, protocol.Fault]{Ok: &replayed.Output, Err: replayed.Fault})
		if string(replayedReceipt) != string(expectedReceipt) {
			t.Fatalf("completed turn lost its exact durable replay: %s, %s", replayedReceipt, expectedReceipt)
		}
		fresh := request
		fresh.ID = protocol.NewRequestID()
		changed := request
		changedWrite := *write
		changedWrite.Text = "stale turn must not write\n"
		changed.Command.Data = &changedWrite
		read := protocol.NewRequest(node.ID(), protocol.Command{Kind: "read_plugin_settings", Data: &protocol.CommandReadPluginSettingsPayload{Package: request.Plugin.Package}})
		read.Plugin = request.Plugin
		otherCaller := node.ID()
		otherCaller[0] ^= 0xff
		for _, entry := range []struct {
			name    string
			caller  protocol.NodeID
			request protocol.Request
			code    protocol.ErrorCode
			message string
		}{
			{"fresh write", node.ID(), fresh, protocol.ErrorCodeConflict, "turn is no longer running"},
			{"fresh read", node.ID(), read, protocol.ErrorCodeConflict, "turn is no longer running"},
			{"changed receipt content", node.ID(), changed, protocol.ErrorCodeConflict, "request identifier was already used for different content"},
			{"different receipt caller", otherCaller, request, protocol.ErrorCodePermissionDenied, "plugin turn belongs to the execution Node"},
		} {
			t.Run(entry.name, func(t *testing.T) {
				accepted, err := node.Dispatch(t.Context(), entry.caller, entry.request)
				var failure *protocol.Fault
				if !errors.As(err, &failure) || failure.Code != entry.code || failure.Message != entry.message {
					t.Fatalf("expired authority admitted=%+v error=%v", accepted.Receipt, err)
				}
			})
		}
		body, err := os.ReadFile(filepath.Join(root, "note.txt"))
		if err != nil || string(body) != "prepared:after\n" {
			t.Fatalf("expired provenance changed the settled file: %q, %v", body, err)
		}
	})
}

func installNativeApprovalFixture(t *testing.T, node *Node) protocol.PluginInfo {
	t.Helper()
	source := t.TempDir()
	entry := "dev.sailry.platform/main.js"
	for name, body := range map[string]string{
		"plugin.json": `{"$schema":"` + plugins.ManifestSchema + `","name":"native-approval-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","scope":"host","actions":["files.write"],"host":{"entry":"` + entry + `","resources":["` + entry + `"],"handlers":["prepare","format"],"read_only":["prepare","format"]},"tools":[{"name":"write","description":"Write a captured fixture","handler":{"name":"prepare","operation":"files.write","result":"format","parameters":{"type":"object","required":["path","text","expected_revision"],"properties":{"path":{"type":"string"},"text":{"type":"string"},"expected_revision":{"type":["string","null"]}},"additionalProperties":false}}}]}}}`,
		entry:         `export function prepare(input){return {path:input.path,text:"prepared:"+input.text,expected_revision:input.expected_revision};} export function format(input){return {wrapped:true,output:input.output};}`,
	} {
		filename := filepath.Join(source, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(filename), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filename, []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	info, err := node.plugins.manager.Install(t.Context(), source, "", "native-approval-fixture")
	if err != nil || len(info.Issues) != 0 || info.Extension == nil {
		t.Fatalf("native script fixture is invalid: %+v, %v", info.Issues, err)
	}
	if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		var err error
		info, err = store.InstallPlugin(t.Context(), db, info, 0, true)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return info
}
