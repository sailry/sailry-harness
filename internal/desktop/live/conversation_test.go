package live

import (
	"context"
	"encoding/json"
	"slices"
	"strings"
	"sync"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/reader"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConversationProjection(t *testing.T) {
	for _, row := range []struct {
		name            string
		parts           []protocol.ConversationPart
		kind            conversation.BlockKind
		count           int
		failed, warning bool
	}{
		{"text", []protocol.ConversationPart{{Kind: "text", Data: "Answer"}}, conversation.TextBlock, 1, false, false},
		{"reasoning_is_canonical_only", []protocol.ConversationPart{{Kind: "thinking", Data: "Private reasoning"}}, conversation.TextBlock, 0, false, false},
		{"image", []protocol.ConversationPart{{Kind: "image", Data: protocol.ConversationImage{Attachment: protocol.Attachment{ID: protocol.AttachmentID{1}}}}}, conversation.ImagesBlock, 1, false, false},
		{"attachment", []protocol.ConversationPart{{Kind: "attachment", Data: protocol.Attachment{ID: protocol.AttachmentID{1}, Spec: protocol.AttachmentSpec{Name: "report.txt"}}}}, conversation.FilesBlock, 1, false, false},
		{"compaction", []protocol.ConversationPart{{Kind: "compaction", Data: "Summary"}}, conversation.CompactionBlock, 1, false, false},
		{"failed_call", []protocol.ConversationPart{{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{Name: "read", Summary: &protocol.ToolDetailSummary{Failure: &protocol.ToolFailure{Origin: "runtime"}}}}}, conversation.ToolsBlock, 1, true, false},
		{"returned_tool_error", []protocol.ConversationPart{{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{Name: "read", Summary: &protocol.ToolDetailSummary{Failure: &protocol.ToolFailure{Origin: "tool"}}}}}, conversation.ToolsBlock, 1, false, true},
		{"retry", []protocol.ConversationPart{{Kind: "resource", Data: json.RawMessage(`{"type":"model_retry","attempt":2,"limit":5}`)}}, conversation.RetryBlock, 1, false, false},
		{"compacting", []protocol.ConversationPart{{Kind: "resource", Data: json.RawMessage(`{"type":"compacting","started_ms":1}`)}}, conversation.TextBlock, 0, false, false},
		{"compaction_failure", []protocol.ConversationPart{{Kind: "resource", Data: json.RawMessage(`{"type":"compaction_failure","failure":{"source":"automatic","stage":"request","reason":"request_failed","continued":true,"cause":"Unavailable"}}`)}}, conversation.ErrorBlock, 1, false, false},
		{"progress", []protocol.ConversationPart{{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{Name: "update_plan", Summary: &protocol.ToolDetailSummary{Progress: &protocol.ConversationProgress{Steps: []protocol.ConversationProgressStep{{Description: "Inspect", State: "completed"}}}}}}}, conversation.ProgressBlock, 1, false, false},
		{"reference", []protocol.ConversationPart{{Kind: "reference", Data: protocol.ConversationReference{Target: protocol.ConversationReferenceTarget{Kind: "host"}, Label: "Host"}}}, conversation.TextBlock, 0, false, false},
	} {
		t.Run(row.name, func(t *testing.T) {
			id, turn := protocol.SessionID{1}, protocol.TurnID{1}
			page := protocol.DefaultConversationPage()
			page.Session = id
			page.Revision = 1
			page.Runs = []protocol.ConversationRun{{Session: id, Turn: turn, Sequence: 1, Revision: 1, Kind: "task", Status: "completed"}}
			page.Entries = []protocol.ConversationEntry{{ID: "answer", Sequence: 1, Turn: turn, Author: "assistant", Parts: row.parts}}
			state := client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page}}
			turns, _, _, _ := Conversation(state)
			if len(turns) != 1 || len(turns[0].Reply.Blocks) != row.count {
				t.Fatalf("projection = %#v", turns)
			}
			if row.count > 0 {
				block := turns[0].Reply.Blocks[0]
				if block.Kind != row.kind {
					t.Fatalf("block = %#v", block)
				}
				if row.kind == conversation.ToolsBlock {
					call := block.Calls[0]
					if call.Failed != row.failed || call.Warning != row.warning {
						t.Fatalf("tool failure classification = %#v", call)
					}
				}
			}
			if len(state.Snapshot.Page.Entries) != 1 {
				t.Fatal("presentation mutated canonical history")
			}
		})
	}
	t.Run("draft_phase_and_failed_diagnostic", func(t *testing.T) {
		for _, row := range []struct {
			name  string
			parts []protocol.ConversationPart
			phase conversation.Phase
		}{
			{"thinking", []protocol.ConversationPart{{Kind: "thinking", Data: "Private reasoning"}}, conversation.Thinking},
			{"text", []protocol.ConversationPart{{Kind: "text", Data: "New answer"}}, conversation.Generating},
			{"compacting", []protocol.ConversationPart{{Kind: "resource", Data: json.RawMessage(`{"type":"compacting","started_ms":1}`)}}, conversation.Compacting},
			{"awaiting_after_tools", nil, conversation.AwaitingResponse},
		} {
			page := protocol.DefaultConversationPage()
			session, turn := protocol.SessionID{1}, protocol.TurnID{1}
			page.Session, page.Revision = session, 1
			page.Runs = []protocol.ConversationRun{{Session: session, Turn: turn, Kind: "task", Status: "running"}}
			page.Entries = []protocol.ConversationEntry{{ID: "earlier", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Older output"}}}}
			state := client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page, Drafts: []protocol.ConversationDraft{{ID: "current", Turn: turn, Parts: row.parts}}}}
			turns, _, _, _ := Conversation(state)
			if turns[0].Reply.Phase != row.phase {
				t.Fatalf("%s phase=%v, want %v", row.name, turns[0].Reply.Phase, row.phase)
			}
		}
		page := protocol.DefaultConversationPage()
		page.Session = protocol.SessionID{1}
		page.Runs = []protocol.ConversationRun{{Turn: protocol.TurnID{1}, Kind: "task", Status: "failed", Error: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Provider rejected the request"}}}
		_, session, _, _ := Conversation(client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page}})
		if session.Diagnostic == nil || session.Diagnostic.Detail != "Provider rejected the request" || len(session.Diagnostic.Fields) != 1 || session.Diagnostic.Fields[0][1] != string(protocol.ErrorCodeUnavailable) {
			t.Fatal("latest failed-run diagnostic was not projected")
		}
	})
	t.Run("context_estimate_and_bound_mentions", func(t *testing.T) {
		count, estimate := uint64(80), uint64(20)
		limit := uint32(100)
		for _, reported := range []bool{false, true} {
			statistics := protocol.ConversationStatistics{ContextEstimate: &estimate, ContextLimit: &limit, Budget: &protocol.ConversationCompactionBudget{ContextLimit: 200}}
			want := 0.1
			if reported {
				statistics.ContextTokens, want = &count, 0.8
			}
			_, session, _, _ := Conversation(client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Statistics: &statistics}})
			if session.Usage == nil || session.Usage.Context != want {
				t.Fatalf("reported=%v context=%+v", reported, session.Usage)
			}
		}
		state := conversation.New()
		opened := 0
		state.SetReferences("session", map[string][]conversation.Reference{"turn": {{ID: "host", Label: "Host", Open: func() { opened++ }}}})
		turns := []conversation.Turn{{ID: "turn", User: "Read @Host", Reply: conversation.Reply{Status: conversation.Completed}}}
		tester := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			ui.Column(c).Fill().Children(func() { conversation.View(c, state, "session", turns, &conversation.Session{}, referenceViewActions{}) })
		}, 700, 400)
		tester.SetLocale("en")
		tester.Frame()
		tester.Frame()
		if _, ok := tester.AccessByRole(ui.RoleLink, "Host"); !ok {
			t.Fatal("bound sent mention has no interactive link")
		}
		if err := tester.Click("Host"); err != nil || opened != 1 {
			t.Fatalf("bound mention target did not open: %v", err)
		}
		ref := protocol.ConversationReference{Target: protocol.ConversationReferenceTarget{Kind: "session", Data: protocol.SessionID{9}}, Label: "Other session"}
		restored, ok := composerReference(ref)
		input := conversationInput(composer.Message{Text: "@Other session", References: []composer.Reference{restored}})
		if !ok || len(input.References) != 1 || input.References[0].Target.Kind != "session" || input.References[0].Target.Data != ref.Target.Data {
			t.Fatal("sent reference mapping lost its original binding")
		}
	})
	t.Run("reader_boundary", func(t *testing.T) {
		if text := strings.Join(strings.Fields(reader.Text("# Heading\n\n**Bold** [Link](https://invalid.example)\n\n`code`")), " "); text != "Heading Bold Link code" {
			t.Fatal(text)
		}
	})
	t.Run("structured_tool_details", func(t *testing.T) {
		raw := json.RawMessage(`{"output":{"text":"Full output","sailry_content":{"version":1}}}`)
		loaded := protocol.ToolDetail{Result: &raw, Content: &protocol.ToolContent{Version: 1, Blocks: []protocol.ToolContentBlock{{Kind: "text", Data: protocol.ToolContentText{Path: "/text"}}, {Kind: "table", Data: protocol.ToolContentTable{Columns: []string{"Key"}, Rows: [][]string{{"Value"}}}}, {Kind: "diff", Data: protocol.ToolContentDiff{Path: "notes.txt", Text: "+new\n-old", Truncated: true}}}}}
		call := conversation.Call{Output: &conversation.Output{Text: "Preview"}}
		projectToolContent(&call, loaded)
		if call.Output.Text != "Full output" || call.Output.Table == nil || len(call.Output.Table.Rows) != 1 || call.Output.Diff == nil || call.Output.Diff.Added != 1 || call.Output.Diff.Removed != 1 || !call.Output.Partial {
			t.Fatalf("structured detail=%#v", call.Output)
		}
	})
	t.Run("tool_identity_and_question_history", func(t *testing.T) {
		session, turn := protocol.SessionID{1}, protocol.TurnID{1}
		id := "repeated"
		askID := "question"
		spec := &protocol.ConversationQuestionSpec{Prompt: "Choose", Input: protocol.ConversationQuestionInput{Kind: "choice", Data: protocol.ConversationQuestionInputChoicePayload{Options: []string{"First", "Second"}}}}
		page := protocol.DefaultConversationPage()
		page.Session = session
		page.Revision = 1
		page.Runs = []protocol.ConversationRun{{Session: session, Turn: turn, Sequence: 1, Revision: 1, Kind: "task", Status: "completed"}}
		page.Entries = []protocol.ConversationEntry{
			{ID: "call-a", Turn: turn, Branch: "a", Sequence: 1, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{ID: &id, Name: "read"}}}},
			{ID: "call-b", Turn: turn, Branch: "b", Sequence: 2, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{ID: &id, Name: "read"}}}},
			{ID: "result-b", Turn: turn, Branch: "b", Sequence: 3, Author: "tool", Parts: []protocol.ConversationPart{{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{ID: &id, Name: "read"}}}},
			{ID: "ask", Turn: turn, Sequence: 4, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{ID: &askID, Name: "ask_user", Summary: &protocol.ToolDetailSummary{Question: &spec}}}}},
		}
		page.Questions = []protocol.ConversationQuestion{{ID: protocol.QuestionID{1}, Session: session, Turn: turn, Entry: "ask", State: protocol.ConversationQuestionState{Kind: "answered", Data: protocol.ConversationQuestionAnswer{Kind: "choices", Data: protocol.ConversationQuestionAnswerChoicesPayload{Selected: []uint64{1}}}}}}
		turns, _, waiting, _ := Conversation(client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page}})
		blocks := turns[0].Reply.Blocks
		if len(blocks) != 3 || blocks[0].Calls[0].ID == blocks[1].Calls[0].ID || blocks[0].Calls[0].State != conversation.NotExecuted || blocks[1].Calls[0].State != conversation.StateNone {
			t.Fatalf("tool association=%#v", blocks)
		}
		if blocks[2].Kind != conversation.QuestionBlock || !slices.Equal(blocks[2].Question.Answers, []string{"Second"}) || waiting.Question != nil {
			t.Fatal("terminal question was not projected into history")
		}
	})
	t.Run("joined_chronology_and_live_metadata", func(t *testing.T) {
		session := protocol.SessionID{1}
		parent, guide := protocol.TurnID{1}, protocol.TurnID{2}
		page := protocol.DefaultConversationPage()
		page.Session = session
		page.Revision = 1
		page.Runs = []protocol.ConversationRun{{Session: session, Turn: parent, Sequence: 1, Revision: 1, Status: "running", StartedMs: new(int64(1))}, {Session: session, Turn: guide, Sequence: 2, Revision: 1, Status: "completed", Joined: &parent}}
		page.Entries = []protocol.ConversationEntry{{ID: "start", Sequence: 1, Turn: parent, Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "First"}}}, {ID: "guide", Sequence: 2, Turn: guide, Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Second"}}}}
		state := client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page, Drafts: []protocol.ConversationDraft{{ID: "answer", Turn: parent, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Partial"}}}}}}
		turns, _, _, _ := Conversation(state)
		if len(turns) != 1 || turns[0].User != "First\n\nSecond" || turns[0].Reply.Blocks[0].Text != "Partial" || !turns[0].Reply.Status.Active() {
			t.Fatalf("joined projection = %#v", turns)
		}
	})
}

type referenceViewActions struct{ conversation.Actions }

func (referenceViewActions) ImageAction(kit.ImageAction, conversation.Image) {}

type conversationTestSubscription struct {
	updates chan protocol.Update
	closed  chan struct{}
	once    sync.Once
}

func (s *conversationTestSubscription) Next(ctx context.Context) (protocol.Update, error) {
	select {
	case value := <-s.updates:
		return value, nil
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	case <-s.closed:
		return protocol.Update{}, context.Canceled
	}
}
func (s *conversationTestSubscription) Close() error {
	s.once.Do(func() { close(s.closed) })
	return nil
}

type conversationTestTransport struct {
	mu           sync.Mutex
	snapshot     protocol.Snapshot
	requests     []protocol.Request
	failure      protocol.ErrorCode
	conversation protocol.ConversationSnapshot
}

func (t *conversationTestTransport) Target() protocol.NodeID { return t.snapshot.Node }
func (t *conversationTestTransport) Subscribe(_ context.Context, topic protocol.Topic) (link.Subscription, error) {
	s := &conversationTestSubscription{updates: make(chan protocol.Update, 2), closed: make(chan struct{})}
	if topic.Kind == "node" {
		s.updates <- protocol.Update{Kind: "snapshot", Data: t.snapshot}
	} else {
		s.updates <- protocol.Update{Kind: "conversation_snapshot", Data: t.conversation}
	}
	return s, nil
}
func (t *conversationTestTransport) Dispatch(_ context.Context, request protocol.Request) (link.Admission, error) {
	t.mu.Lock()
	t.requests = append(t.requests, request)
	failure := t.failure
	t.mu.Unlock()
	response := link.Response{}
	switch request.Command.Kind {
	case "set_session_config":
		p := request.Command.Data.(protocol.CommandSetSessionConfigPayload)
		s := t.snapshot.Sessions[0]
		s.Revision++
		s.Config = p.Config
		response.Output = protocol.Output{Kind: "session", Data: s}
	case "create_session_at":
		p := request.Command.Data.(protocol.CommandCreateSessionAtPayload)
		s := protocol.DefaultSession()
		s.ID = protocol.SessionID{9}
		s.Project = p.Project
		if p.Worktree != nil {
			s.Worktree = *p.Worktree
		}
		s.Revision = 1
		s.Config = *p.Config
		response.Output = protocol.Output{Kind: "session", Data: s}
	case "submit_turn", "guide_turn", "queue_turn", "replace_turn":
		if failure != "" {
			response.Fault = &protocol.Fault{Code: failure, Message: "Captured rejection"}
		} else {
			queued := protocol.DefaultQueuedTurn()
			queued.Kind = protocol.ConversationRunKindTask
			queued.Config.Mode, queued.Config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
			response.Output = protocol.Output{Kind: "queued_turn", Data: queued}
		}
	case "read_power_settings":
		response.Output = protocol.Output{Kind: "power_settings", Data: protocol.PowerSettings{Revision: 1}}
	case "read_telemetry_settings":
		response.Output = protocol.Output{Kind: "telemetry_settings", Data: protocol.TelemetrySettings{Revision: 1}}
	case "list_channel_presets":
		response.Output = protocol.Output{Kind: "channel_presets", Data: protocol.DefaultConversationPresetsListing()}
	case "read_compaction_settings":
		response.Output = protocol.Output{Kind: "compaction_settings", Data: protocol.ConversationCompactionSettings{Revision: 1, Percent: 80}}
	}
	completion := make(chan link.Response, 1)
	completion <- response
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}, nil
}

