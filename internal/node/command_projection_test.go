package node

import (
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/tokens"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type commandPollingStatus struct {
	Kind    string `json:"kind"`
	Outcome struct {
		Kind string `json:"kind"`
		Code int32  `json:"data"`
	} `json:"data"`
}

type commandPollingInfo struct {
	ID      protocol.RequestID     `json:"id"`
	Command string                 `json:"command"`
	Status  commandPollingStatus   `json:"status"`
	Access  protocol.ProcessAccess `json:"access"`
}

type commandPollingReceipt struct {
	Kind string `json:"kind"`
	Data struct {
		commandPollingInfo
		Info        commandPollingInfo                 `json:"info"`
		Wait        protocol.ProcessWait               `json:"wait"`
		Stdout      protocol.ProcessCapture            `json:"stdout"`
		Stderr      protocol.ProcessCapture            `json:"stderr"`
		Stream      protocol.ProcessOutputOutputStream `json:"stream"`
		Offset      uint64                             `json:"offset"`
		TotalBytes  uint64                             `json:"total_bytes"`
		NextOffset  *uint64                            `json:"next_offset"`
		Text        string                             `json:"text"`
		Truncated   bool                               `json:"truncated"`
		InvalidUtf8 bool                               `json:"invalid_utf8"`
	} `json:"data"`
	Output protocol.ToolOutputOutputReference `json:"output"`
}

type commandPollingModel struct {
	agentToolScript
	requests []fantasy.Prompt
}

func (model *commandPollingModel) Stream(ctx context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	model.mu.Lock()
	model.requests = append(model.requests, call.Prompt)
	model.mu.Unlock()
	return model.agentToolScript.Stream(ctx, call)
}