func TestComposerSendRouting(t *testing.T) {
	for _, row := range []struct {
		name, session, kind string
		running             bool
		failure             protocol.ErrorCode
		configure           bool
	}{
		{"new_project_session", "draft/1", "submit_turn", false, "", false},
		{"existing", "existing", "submit_turn", false, "", false},
		{"running_guides", "existing", "guide_turn", true, "", false},
		{"failure_retains_draft", "existing", "submit_turn", false, protocol.ErrorCodeUnavailable, false},
		{"unknown_retains_request", "existing", "submit_turn", false, protocol.ErrorCodeOutcomeUnknown, false},
		{"configuration_before_admission", "existing", "submit_turn", false, "", true},
		{"replace_references", "existing", "replace_turn", false, "", false},
	} {
		t.Run(row.name, func(t *testing.T) {
			node, project, worktree, provider := protocol.NodeID{1}, protocol.ProjectID{2}, protocol.WorktreeID{3}, protocol.ProviderID{4}
			session := protocol.SessionID{5}
			config := protocol.DefaultSessionConfig()
			config.Provider = provider
			config.Model = "fixture"
			config.Mode = "code"
			config.Permission = "ask"
			config.Effort = protocol.Effort{Kind: "default"}
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node = node
			snapshot.Projects = []protocol.Project{{ID: project, Name: "Workspace", Path: "/fixture/project"}}
			snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Project: &project, Path: "/fixture/worktree", Main: true}}
			snapshot.Providers = []protocol.ConversationProvider{{ID: provider, Revision: 7, Enabled: true, Name: "Provider", Api: "responses", Authentication: "api_key", Models: []protocol.ConversationModel{{ID: "fixture", DefaultEffort: protocol.Effort{Kind: "default"}}}}}
			if row.session == "existing" {
				snapshot.Sessions = []protocol.Session{{ID: session, Revision: 6, Worktree: worktree, Project: &project, Config: config}}
			}
			transport := &conversationTestTransport{snapshot: snapshot, failure: row.failure}
			transport.conversation = protocol.DefaultConversationSnapshot()
			transport.conversation.Node = node
			transport.conversation.Page.Session = session
			transport.conversation.Page.Revision = 1
			if row.running {
				transport.conversation.Page.Runs = []protocol.ConversationRun{{Session: session, Turn: protocol.TurnID{6}, Sequence: 1, Revision: 1, Kind: "task", Status: "running"}}
			}
			if row.name == "replace_references" {
				turn := protocol.TurnID{6}
				transport.conversation.Page.Runs = []protocol.ConversationRun{{Session: session, Turn: turn, Worktree: worktree, Sequence: 1, Revision: 1, Kind: "task", Status: "completed"}}
				transport.conversation.Page.Entries = []protocol.ConversationEntry{{ID: "sent", Sequence: 1, Turn: turn, Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Captured draft @Host"}, {Kind: "reference", Data: protocol.ConversationReference{Target: protocol.ConversationReferenceTarget{Kind: "host"}, Label: "Host"}}}}}
			}
			controller := client.New(transport)
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			defer controller.Close()
			r, err := NewWithPreferences(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"))
			if err != nil {
				t.Fatal(err)
			}
			defer r.Close()
			id := session.String()
			if row.session != "existing" {
				id = r.App.Sidebar.Selected
			}
			cv := r.App.Data.Find(id)
			if cv == nil {
				cv = &viewmodel.Conversation{ID: id}
			}
			comp := r.App.Composer(id)
			comp.Model = modelKey(provider, "fixture")
			comp.Project = project.String()
			comp.Worktree = "/fixture/worktree"
			comp.Host = node.String()
			if row.running || row.name == "replace_references" {
				changes := make(chan struct{}, 1)
				release, err := controller.WatchConversation(t.Context(), session, func() {
					select {
					case changes <- struct{}{}:
					default:
					}
				})
				if err != nil {
					t.Fatal(err)
				}
				defer release()
				for {
					state, ok := controller.Conversation(session)
					if ok && state.Connected {
						break
					}
					select {
					case <-changes:
					case <-t.Context().Done():
						t.Fatal(t.Context().Err())
					}
				}
			}
			var context *ui.Context
			tester := ui.NewTester(func(c *ui.Context) { context = c; r.drain(c) }, 640, 400)
			tester.SetLocale("en")
			tester.Frame()
			r.workers.Wait()
			tester.Frame()
			actions := &conversationActions{r: r, c: context, cv: cv, comp: comp}
			message := composer.Message{Text: "Captured draft", Model: comp.Model, Mode: composer.Code, Permission: composer.AskFirst}
			if row.name == "replace_references" {
				actions.Edit((protocol.TurnID{6}).String())
				if comp.Draft != "Captured draft @Host" || len(comp.References) != 1 || comp.References[0].ID != "host" {
					t.Fatal("editing a sent reference did not restore the bound draft")
				}
				message.Text, message.References = comp.Draft, comp.References
			}
			if row.configure {
				session, _ := r.session(id)
				config := session.Config
				config.Mode = "plan"
				r.saveComposerConfig(session, config, nil)
				message.Mode = composer.Plan
			}
			if row.running {
				actions.Guide(message)
			} else {
				actions.Send(message)
			}
			for range 4 {
				r.workers.Wait()
				tester.Frame()
			}
			transport.mu.Lock()
			var commands []protocol.Request
			for _, request := range transport.requests {
				if request.Command.Kind == "create_session_at" || request.Command.Kind == row.kind {
					commands = append(commands, request)
				}
			}
			transport.mu.Unlock()
			if row.session != "existing" {
				if len(commands) != 2 || commands[0].Command.Kind != "create_session_at" {
					t.Fatalf("new session routing = %#v", commands)
				}
				create := commands[0].Command.Data.(protocol.CommandCreateSessionAtPayload)
				if create.Target != node || create.Project == nil || *create.Project != project || create.Worktree == nil || *create.Worktree != worktree || create.ProviderRevision != 7 {
					t.Fatalf("captured project/Node scope = %#v", create)
				}
			} else if len(commands) != 1 {
				t.Fatalf("existing routing = %#v", commands)
			}
			if last := commands[len(commands)-1]; last.Command.Kind != row.kind || last.Target != node {
				t.Fatalf("send target = %#v", last)
			}
			if row.name == "replace_references" {
				p := commands[len(commands)-1].Command.Data.(protocol.CommandReplaceTurnPayload)
				if len(p.Message.References) != 1 || p.Message.References[0].Target.Kind != "host" {
					t.Fatal("ReplaceTurn dropped the original reference target")
				}
			}
			if row.configure {
				p := commands[len(commands)-1].Command.Data.(protocol.CommandSubmitTurnPayload)
				if p.ExpectedRevision != 7 {
					t.Fatal("admission did not use the confirmed config revision")
				}
			}
			if row.failure != "" && comp.Draft != "Captured draft" {
				t.Fatal("failed request discarded the draft")
			}
			if row.failure == "" && comp.Draft != "" {
				t.Fatal("successful request retained the submitted draft")
			}
			if row.failure == protocol.ErrorCodeOutcomeUnknown {
				first := commands[len(commands)-1].ID
				actions.Send(message)
				r.workers.Wait()
				tester.Frame()
				transport.mu.Lock()
				last := transport.requests[len(transport.requests)-1]
				transport.mu.Unlock()
				if last.ID != first {
					t.Fatal("explicit retry changed uncertain request identity")
				}
			}
			if slices.ContainsFunc(r.App.Data.Models, func(m composer.Model) bool { return m.Name == "" }) {
				t.Fatal("live model projection contains an empty sample")
			}
		})
	}
	t.Run("projected_default_effort", func(t *testing.T) {
		provider := protocol.ProviderID{1}
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = protocol.NodeID{1}
		snapshot.Providers = []protocol.ConversationProvider{{ID: provider, Enabled: true, Models: []protocol.ConversationModel{{ID: "reasoning", Reasoning: true, Efforts: []protocol.Effort{{Kind: "low"}, {Kind: "high"}}, DefaultEffort: protocol.Effort{Kind: "medium"}}}}}
		controller := client.New(&conversationTestTransport{snapshot: snapshot})
		if err := controller.Start(t.Context()); err != nil {
			t.Fatal(err)
		}
		defer controller.Close()
		r, err := NewWithPreferences(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"))
		if err != nil {
			t.Fatal(err)
		}
		defer r.Close()
		comp := composer.New("")
		comp.Mode, comp.Permission, comp.Effort = composer.Plan, composer.FullAccess, 0
		r.defaultComposer(comp)
		if comp.Model != modelKey(provider, "reasoning") || comp.EffortName(nil, comp.Chosen(r.App.Data.Models)) != "medium" || comp.Mode != composer.Plan || comp.Permission != composer.FullAccess {
			t.Fatalf("projected model default = %#v", comp)
		}
	})
}