func TestCommandPollingProjection(t *testing.T) {
	t.Parallel()
	ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
	defer cancel()
	const sample = `sample_count=$(($sample_count + 1)); sample_status="$sample_count"; `
	command := "sample_count=0; " + strings.Repeat(sample, 120)
	const stdout, stderr = "ready: 世界\r\n", "latest diagnostic\n"
	ready, finish := make(chan protocol.RequestID, 1), make(chan struct{})
	script := &commandPollingModel{}
	node, err := StartWithOptions(ctx, filepath.Join(t.TempDir(), "profile"), Options{
		MCP: fixtureMCPOptions(),
		Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) {
			return script, nil
		},
		Process: process.Options{Run: func(ctx context.Context, launch process.Launch, emit func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
			if launch.Command != command || !launch.Background {
				return protocol.ProcessCompletion{}, errors.New("unexpected command launch")
			}
			emit(protocol.ProcessOutputOutputStreamStdout, []byte(stdout))
			emit(protocol.ProcessOutputOutputStreamStderr, []byte(stderr))
			ready <- launch.ID
			select {
			case <-finish:
				return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(7)}}, nil
			case <-ctx.Done():
				return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "cancelled"}}, nil
			}
		}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Stop(context.Background())
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Enabled, provider.Name = protocol.NewProviderID(), true, "Command fixture"
	provider.Api, provider.Authentication = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey
	provider.Endpoint, provider.DefaultModel = "https://fixture.invalid/v1", "fixture"
	selected := protocol.DefaultConversationModel()
	selected.ID, selected.Context, selected.Output, selected.Tools = "fixture", 64000, 8192, true
	provider.Models = []protocol.ConversationModel{selected}
	secret := protocol.NewSecret("fixture")
	provider = commandFixture(t, node, protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}}).Data.(protocol.ConversationProvider)
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Credential = provider.ID, provider.DefaultModel, provider.Credential
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionFull
	session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Config: &config}}).Data.(protocol.Session)
	subscription, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()

	call := func(id, tool string, args map[string]any) [3]string {
		body, _ := json.Marshal(args)
		return [3]string{id, tool, string(body)}
	}
	var commandID, launchedID protocol.RequestID
	var reference protocol.ToolOutputOutputReference
	var firstRound []*canonical.ToolResponse
	var firstResults map[string]string
	for round, providerIDs := range [][]string{{"launch", "poll", "poll", "page"}, {"poll", "next-page", "poll", "original", "wrapped-original", "retained-stderr"}} {
		script.reset(func(step int, results map[string]string) [][3]string {
			if round == 0 {
				switch step {
				case 1:
					return [][3]string{call("launch", "run_command", map[string]any{"command": command, "background": true})}
				case 2:
					var receipt commandPollingReceipt
					if json.Unmarshal([]byte(results["launch"]), &receipt) != nil || receipt.Data.ID == (protocol.RequestID{}) {
						return nil
					}
					commandID = receipt.Data.ID
					select {
					case launchedID = <-ready:
						return [][3]string{call("poll", "read_command", map[string]any{"id": commandID, "wait_ms": 1})}
					case <-ctx.Done():
						return nil
					}
				case 3:
					return [][3]string{call("poll", "read_command", map[string]any{"id": commandID})}
				case 4:
					return [][3]string{call("page", "read_command", map[string]any{"id": commandID, "stream": "stdout", "offset": 7, "limit": 3})}
				}
			} else {
				switch step {
				case 1:
					var page commandPollingReceipt
					if json.Unmarshal([]byte(results["page"]), &page) != nil || page.Data.NextOffset == nil {
						return nil
					}
					return [][3]string{
						call("poll", "read_command", map[string]any{"id": commandID}),
						call("next-page", "read_command", map[string]any{"id": commandID, "stream": "stdout", "offset": *page.Data.NextOffset, "limit": 5}),
					}
				case 2:
					close(finish)
					return [][3]string{call("poll", "read_command", map[string]any{"id": commandID, "wait_ms": 5000})}
				case 3:
					for id, body := range results {
						var receipt commandPollingReceipt
						if json.Unmarshal([]byte(body), &receipt) == nil && receipt.Kind == "command_output" && receipt.Data.Info.ID == commandID &&
							receipt.Data.Wait == protocol.ProcessWaitExited && receipt.Output.Call == id {
							reference = receipt.Output
							return [][3]string{
								call("original", "read_tool_output", map[string]any{"output": reference, "offset": 0}),
								call("wrapped-original", "read_command", map[string]any{"output": reference, "offset": 0}),
								call("retained-stderr", "read_command", map[string]any{"output": reference, "stream": "stderr", "offset": 0}),
							}
						}
					}
				}
			}
			return nil
		})
		script.mu.Lock()
		start := len(script.requests)
		script.mu.Unlock()
		turn := commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
			Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Check the retained command"},
		}}).Data.(protocol.QueuedTurn).ID
		for {
			var run protocol.ConversationRun
			if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				var err error
				run, err = store.ReadAgentRun(ctx, db, turn)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			if run.Status == protocol.ConversationStatusCompleted {
				break
			}
			if run.Status == protocol.ConversationStatusFailed || run.Status == protocol.ConversationStatusCancelled || run.Status == protocol.ConversationStatusInterrupted {
				t.Fatalf("round %d ended with %s", round+1, run.Status)
			}
			if _, err := subscription.Next(ctx); err != nil {
				t.Fatal(err)
			}
		}
		var state store.AgentState
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			state, err = store.LoadAgentState(ctx, db, session.ID)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		session = state.Session
		occurrences := map[string]string{}
		var stored []*canonical.ToolResponse
		for _, record := range state.Records {
			if message := record.Message(); record.Turn == turn && message != nil {
				for pos, block := range message.Content {
					switch tool := block.Data.(type) {
					case *canonical.ToolRequest:
						occurrences[tool.ID] = canonical.ToolCallOccurrenceID(record.ID, pos)
					case *canonical.ToolResponse:
						stored = append(stored, tool)
					}
				}
			}
		}
		script.mu.Lock()
		count := len(script.requests) - start
		var initial, final fantasy.Prompt
		if count != 0 {
			initial, final = script.requests[start], script.requests[len(script.requests)-1]
		}
		script.mu.Unlock()
		if count != 5-round || len(stored) != len(providerIDs) {
			t.Fatalf("round %d recorded %d model requests and %d tool results", round+1, count, len(stored))
		}
		results := commandPollingResults(t, final)
		for i, result := range stored {
			id := providerIDs[i]
			if id == "poll" && (round != 0 || i != 1) {
				id = occurrences[result.ID]
			}
			if _, ok := results[result.ID]; !ok || occurrences[result.ID] == "" || result.ID != id || result.ID == commandID.String() {
				t.Fatalf("round %d lost the stored/model call pairing for %s: %s", round+1, providerIDs[i], result.ID)
			}
		}

		if round == 0 {
			firstRound, firstResults = stored, results
			projected := checkCommandPollingTokens(t, "turn 1", command, commandID, stored, results)
			if launchedID != commandID || projected[0].Kind != "command_started" || projected[0].Data.Status.Kind != "running" {
				t.Fatal("launch did not report the runner's UUID and running status")
			}
			assertRunningPoll(t, projected[1], protocol.ProcessWaitDeadline, stdout, stderr)
			assertRunningPoll(t, projected[2], "", stdout, stderr)
			assertCommandPollingPage(t, projected[3], "stdout", "世", 7, uint64(len(stdout)), new(uint64(10)), "running")
			continue
		}

		replay := commandPollingResults(t, initial)
		if len(replay) != len(firstRound) {
			t.Fatalf("turn 2 started with %d tool results, want %d replayed receipts", len(replay), len(firstRound))
		}
		for _, receipt := range firstRound {
			want := firstResults[receipt.ID]
			if replay[receipt.ID] != want || results[receipt.ID] != want {
				t.Errorf("turn 2 did not replay the projected receipt for %s", receipt.ID)
			}
		}
		checkCommandPollingTokens(t, "turn 2 replay", command, commandID, firstRound, replay)
		checkCommandPollingTokens(t, "turn 2 new", command, commandID, []*canonical.ToolResponse{stored[0], stored[1], stored[2], stored[5]}, results)
		running := decodeCommandPollingReceipt(t, stored[0].ID, results[stored[0].ID])
		assertRunningPoll(t, running, "", stdout, stderr)
		if stored[0].Revision != firstRound[2].Revision || stored[0].ID == firstRound[2].ID {
			t.Error("identical polling results must keep their content revision and distinct call IDs")
		}
		assertCommandPollingPage(t, decodeCommandPollingReceipt(t, stored[1].ID, results[stored[1].ID]), "stdout", "界\r\n", 10, uint64(len(stdout)), nil, "running")
		poll := decodeCommandPollingReceipt(t, stored[2].ID, results[stored[2].ID])
		if poll.Kind != "command_output" || poll.Data.Wait != protocol.ProcessWaitExited || poll.Data.Info.Status.Kind != "finished" ||
			poll.Data.Info.Status.Outcome.Kind != "exited" || poll.Data.Info.Status.Outcome.Code != 7 || stored[2].Status != canonical.ToolErrorStatus ||
			poll.Data.Stdout != (protocol.ProcessCapture{Text: stdout}) || poll.Data.Stderr != (protocol.ProcessCapture{Text: stderr}) {
			t.Error("finished poll lost its exit code, wait reason or captured streams")
		}
		finalBody := string(stored[2].Result.Value)
		for _, saved := range stored[3:5] {
			var page protocol.ToolOutputOutputPage
			raw := saved.Result.Value
			if saved.ID == "wrapped-original" {
				var wrapped protocol.Output
				if err := json.Unmarshal(raw, &wrapped); err != nil || wrapped.Kind != "tool_output" {
					t.Fatalf("read_command did not return a canonical output page: %v", err)
				}
				nested, ok := wrapped.Data.(*protocol.ToolOutputOutputPage)
				if !ok {
					t.Fatalf("wrapped canonical page has type %T", wrapped.Data)
				}
				page = *nested
			} else if err := json.Unmarshal(raw, &page); err != nil {
				t.Fatalf("read_tool_output returned an invalid canonical page: %v", err)
			}
			if page.Turn != turn || page.Output != reference || page.Offset != 0 || page.NextOffset != nil || page.TotalBytes != uint64(len(finalBody)) || page.Text != finalBody ||
				!strings.Contains(results[saved.ID], "\n"+finalBody+"\n") {
				t.Errorf("%s did not recover the exact canonical command result", saved.ID)
			}
		}
		assertCommandPollingPage(t, decodeCommandPollingReceipt(t, stored[5].ID, results[stored[5].ID]), "stderr", stderr, 0, uint64(len(stderr)), nil, "finished")
	}
}

func commandPollingResults(t *testing.T, prompt fantasy.Prompt) map[string]string {
	t.Helper()
	calls := map[string]bool{}
	results := map[string]string{}
	for _, message := range prompt {
		for _, part := range message.Content {
			if call, ok := fantasy.AsMessagePart[fantasy.ToolCallPart](part); ok {
				if calls[call.ToolCallID] {
					t.Fatalf("duplicate model call ID %s", call.ToolCallID)
				}
				calls[call.ToolCallID] = true
			}
			if result, ok := fantasy.AsMessagePart[fantasy.ToolResultPart](part); ok {
				body, text := fantasy.AsToolResultOutputType[fantasy.ToolResultOutputContentText](result.Output)
				if _, duplicate := results[result.ToolCallID]; !calls[result.ToolCallID] || duplicate || !text {
					t.Fatalf("invalid model tool result for %s", result.ToolCallID)
				}
				results[result.ToolCallID] = body.Text
			}
		}
	}
	return results
}

func checkCommandPollingTokens(t *testing.T, label, command string, commandID protocol.RequestID, stored []*canonical.ToolResponse, results map[string]string) []commandPollingReceipt {
	t.Helper()
	var canonicalTokens, modelTokens uint64
	var projected []commandPollingReceipt
	for _, saved := range stored {
		body := results[saved.ID]
		original := decodeCommandPollingReceipt(t, saved.ID, string(saved.Result.Value))
		receipt := decodeCommandPollingReceipt(t, saved.ID, body)
		if want := (protocol.ToolOutputOutputReference{Call: saved.ID, Revision: saved.Revision}); receipt.Output != want || receipt.Kind != original.Kind {
			t.Errorf("%s changed the kind or canonical reference for %s", label, saved.ID)
		}
		revision := blake3.Sum256(saved.Result.Value)
		if receipt.Output.Revision != hex.EncodeToString(revision[:]) {
			t.Errorf("%s has a revision that does not identify the saved content for %s", label, saved.ID)
		}
		before, after := original.Data.Info, receipt.Data.Info
		if receipt.Kind == "command_started" {
			before, after = original.Data.commandPollingInfo, receipt.Data.commandPollingInfo
		}
		if before.Command != command || after.Command != "" || before.ID != commandID || after.ID != commandID || after.Access != before.Access {
			t.Errorf("%s lost process identity or access, or kept the echoed command for %s", label, saved.ID)
		}
		projected = append(projected, receipt)
		canonicalTokens += tokens.Text(string(saved.Result.Value))
		modelTokens += tokens.Text(body)
	}
	t.Logf("o200k %s (%d command receipts): canonical=%d model=%d", label, len(stored), canonicalTokens, modelTokens)
	if modelTokens == 0 || modelTokens*4 > canonicalTokens {
		t.Errorf("%s kept %d of %d command result tokens", label, modelTokens, canonicalTokens)
	}
	return projected
}

func assertRunningPoll(t *testing.T, receipt commandPollingReceipt, wait protocol.ProcessWait, stdout, stderr string) {
	t.Helper()
	data := receipt.Data
	if receipt.Kind != "command_output" || data.Wait != wait || data.Info.Status.Kind != "running" ||
		data.Stdout != (protocol.ProcessCapture{Text: stdout}) || data.Stderr != (protocol.ProcessCapture{Text: stderr}) {
		t.Errorf("running poll %s changed status, wait %q or captured streams", receipt.Output.Call, wait)
	}
}

func assertCommandPollingPage(t *testing.T, receipt commandPollingReceipt, stream protocol.ProcessOutputOutputStream, text string, offset, total uint64, next *uint64, status string) {
	t.Helper()
	data := receipt.Data
	if receipt.Kind != "command_output_page" || data.Info.Status.Kind != status || data.Stream != stream || data.Offset != offset || data.TotalBytes != total ||
		(data.NextOffset == nil) != (next == nil) || next != nil && data.NextOffset != nil && *data.NextOffset != *next || data.Text != text ||
		data.Wait != "" || data.Truncated || data.InvalidUtf8 {
		t.Errorf("page %s changed status, stream, range, flags or text", receipt.Output.Call)
	}
}

func decodeCommandPollingReceipt(t *testing.T, id, body string) commandPollingReceipt {
	t.Helper()
	var receipt commandPollingReceipt
	if err := json.Unmarshal([]byte(body), &receipt); err != nil {
		t.Fatalf("command receipt %s is not valid JSON: %v", id, err)
	}
	return receipt
}
