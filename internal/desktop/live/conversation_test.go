package live

import (
	"context"
	"encoding/json"
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/reader"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConversationProjection(t *testing.T) {
	t.Run("selected role and skill retain canonical identities", func(t *testing.T) {
		role := protocol.RoleReference{ID: protocol.NewRoleID(), Revision: 7}
		encoded, err := json.Marshal(role)
		if err != nil {
			t.Fatal(err)
		}
		message := composer.Message{Text: "Use these selections", References: []composer.Reference{
			{ID: role.ID.String(), Kind: composer.Agent, Label: "Explore", Insert: string(encoded)},
			{ID: "web-search:research", Kind: composer.Skill, Label: "Research"},
		}}
		input := conversationInput(message)
		if len(input.References) != 2 {
			t.Fatal("selected role or skill was dropped", input.References)
		}
		for index, reference := range input.References {
			restored, ok := composerReference(reference)
			if !ok || restored.ID != message.References[index].ID || restored.Kind != message.References[index].Kind || restored.Insert != message.References[index].Insert {
				t.Fatalf("selection changed after queue restoration: %#v", restored)
			}
		}
	})
	t.Run("delegated foreground wave presentation", func(t *testing.T) {
		parent, childID := protocol.SessionID{1}, protocol.SessionID{2}
		turn, childTurn := protocol.TurnID{1}, protocol.TurnID{2}
		callID := "delegation"
		name := "Research"
		page := protocol.DefaultConversationPage()
		page.Session = parent
		page.Runs = []protocol.ConversationRun{{Session: parent, Turn: turn, Kind: "task", Status: "running"}}
		page.Entries = []protocol.ConversationEntry{
			{ID: "spawn", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{ID: &callID, Name: "spawn_agent", Summary: &protocol.ToolDetailSummary{}}}}},
			{ID: "result", Turn: turn, Author: "tool", Parts: []protocol.ConversationPart{{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{ID: &callID, Name: "spawn_agent"}}}},
		}
		child := protocol.ConversationChild{Run: protocol.ConversationRun{Session: childID, Turn: childTurn, Kind: "task", Status: "running", Sequence: 2}, Origin: protocol.Delegation{Session: parent, Turn: turn, Entry: "spawn"}, Name: &name}
		page.Children = []protocol.ConversationChild{child}
		for _, row := range []struct {
			name   string
			change func(*protocol.ConversationSnapshot)
			wait   int
		}{
			{"running foreground", func(*protocol.ConversationSnapshot) {}, 1},
			{"queued foreground", func(snapshot *protocol.ConversationSnapshot) { snapshot.Page.Children[0].Run.Status = "queued" }, 1},
			{"stopping foreground", func(snapshot *protocol.ConversationSnapshot) { snapshot.Page.Children[0].Run.Status = "stopping" }, 1},
			{"continued child settled", func(snapshot *protocol.ConversationSnapshot) { snapshot.Page.Children[0].Run.Status = "continued" }, 0},
			{"streaming parent", func(snapshot *protocol.ConversationSnapshot) {
				snapshot.Drafts = []protocol.ConversationDraft{{Turn: turn, Author: "assistant"}}
			}, 0},
			{"parent approval", func(snapshot *protocol.ConversationSnapshot) {
				snapshot.Page.Approvals = []protocol.ConversationApproval{{Turn: turn, State: "pending"}}
			}, 0},
			{"pending guidance", func(snapshot *protocol.ConversationSnapshot) {
				snapshot.Page.Queue.Items = []protocol.ConversationPending{{Turn: protocol.TurnID{3}, Guidance: &turn}}
			}, 0},
			{"tool batch uncommitted", func(snapshot *protocol.ConversationSnapshot) { snapshot.Page.Entries = snapshot.Page.Entries[:1] }, 0},
			{"background child", func(snapshot *protocol.ConversationSnapshot) {
				call, _ := detail[protocol.ConversationPartToolCallPayload](snapshot.Page.Entries[0].Parts[0].Data)
				call.Summary = &protocol.ToolDetailSummary{Delegation: protocol.ToolDetailDelegation{Background: true}}
				snapshot.Page.Entries[0].Parts[0].Data = call
			}, 0},
		} {
			t.Run(row.name, func(t *testing.T) {
				snapshot := protocol.ConversationSnapshot{Page: page}
				snapshot.Page.Runs = slices.Clone(page.Runs)
				snapshot.Page.Children = slices.Clone(page.Children)
				snapshot.Page.Entries = slices.Clone(page.Entries)
				for index := range snapshot.Page.Entries {
					snapshot.Page.Entries[index].Parts = slices.Clone(page.Entries[index].Parts)
				}
				row.change(&snapshot)
				turns, _, _, _ := Conversation(client.ConversationState{Connected: true, Snapshot: snapshot})
				if len(turns) != 1 || turns[0].Reply.Waiting != row.wait {
					t.Fatalf("waiting projection=%#v, want %d", turns, row.wait)
				}
				if row.wait > 0 && turns[0].Reply.Phase != conversation.WaitingChildren {
					t.Fatal("foreground wave did not show waiting children")
				}
			})
		}
	})
	t.Run("child ranking and registered ownership", func(t *testing.T) {
		parent, childID := protocol.SessionID{1}, protocol.SessionID{2}
		worktree, wrong := protocol.WorktreeID{1}, protocol.WorktreeID{2}
		origin := protocol.Delegation{Session: parent}
		child := protocol.ConversationChild{Run: protocol.ConversationRun{Session: childID, Worktree: worktree, Status: "running", Sequence: 1}, Origin: origin}
		newer := child
		newer.Run.Status, newer.Run.Sequence = "queued", 2
		selected := client.LatestConversationChildren([]protocol.ConversationChild{child, newer})
		if len(selected) != 1 || selected[0].Run.Status != "running" {
			t.Fatal("queued work replaced the active child", selected)
		}
		child.Run.Status = "completed"
		selected = client.LatestConversationChildren([]protocol.ConversationChild{child, newer})
		if len(selected) != 1 || selected[0].Run.Status != "completed" {
			t.Fatal("queued work replaced the last executed child", selected)
		}
		snapshot := protocol.DefaultSnapshot()
		delegation := &origin
		snapshot.Sessions = []protocol.Session{{ID: parent, Worktree: worktree}, {ID: childID, Worktree: worktree, Delegation: &delegation}}
		snapshot.Worktrees = []protocol.Worktree{{ID: worktree}}
		if _, ok := client.ConversationChildSession(snapshot, parent, child); !ok {
			t.Fatal("registered child could not be opened")
		}
		snapshot.Sessions[1].Worktree = wrong
		if _, ok := client.ConversationChildSession(snapshot, parent, child); ok {
			t.Fatal("stale child binding opened another worktree")
		}
	})
	t.Run("approval file changes and localized notices", testApprovalProjection)
	t.Run("attachment chooser availability", testAttachmentPickerAvailability)
	t.Run("checkpoint tool body uses only unified change lines", func(t *testing.T) {
		name := "note.txt"
		format := protocol.ToolDisplayProjectionFormatUnified
		loaded := protocol.ToolDetail{Resolved: &protocol.ToolDetailResolved{Output: &protocol.ToolDetailOutput{Body: &protocol.ToolDetailBody{Path: &name, Diff: &format, Text: "diff --git a/note.txt b/note.txt\n--- a/note.txt\n+++ b/note.txt\n@@ -1,2 +1,2 @@\n before\n-old\n+new\n\\ No newline at end of file\n"}}}}
		call := conversation.Call{Output: &conversation.Output{Text: "replacement fragment"}}
		projectToolContent(&call, loaded)
		if call.Output.Text != "" || call.Output.Diff == nil || call.Output.Diff.Path != name || call.Output.Diff.Added != 1 || call.Output.Diff.Removed != 1 || len(call.Output.Diff.Lines) != 3 || call.Output.Diff.Lines[0].Text != "before" {
			t.Fatalf("checkpoint diff projection = %#v", call.Output)
		}
	})
	t.Run("grounding links and native command tail are live data", func(t *testing.T) {
		session, turn := protocol.NewSessionID(), protocol.NewTurnID()
		end := uint32(3)
		title := "Public source"
		suggestions := "<div>Provider suggestions</div>"
		page := protocol.DefaultConversationPage()
		page.Session = session
		page.Runs = []protocol.ConversationRun{{Turn: turn, Session: session, Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusCompleted}}
		page.Entries = []protocol.ConversationEntry{{ID: "grounded", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "结果🙂"}}, Citations: []protocol.ConversationCitation{{Uri: "https://example.com/source", Title: &title, End: &end}}, SearchSuggestions: &suggestions}}
		state := client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page}}
		turns, _, _, _ := Conversation(state)
		if len(turns) != 1 || len(turns[0].Reply.Citations) != 1 || turns[0].Reply.Citations[0].End != 3 {
			t.Fatal(turns)
		}
		found := false
		for _, block := range turns[0].Reply.Blocks {
			if block.Kind == conversation.SearchBlock {
				found = true
				if block.Text != "结果🙂" || len(block.Sources) != 1 || block.Sources[0].URI != "https://example.com/source" {
					t.Fatal(block)
				}
			}
		}
		if !found {
			t.Fatal("provider grounding did not reach Globe disclosure")
		}
		// The provider's suggestions keep only their web links.
		linked := `<style>a{}</style><div><a href="https://example.com/q?a=1"> Go  iterators </a><a href="javascript:alert(1)">x</a><script>bad()</script></div>`
		page.Entries[0].SearchSuggestions = &linked
		state.Snapshot.Page = page
		suggested, _, _, _ := Conversation(state)
		for _, block := range suggested[0].Reply.Blocks {
			if block.Kind == conversation.SearchBlock && (len(block.Suggestions) != 1 || block.Suggestions[0].URI != "https://example.com/q?a=1" || block.Suggestions[0].Title != "Go iterators") {
				t.Fatalf("search suggestions = %+v", block.Suggestions)
			}
		}
		if from := conversation.AnswerFrom(conversation.Compact(turns[0].Reply.Blocks)); from != 1 || turns[0].Reply.Blocks[from].Kind != conversation.TextBlock {
			t.Fatal("grounding hid the answer in folded work", turns[0].Reply.Blocks)
		}
		secondEnd := uint32(2)
		zero := uint32(0)
		page.Entries[0].Citations = append(page.Entries[0].Citations, protocol.ConversationCitation{Uri: "https://example.com/unpositioned"}, protocol.ConversationCitation{Uri: "https://example.com/start", End: &zero})
		page.Entries = append(page.Entries, protocol.ConversationEntry{ID: "second", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "More"}}, Citations: []protocol.ConversationCitation{{Uri: "https://example.com/second", End: &secondEnd}, {Uri: "https://example.com/last"}, {Uri: "https://example.com/next-start", End: &zero}}})
		state.Snapshot.Page = page
		projected, _, _, _ := Conversation(state)
		for index, want := range []int{3, 3, -1, 5, 7, -4} {
			if got := projected[0].Reply.Citations[index].End; got != want {
				t.Fatalf("citation %d endpoint=%d, want %d", index, got, want)
			}
		}
		command := conversation.Call{}
		result := json.RawMessage(`{"kind":"command_result","data":{"stdout":{"text":"first\nlast"}},"sailry_command":{"id":"retained"}}`)
		projectToolContent(&command, protocol.ToolDetail{Result: &result})
		if command.Output == nil || !command.Output.Command {
			t.Fatal("command result did not select last100 line presentation", command)
		}
	})

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
		{"output_limit", []protocol.ConversationPart{{Kind: "resource", Data: json.RawMessage(`{"type":"output_limit"}`)}}, conversation.LimitBlock, 1, false, false},
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
			// The compacting phase carries how long it has run.
			if (turns[0].Reply.PhaseElapsed > 0) != (row.name == "compacting") {
				t.Fatalf("%s phase elapsed = %v", row.name, turns[0].Reply.PhaseElapsed)
			}
		}
		page := protocol.DefaultConversationPage()
		page.Session = protocol.SessionID{1}
		page.Runs = []protocol.ConversationRun{{Turn: protocol.TurnID{1}, Kind: "task", Status: "failed", Error: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Provider rejected the request"}}}
		_, session, _, _ := Conversation(client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page}})
		if session.Diagnostic == nil || session.Diagnostic.Detail != "Provider rejected the request" || len(session.Diagnostic.Fields) != 1 || session.Diagnostic.Fields[0][1] != string(protocol.ErrorCodeUnavailable) {
			t.Fatal("latest failed-run diagnostic was not projected")
		}
		// A failed automatic compaction says why once: its own block replaces the run's.
		page.Entries = []protocol.ConversationEntry{{ID: "failure", Turn: protocol.TurnID{1}, Author: "system", Parts: []protocol.ConversationPart{{Kind: "resource", Data: json.RawMessage(`{"type":"compaction_failure","failure":{"source":"automatic","stage":"request","reason":"request_failed","continued":false,"cause":"Provider rejected the request"}}`)}}}}
		compacting, reported, _, _ := Conversation(client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page}})
		if blocks := compacting[0].Reply.Blocks; len(blocks) != 1 || blocks[0].Kind != conversation.ErrorBlock || blocks[0].Label != "chat_compact_failed" || blocks[0].Text != "Provider rejected the request" || reported.Diagnostic == nil {
			t.Fatalf("failed compaction blocks = %+v", compacting[0].Reply.Blocks)
		}
		// Compaction runs: a completed one without a summary compacted nothing,
		// unless its entries are not loaded.
		for _, row := range []struct {
			name    string
			status  protocol.ConversationStatus
			summary bool
			missing bool
			want    conversation.CompactionState
		}{
			{"summarized", "completed", true, false, conversation.Compacted},
			{"unneeded", "completed", false, false, conversation.CompactionUnneeded},
			{"not loaded", "completed", false, true, conversation.Compacted},
			{"failed", "failed", false, false, conversation.CompactionFailed},
			{"running", "running", false, false, conversation.CompactionRunning},
		} {
			manual := protocol.DefaultConversationPage()
			manual.Session = protocol.SessionID{1}
			manual.Runs = []protocol.ConversationRun{{Turn: protocol.TurnID{2}, Kind: "compaction", Status: row.status}}
			if row.summary {
				manual.Entries = []protocol.ConversationEntry{{ID: "summary", Turn: protocol.TurnID{2}, Author: "system", Parts: []protocol.ConversationPart{{Kind: "compaction", Data: "Summary"}}}}
			}
			snapshot := protocol.ConversationSnapshot{Page: manual}
			if row.missing {
				snapshot.Missing = []protocol.TurnID{{2}}
			}
			compacted, _, _, _ := Conversation(client.ConversationState{Connected: true, Snapshot: snapshot})
			if k := compacted[0].Compaction; k == nil || k.State != row.want || row.summary != (k.Summary == "Summary") {
				t.Fatalf("%s compaction = %+v", row.name, compacted[0].Compaction)
			}
		}
		page.Entries = nil
		// A recovery that gave up names its reason on the last retry row and as the failure's title.
		turn := protocol.TurnID{1}
		page.Runs[0].Error = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "429", Details: &protocol.FaultDetails{Kind: "model_recovery", Data: protocol.FaultDetailsModelRecoveryPayload{Reason: protocol.ConversationRecoveryStopRateLimited}}}
		denied, missing, call := protocol.ErrorCodePermissionDenied, protocol.ErrorCodeNotFound, "call-1"
		page.Entries = []protocol.ConversationEntry{{ID: "work", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{
			{Kind: "resource", Data: json.RawMessage(`{"type":"model_retry","attempt":1,"limit":5,"error":"slow down"}`)},
			{Kind: "resource", Data: json.RawMessage(`{"type":"model_retry","attempt":2,"limit":5,"error":"hidden","fallback":true}`)},
			{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{Name: "load_skill", Summary: &protocol.ToolDetailSummary{Failure: &protocol.ToolFailure{Code: &denied, Origin: "runtime"}}}},
			{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{Name: "read_session", Summary: &protocol.ToolDetailSummary{Failure: &protocol.ToolFailure{Code: &missing, Origin: "runtime"}}}},
			{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{ID: &call, Name: "screenshot", Images: []protocol.ConversationImage{{Index: 2, Attachment: protocol.Attachment{ID: protocol.AttachmentID{9}, Spec: protocol.AttachmentSpec{Name: "screen.png"}}}}}},
		}}}
		stopped, failed, _, _ := Conversation(client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page}}, "zh-CN")
		var retries []*conversation.Retry
		var calls []conversation.Call
		title := ""
		for _, block := range stopped[0].Reply.Blocks {
			switch block.Kind {
			case conversation.RetryBlock:
				retries = append(retries, block.Retry)
			case conversation.ToolsBlock:
				calls = append(calls, block.Calls...)
			case conversation.ErrorBlock:
				title = block.Label
			}
		}
		if len(retries) != 2 || retries[0].Stopped || retries[0].Detail != "slow down" || !retries[1].Stopped || retries[1].Reason != "chat_model_rate_limited" || retries[1].Detail != "" {
			t.Fatalf("stopped retries = %+v", retries)
		}
		if title != "chat_model_rate_limited" || failed.Diagnostic == nil || failed.Diagnostic.Title != "chat_model_rate_limited" || i18n.In("zh-CN", title) == title {
			t.Fatal("the stop reason is not the failure's title", title, failed.Diagnostic)
		}
		// Built-in tools carry their label key and the kind of a failure without a message, in the user's language.
		if len(calls) != 3 || calls[0].Label != "" || calls[0].LabelKey != "tool_load_skill" || calls[0].Error != i18n.In("zh-CN", "tool_denied") ||
			calls[1].LabelKey != "tool_read_session" || calls[1].Error != i18n.In("zh-CN", "tool_not_found") || calls[0].Error == i18n.In("en", "tool_denied") {
			t.Fatalf("core tool labels = %+v", calls)
		}
		// A picture a tool returned shows under its output, named as a reply refers to it.
		if out := calls[2].Output; out == nil || len(out.Images) != 1 || out.Images[0].ID != (protocol.AttachmentID{9}).String() || out.Images[0].Reference != "call-1/2" || out.Images[0].Name != "screen.png" {
			t.Fatalf("tool result images = %+v", calls[2].Output)
		}
		if key, ok := imageReference("sailry-image:call-1/2"); !ok || key != "call-1/2" {
			t.Fatal("image reference", key, ok)
		}
		if _, ok := imageReference("sailry-image:call-1/02"); ok {
			t.Fatal("a padded image index was accepted")
		}
	})
	t.Run("connection references", testConnectionReferences)
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
	sessions     map[protocol.SessionID]protocol.Session
	teamFailure  protocol.ErrorCode
	teamOutputs  map[protocol.RequestID]protocol.Output
	// handle answers a command before the fixture's own answers, under mu.
	handle func(protocol.Request) (link.Response, bool)
}

func (t *conversationTestTransport) Target() protocol.NodeID { return t.snapshot.Node }
func (t *conversationTestTransport) WatchConnection(ctx context.Context, changed func(link.ConnectionInfo)) error {
	return link.NewLocal(t.Target(), t.Target(), nil).WatchConnection(ctx, changed)
}
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
	defer t.mu.Unlock()
	t.requests = append(t.requests, request)
	failure := t.failure
	if t.sessions == nil {
		t.sessions = map[protocol.SessionID]protocol.Session{}
		for _, session := range t.snapshot.Sessions {
			t.sessions[session.ID] = session
		}
	}
	response := link.Response{}
	if t.handle != nil {
		if answer, handled := t.handle(request); handled {
			completion := make(chan link.Response, 1)
			completion <- answer
			return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}, nil
		}
	}
	switch request.Command.Kind {
	case "set_session_config":
		p := request.Command.Data.(protocol.CommandSetSessionConfigPayload)
		s := t.sessions[p.Session]
		if p.ExpectedRevision != s.Revision {
			response.Fault = &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "session changed before configuration save"}
			break
		}
		s.Revision++
		s.Config = p.Config
		t.sessions[s.ID] = s
		response.Output = protocol.Output{Kind: "session", Data: s}
	case "create_session":
		p := request.Command.Data.(protocol.CommandCreateSessionPayload)
		s := protocol.DefaultSession()
		s.ID = protocol.SessionID{9}
		s.Project = p.Project
		if p.Worktree != nil {
			s.Worktree = *p.Worktree
		}
		s.Revision = 1
		s.Config = *p.Config
		t.sessions[s.ID] = s
		response.Output = protocol.Output{Kind: "session", Data: s}
	case "set_session_team":
		if saved, exists := t.teamOutputs[request.ID]; exists {
			response.Output = saved
			break
		}
		input, _ := detail[protocol.CommandSetSessionExecutionPayload](request.Command.Data)
		session := t.sessions[input.Session]
		if input.ExpectedRevision != session.Revision {
			response.Fault = &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "session changed before team selection"}
			break
		}
		session.Revision++
		session.Roles.Execution = input.Execution
		session.Roles.Models = input.Models
		session.Roles.Profiles = slices.Clone(t.snapshot.Roles)
		if input.Team != nil {
			session.Roles.Team = &protocol.TeamReference{ID: input.Team.ID, Revision: *input.Team.Revision}
			for _, team := range t.snapshot.Teams {
				if team.ID == input.Team.ID && team.Model != nil {
					session.Config.Provider, session.Config.Model = team.Model.Provider, team.Model.Model
				}
			}
		}
		t.sessions[session.ID] = session
		response.Output = protocol.Output{Kind: "session", Data: session}
		if t.teamOutputs == nil {
			t.teamOutputs = map[protocol.RequestID]protocol.Output{}
		}
		t.teamOutputs[request.ID] = response.Output
		if t.teamFailure != "" {
			response = link.Response{Fault: &protocol.Fault{Code: t.teamFailure, Message: "team acknowledgement is unavailable"}}
		}
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
	case "read_team_settings":
		response.Output = protocol.Output{Kind: "team_settings", Data: protocol.DefaultTeamSettings()}
	}
	completion := make(chan link.Response, 1)
	completion <- response
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}, nil
}

func TestComposerSendRouting(t *testing.T) {
	t.Run("first run without a provider", firstRun)
	t.Run("questions and approvals in the window", interactionKinds)
	t.Run("composer parity commands", composerParityCommands)
	t.Run("revoked remote actions never target same-ID local resources", func(t *testing.T) {
		localNode, remoteNode := protocol.NodeID{1}, protocol.NodeID{2}
		project, worktree, session := protocol.ProjectID{3}, protocol.WorktreeID{4}, protocol.SessionID{5}
		transports := make([]*conversationTestTransport, 2)
		owners := make([]*Runtime, 2)
		for index, node := range []protocol.NodeID{localNode, remoteNode} {
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node = node
			snapshot.Projects = []protocol.Project{{ID: project, Name: "Workspace", Path: "/fixture/project"}}
			snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Project: &project, Path: "/fixture/worktree", Main: true}}
			snapshot.Sessions = []protocol.Session{{ID: session, Project: &project, Worktree: worktree, Revision: 7}}
			transports[index] = &conversationTestTransport{snapshot: snapshot}
			controller := client.New(transports[index])
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			defer controller.Close()
			owners[index] = &Runtime{ctx: t.Context(), controller: controller, busy: map[string]bool{}, uncertain: map[string]protocol.Request{}}
		}
		local, remote := owners[0], owners[1]
		localConversation := &viewmodel.Conversation{Node: localNode.String(), ID: session.String(), Revision: 7}
		remoteConversation := &viewmodel.Conversation{Node: remoteNode.String(), ID: session.String(), Revision: 7}
		localProject := &viewmodel.Project{Host: localNode.String(), ID: project.String(), Conversations: []*viewmodel.Conversation{localConversation}}
		remoteProject := &viewmodel.Project{Host: remoteNode.String(), ID: project.String(), Conversations: []*viewmodel.Conversation{remoteConversation}}
		local.App = shell.NewLive(&viewmodel.Data{Projects: []*viewmodel.Project{localProject, remoteProject}, Workspace: &viewmodel.Workspace{}})
		remote.App, remote.parent = local.App, local
		local.bindings = map[string]*Runtime{remoteNode.String(): remote}
		local.newDraft(remoteProject)
		if len(localProject.Conversations) != 1 || len(remoteProject.Conversations) != 2 {
			t.Fatal("remote draft was inserted into a same-ID local project")
		}
		draft := remoteProject.Conversations[0]
		draftComposer := local.App.Composer(draft.ID)
		draftComposer.Draft = "Retained remote draft"
		if local.conversationRuntime(draft, draftComposer) != remote || draft.Node != remoteNode.String() || draftComposer.Host != remoteNode.String() {
			t.Fatal("remote draft project lookup replaced its captured Host")
		}
		local.App.Sidebar.Selected = localConversation.ID
		local.App.OpenProject(remoteProject, shell.ProjectActions{})
		if local.selectedRuntime() != remote {
			t.Fatal("project overview followed the previous local conversation")
		}
		comp := composer.New("")
		captured := &conversationActions{r: remote, cv: remoteConversation, comp: comp}
		nativeEffects := 0
		for _, owner := range owners {
			owner.exportConversation = func(string, string, conversation.ExportFormat) { nativeEffects++ }
			owner.readAloud = func(string, string, string) { nativeEffects++ }
		}
		delete(local.bindings, remoteNode.String())
		if local.runtimeFor(remoteNode.String()) != nil || local.selectedRuntime() != nil {
			t.Fatal("revoked remote owner fell back to the local Runtime")
		}
		if local.runtimeFor("") != local || local.runtimeFor(localNode.String()) != local {
			t.Fatal("empty or explicit local Node lost local ownership")
		}
		local.Refresh()
		renameCompleted, projectCompleted := false, false
		local.rename(remoteConversation, "Captured rename", func(ok bool) { renameCompleted = !ok })
		local.archive(remoteConversation)
		local.read(remoteConversation, true)
		local.removeSession(remoteConversation)
		local.removeProject(remoteProject)
		local.saveProject(sidebar.Editor{Project: remoteProject}, remoteNode.String(), func(ok bool) { projectCompleted = !ok })
		local.forkHere(remoteConversation)
		local.forkWorkspaceConversation(remoteConversation)
		local.exportSession(remoteConversation, conversation.ExportMarkdown)
		if !renameCompleted || !projectCompleted {
			t.Fatal("revoked remote save did not preserve its failed draft")
		}
		tester := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			captured.c = c
			captured.Compact()
			captured.Fork((protocol.TurnID{6}).String())
			captured.Export("", conversation.ExportMarkdown)
			captured.Read("", "Captured text")
			ctx, actions := local.conversation(c, remoteConversation, comp)
			if !ctx.Readonly || remoteConversation.Session.Connection != conversation.Offline {
				t.Fatal("revoked remote conversation was not rendered unavailable")
			}
			actions.Compact()
			actions.Fork((protocol.TurnID{6}).String())
			actions.Export("", conversation.ExportMarkdown)
		}, 640, 400)
		tester.Frame()
		for index, owner := range owners {
			owner.workers.Wait()
			transports[index].mu.Lock()
			requests := len(transports[index].requests)
			transports[index].mu.Unlock()
			if requests != 0 {
				t.Fatalf("revoked action dispatched %d requests to Node %d", requests, index)
			}
		}
		if nativeEffects != 0 {
			t.Fatal("revoked remote action started a native effect")
		}
		freshLocalProject := &viewmodel.Project{Host: localNode.String(), ID: project.String(), Conversations: []*viewmodel.Conversation{localConversation}}
		local.App.ApplyData(&viewmodel.Data{Projects: []*viewmodel.Project{freshLocalProject}, Workspace: &viewmodel.Workspace{}})
		if len(freshLocalProject.Conversations) != 1 {
			t.Fatal("revoked remote draft was retained inside a same-ID local project")
		}
		if retained := local.App.Data.Find(draft.ID); retained != draft || retained.Node != remoteNode.String() || draftComposer.Draft != "Retained remote draft" {
			t.Fatal("revoked peer discarded the remote draft or its captured Node")
		}
	})
	t.Run("file and folder references retain their captured paths", func(t *testing.T) {
		rows := composerFileReferences("docs", []protocol.FileEntry{{Name: "guide.md", Kind: protocol.EntryKindFile}, {Name: "examples", Kind: protocol.EntryKindDirectory}, {Name: "link", Kind: protocol.EntryKindSymlink}})
		if len(rows) != 2 || rows[0].ID != "docs/guide.md" || rows[1].Kind != composer.Folder {
			t.Fatalf("file reference rows = %#v", rows)
		}
		input := conversationInput(composer.Message{Text: "inspect", References: rows})
		if len(input.References) != 2 || input.References[0].Target.Kind != "file" || input.References[1].Target.Kind != "directory" {
			t.Fatalf("captured references = %#v", input.References)
		}
		for index, ref := range input.References {
			got, ok := composerReference(ref)
			if !ok || got.ID != rows[index].ID || got.Kind != rows[index].Kind {
				t.Fatalf("restored reference = %#v, %v", got, ok)
			}
		}
	})
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
		{"team_before_admission", "existing", "submit_turn", false, "", false},
		{"new_team_before_admission", "draft/1", "submit_turn", false, "", false},
		{"uncertain_team_before_admission", "existing", "submit_turn", false, "", false},
		{"replace_references", "existing", "replace_turn", false, "", false},
		{"new_session_after_navigation", "draft/1", "submit_turn", false, "", false},
		{"new_session_base_keeps_later_input", "draft/1", "submit_turn", false, "", false},
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
			team := strings.Contains(row.name, "team_before_admission")
			if team {
				snapshot.Roles = []protocol.RoleProfile{{ID: protocol.RoleID{8}, Revision: 3, Key: "review", Name: "Review"}}
				snapshot.Teams = []protocol.TeamProfile{{ID: protocol.TeamID{8}, Revision: 5, Key: "reviewers", Name: "Reviewers", Members: []protocol.RoleID{{8}}}}
				if row.name == "uncertain_team_before_admission" {
					snapshot.Teams[0].Model = &protocol.RoleModel{Provider: provider, Model: "team-model"}
					snapshot.Providers[0].Models = append(snapshot.Providers[0].Models, protocol.ConversationModel{ID: "team-model", DefaultEffort: protocol.Effort{Kind: "default"}})
				}
			}
			if row.session == "existing" {
				snapshot.Sessions = []protocol.Session{{ID: session, Revision: 6, Worktree: worktree, Project: &project, Config: config, Roles: protocol.DefaultRoleSnapshot()}}
				if row.running {
					snapshot.Sessions[0].Activity.Run = &protocol.ConversationRun{Session: session, Turn: protocol.TurnID{6}, Sequence: 1, Revision: 1, Kind: "task", Status: "running"}
				}
			}
			transport := &conversationTestTransport{snapshot: snapshot, failure: row.failure}
			// based sends from a draft whose base worktree is created first.
			based, baseTree := row.name == "new_session_base_keeps_later_input", protocol.Worktree{ID: protocol.WorktreeID{4}, Project: &project, Path: "/fixture/base"}
			if based {
				transport.handle = func(request protocol.Request) (link.Response, bool) {
					if request.Command.Kind != "create_managed_worktree" {
						return link.Response{}, false
					}
					return link.Response{Output: protocol.Output{Kind: "worktree", Data: baseTree}}, true
				}
			}
			if row.name == "uncertain_team_before_admission" {
				transport.teamFailure = protocol.ErrorCodeUnavailable
			}
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
				sent := protocol.DefaultAttachment()
				sent.ID = protocol.AttachmentID{7}
				transport.conversation.Page.Entries = []protocol.ConversationEntry{{ID: "sent", Sequence: 1, Turn: turn, Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Captured draft @Host"}, {Kind: "reference", Data: protocol.ConversationReference{Target: protocol.ConversationReferenceTarget{Kind: "host"}, Label: "Host"}}, {Kind: "attachment", Data: sent}}}}
				// A reply with one link of each kind, a later turn to rewind before, and the rewind's backup session.
				backup := protocol.SessionID{10}
				later := protocol.TurnID{7}
				transport.conversation.Page.Runs = append(transport.conversation.Page.Runs, protocol.ConversationRun{Session: session, Turn: later, Worktree: worktree, Sequence: 2, Revision: 1, Kind: "task", Status: "completed"})
				transport.conversation.Page.Entries = append(transport.conversation.Page.Entries,
					protocol.ConversationEntry{ID: "reply", Sequence: 2, Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Open [website](https://example.com/page), [backup](sailry://session/" + backup.String() + ") or [source](src/main.go#L12)."}}},
					protocol.ConversationEntry{ID: "later", Sequence: 3, Turn: later, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Later answer"}}})
				transport.snapshot.Sessions = append(transport.snapshot.Sessions, protocol.Session{ID: backup, Revision: 1, Worktree: worktree, Project: &project, Config: config, Roles: protocol.DefaultRoleSnapshot()})
				transport.handle = func(request protocol.Request) (link.Response, bool) {
					if request.Command.Kind != "rewind_conversation" {
						return link.Response{}, false
					}
					input := request.Command.Data.(protocol.CommandRewindConversationPayload)
					saved := transport.snapshot.Sessions[1]
					return link.Response{Output: protocol.Output{Kind: "rewound", Data: protocol.ConversationRewind{Session: session, Revision: 2, Through: input.Through, Backup: &saved}}}, true
				}
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
			comp.Team = defaultTeamID
			if team {
				comp.Team = snapshot.Teams[0].ID.String()
				comp.MemberModels = map[string]*composer.MemberModel{snapshot.Roles[0].ID.String(): {Model: modelKey(provider, "fixture"), Overridden: true}}
			}
			comp.Model = modelKey(provider, "fixture")
			comp.Project = project.String()
			comp.Worktree = "/fixture/worktree"
			comp.Host = node.String()
			if based {
				r.sessionBases[comp] = &sessionBaseStart{base: sessionBase{From: "main", Branch: "feature", Commit: strings.Repeat("a", 40)}, project: project, source: worktree, worktree: comp.Worktree}
			}
			// Browser tabs opened in this draft, in another draft and in no
			// conversation: only the first follows the draft into its session.
			r.browser = &browserState{available: true, tabs: map[string]*browserTab{}, serving: map[*Runtime]context.CancelFunc{r: func() {}},
				documents: map[string]*documentView{}, sources: map[string]protocol.ArtifactContent{}}
			var browserTabs [3]*browserTab
			for index := range browserTabs {
				browserTabs[index] = r.browserTabFor(r.App.Panel.AddBrowser(false).ID)
			}
			browserTabs[1].conversation, browserTabs[2].conversation = "draft/other", ""
			if row.running || row.name == "replace_references" || row.failure == protocol.ErrorCodeOutcomeUnknown {
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
			var frame func(*ui.Context)
			hold := false // keeps queued work, as a page load no headless window can make
			tester := ui.NewTester(func(c *ui.Context) {
				context = c
				if !hold {
					r.drain(c)
				}
				if frame != nil {
					frame(c)
				}
			}, 640, 400)
			tester.SetLocale("en")
			tester.Frame()
			r.workers.Wait()
			tester.Frame()
			actions := &conversationActions{r: r, c: context, cv: cv, comp: comp}
			// shown renders one frame with the conversation on screen, absent one without it.
			shown := func(c *ui.Context) { r.conversation(c, cv, comp); r.finishFrame() }
			absent := func(*ui.Context) { r.finishFrame() }
			render := func(step func(*ui.Context)) {
				frame = step
				tester.Frame()
				frame = nil
			}
			turns := func() []protocol.Request {
				transport.mu.Lock()
				defer transport.mu.Unlock()
				var sent []protocol.Request
				for _, request := range transport.requests {
					if request.Command.Kind == "submit_turn" || request.Command.Kind == "replace_turn" {
						sent = append(sent, request)
					}
				}
				return sent
			}
			message := composer.Message{Text: "Captured draft", Model: comp.Model, Mode: composer.Code, Permission: composer.AskFirst}
			// view draws the transcript as the shell does, so its controls take the pointer.
			view := func(c *ui.Context) {
				_, act := r.conversation(c, cv, comp)
				ui.Column(c).Fill().Children(func() { conversation.View(c, r.App.Messages, id, cv.Turns, &cv.Session, act) })
				r.finishFrame()
			}
			settle := func() {
				for range 4 {
					r.workers.Wait()
					tester.Frame()
				}
			}
			if row.running {
				// A running conversation offers no edit, rewind or fork of its turn.
				if got := actions.controls(protocol.TurnID{6}); got.ReadOnly || !got.NoEdit || !got.NoRewind || !got.NoFork || !got.NoWorktree {
					t.Fatalf("running turn controls = %+v", got)
				}
			}
			if row.configure {
				session, _ := r.session(id)
				config := session.Config
				config.Mode = "plan"
				r.saveComposerConfig(session, config, nil)
				message.Mode = composer.Plan
			}
			creations := func() int {
				transport.mu.Lock()
				defer transport.mu.Unlock()
				total := 0
				for _, request := range transport.requests {
					if request.Command.Kind == "create_session" {
						total++
					}
				}
				return total
			}
			if row.session != "existing" {
				// Until its first send a draft is no session and no sidebar row.
				if creations() != 0 || r.App.Data.Find(id) == nil || r.App.Sidebar.Data.Find(id) != nil || slices.Contains(r.App.Sidebar.Order(), id) {
					t.Fatal("an unsent draft is a session or is listed in the sidebar")
				}
			}
			if row.name == "replace_references" {
				// The message is edited in its bubble; the composer's draft is not involved.
				comp.Draft = "Unsent draft"
				frame = view
				tester.Frame()
				editing := func() bool { return len(r.conversationEdits) == 1 && cv.Turns[0].Editing != nil }
				open := func() {
					t.Helper()
					tester.Click("Edit")
					tester.Frame()
					if !editing() || cv.Turns[0].Editing.Text != "Captured draft @Host" || !tester.HasText("Save") || comp.Draft != "Unsent draft" {
						t.Fatal("Edit did not open the sent message in its bubble", len(r.conversationEdits), comp.Draft)
					}
				}
				closed := func(how string) {
					t.Helper()
					tester.Frame()
					tester.Frame()
					if editing() || len(r.conversationEdits) != 0 || tester.HasText("Save") || len(turns()) != 0 || comp.Draft != "Unsent draft" {
						t.Fatal(how+" did not restore the bubble", len(r.conversationEdits), len(turns()))
					}
				}
				open()
				tester.Key(0, ui.KeyEscape)
				closed("Escape")
				open()
				tester.Click("Cancel")
				closed("Cancel")
				open()
				tester.ClickAt(8, 392) // the focus leaves the editor
				closed("blur")
				// An abandoned editor never turns a composer send into a replacement.
				open()
				actions.Send(message)
				settle()
				if sent := turns(); len(sent) != 1 || sent[0].Command.Kind != "submit_turn" {
					t.Fatal("an open editor changed what the composer sends")
				}
				comp.Draft = "Unsent draft"
				if !editing() {
					open()
				}
				// The transport holds the request while the editor is looked at.
				transport.mu.Lock()
				tester.Click("Save")
				tester.Frame()
				tester.Frame()
				flying := cv.Turns[0].Editing
				transport.mu.Unlock()
				if flying == nil || !flying.Pending {
					t.Fatal("a replacement in flight showed no pending editor")
				}
				settle()
				if len(r.conversationEdits) != 0 || cv.Turns[0].Editing != nil || tester.HasText("Save") || comp.Draft != "Unsent draft" {
					t.Fatal("an accepted replacement left its editor open or touched the draft")
				}
				comp.Draft = ""
			} else if row.running {
				actions.Guide(message)
			} else {
				actions.Send(message)
			}
			if based {
				// Typed while the base worktree is being created.
				comp.Draft = "Typed meanwhile"
				comp.References = []composer.Reference{{ID: "host", Kind: composer.Location, Label: "Host"}}
				comp.Attachments = []*composer.Attachment{{ID: "later", Name: "later.txt", Status: composer.Ready}}
			}
			elsewhere := ""
			if row.name == "new_session_after_navigation" {
				// The user leaves the draft while its session is being created.
				r.newDraft(nil)
				elsewhere = r.App.Sidebar.Selected
			}
			for range 4 {
				r.workers.Wait()
				tester.Frame()
			}
			if row.name == "new_session_after_navigation" {
				// The first send created the one session, now listed; the
				// draft opened meanwhile is still neither.
				if created := (protocol.SessionID{9}).String(); creations() != 1 || r.App.Sidebar.Data.Find(created) == nil || r.App.Sidebar.Data.Find(elsewhere) != nil || r.App.Data.Find(id) != nil {
					t.Fatal("the first send did not turn its draft into the one listed session")
				}
			}
			if created := (protocol.SessionID{9}).String(); row.name == "new_project_session" && r.App.Sidebar.Selected != created {
				t.Fatal("created session did not replace its selected draft")
			} else if elsewhere != "" && (r.App.Sidebar.Selected != elsewhere || r.App.Data.Find(created) == nil) {
				t.Fatal("finished session creation took the selection from the conversation opened meanwhile")
			}
			if row.name == "uncertain_team_before_admission" {
				transport.mu.Lock()
				for _, request := range transport.requests {
					if request.Command.Kind == "submit_turn" || request.Command.Kind == "set_session_config" {
						t.Error("uncertain team selection admitted a later mutation", request.Command.Kind)
					}
				}
				transport.mu.Unlock()
				if comp.Draft != message.Text {
					t.Fatal("uncertain team selection discarded the input")
				}
				comp.Mode, message.Mode = composer.Plan, composer.Plan
				actions.Send(message)
				for range 4 {
					r.workers.Wait()
					tester.Frame()
				}
			}
			transport.mu.Lock()
			var commands []protocol.Request
			for _, request := range transport.requests {
				if request.Command.Kind == "create_session_at" || request.Command.Kind == "create_session" || request.Command.Kind == row.kind {
					commands = append(commands, request)
				}
			}
			transport.mu.Unlock()
			if team {
				transport.mu.Lock()
				var selected, sent int
				var teamRequest protocol.RequestID
				for index, request := range transport.requests {
					if request.Command.Kind == "set_session_team" {
						if row.name == "uncertain_team_before_admission" && selected > 0 && request.ID != teamRequest {
							t.Error("uncertain team selection changed request identity")
						}
						teamRequest = request.ID
						selected = index + 1
						input, _ := detail[protocol.CommandSetSessionExecutionPayload](request.Command.Data)
						if input.Team == nil || input.Team.ID != snapshot.Teams[0].ID || input.Team.Revision == nil || *input.Team.Revision != 5 || len(input.Models) != 1 {
							t.Errorf("team admission lost captured selection: %#v", input)
						}
					}
					if request.Command.Kind == "submit_turn" {
						sent = index + 1
						input, _ := detail[protocol.CommandSubmitTurnPayload](request.Command.Data)
						expected := uint64(7)
						if row.session != "existing" {
							expected = 2
						} else if row.name == "uncertain_team_before_admission" {
							expected = 8
						}
						if input.ExpectedRevision != expected {
							t.Errorf("send did not use saved team revision: %#v", input)
						}
					}
					if request.Command.Kind == "set_session_config" && row.name == "uncertain_team_before_admission" {
						input, _ := detail[protocol.CommandSetSessionConfigPayload](request.Command.Data)
						if input.Config.Model != "team-model" || input.Config.Mode != protocol.WorkModePlan {
							t.Errorf("team acknowledgement lost the selected lead or explicit mode: %#v", input.Config)
						}
					}
				}
				transport.mu.Unlock()
				if selected == 0 || selected >= sent {
					t.Fatal("turn admitted before its captured team", selected, sent)
				}
			}
			if row.session != "existing" {
				// A session on the Node that holds its provider is created by
				// that Node itself.
				if len(commands) != 2 || commands[0].Command.Kind != "create_session" || commands[0].Target != node {
					t.Fatalf("new session routing = %#v", commands)
				}
				create := commands[0].Command.Data.(protocol.CommandCreateSessionPayload)
				started := worktree
				if based {
					started = baseTree.ID
				}
				if create.Project == nil || *create.Project != project || create.Worktree == nil || *create.Worktree != started || create.Config == nil || create.Config.Provider != provider || create.Config.Model != "fixture" {
					t.Fatalf("captured project/Node scope = %#v", create)
				}
				// The session owns its draft's browser tab and no other: an agent
				// call of the selected session rebinds nothing.
				created := (protocol.SessionID{9}).String()
				r.App.Sidebar.Selected = created
				listed := r.planBrowserCall(node, created, browserRequest{Action: "tabs"})
				var answer struct {
					Tabs []struct{ ID uint64 }
				}
				if listed.result == nil || listed.result.Ok == nil || json.Unmarshal(*listed.result.Ok, &answer) != nil || len(answer.Tabs) != 1 || answer.Tabs[0].ID != browserTabs[0].id {
					t.Fatalf("the created session's browser tabs = %+v", listed.result)
				}
				if browserTabs[0].conversation != created || browserTabs[1].conversation != "draft/other" || browserTabs[2].conversation != "" {
					t.Fatalf("browser tabs changed owner: %q, %q, %q", browserTabs[0].conversation, browserTabs[1].conversation, browserTabs[2].conversation)
				}
				if elsewhere != "" {
					r.App.Sidebar.Selected = elsewhere
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
				if len(p.Message.Attachments) != 1 || p.Message.Attachments[0] != (protocol.AttachmentID{7}) {
					t.Fatal("ReplaceTurn dropped the attachment of the edited message")
				}
				// A rejected replacement keeps the editor and its text, and toasts once.
				for _, failure := range []protocol.ErrorCode{protocol.ErrorCodeConflict, protocol.ErrorCodeUnavailable, protocol.ErrorCodeOutcomeUnknown} {
					tester.Click("Edit")
					tester.Frame()
					tester.Type(" more")
					transport.mu.Lock()
					transport.failure = failure
					transport.mu.Unlock()
					before := len(turns())
					tester.Click("Save")
					settle()
					sent := turns()
					if len(sent) != before+1 || sent[len(sent)-1].Command.Kind != "replace_turn" {
						t.Fatal("Save did not send the replacement", failure)
					}
					if got := sent[len(sent)-1].Command.Data.(protocol.CommandReplaceTurnPayload); got.Message.Text != "Captured draft @Host more" || len(got.Message.References) != 1 || len(got.Message.Attachments) != 1 {
						t.Fatalf("edited replacement = %#v", got.Message)
					}
					if edit := cv.Turns[0].Editing; edit == nil || edit.Pending || !tester.HasText("Save") || comp.Draft != "" {
						t.Fatal("a rejected replacement closed its editor or touched the draft", failure)
					}
					transport.mu.Lock()
					transport.failure = ""
					transport.mu.Unlock()
					tester.Click("Cancel")
					tester.Frame()
					tester.Frame()
				}
				// A message of a conversation that left the screen is no longer edited.
				tester.Click("Edit")
				tester.Frame()
				render(absent)
				if len(r.conversationEdits) != 0 {
					t.Fatal("an edit survived leaving its conversation")
				}
				comp.Draft = ""
				frame = view
				tester.Frame()
				// Each link of a reply goes where its kind belongs.
				backup := (protocol.SessionID{10}).String()
				tabs := len(r.App.Panel.Tabs)
				hold = true
				if err := tester.Click("website"); err != nil {
					t.Fatal(err)
				}
				tester.Frame()
				r.mu.Lock()
				loads := len(r.queue)
				r.queue = nil
				r.mu.Unlock()
				hold = false
				if len(r.App.Panel.Tabs) != tabs+1 || r.App.Panel.Active.Kind != panel.Browser || !r.App.PanelOpen || loads != 1 || len(tester.OpenedURLs()) != 0 {
					t.Fatal("a web link did not open in a browser tab", len(r.App.Panel.Tabs), loads, tester.OpenedURLs())
				}
				if err := tester.ClickWith(ui.Cmd, "website"); err != nil {
					t.Fatal(err)
				}
				tester.Frame()
				if opened := tester.OpenedURLs(); len(r.App.Panel.Tabs) != tabs+1 || len(opened) != 1 || opened[0] != "https://example.com/page" {
					t.Fatal("a modifier click did not open the system browser", opened)
				}
				for link, want := range map[string]fileTarget{
					"src/main.go#L12":                   {workspace: true, path: "src/main.go", line: 12},
					"src/main.go:7:3":                   {workspace: true, path: "src/main.go", line: 7},
					"file:///fixture/worktree/a%20b.md": {workspace: true, path: "a b.md"},
					"/etc/hosts":                        {path: "/etc/hosts"},
					"./":                                {workspace: true},
				} {
					if got, ok := resolveFileLink(normalizeLink(link), "/fixture/worktree"); !ok || got != want {
						t.Fatalf("file link %q = %+v, %v", link, got, ok)
					}
				}
				// A workspace file link shows the session's workspace and opens the file there at its line.
				if err := tester.Click("source"); err != nil {
					t.Fatal(err)
				}
				tester.Frame()
				if tab := r.App.Panel.Active; r.workspace == nil || r.workspace.projects[worktree] == nil || tab.Kind != panel.Files || tab.ID != "src/main.go" {
					t.Fatalf("a workspace file link opened %+v", tab)
				}
				// A session link carries the agent's icon before its text.
				if err := tester.Click("\uFFFC\u2060backup"); err != nil {
					t.Fatal(err, tester.Texts())
				}
				tester.Frame()
				if r.App.Sidebar.Selected != backup {
					t.Fatal("a session link did not open its session", r.App.Sidebar.Selected)
				}
				r.OpenSession(id)
				tester.Frame()
				// Rewinding to the first turn reports once, and its toast opens the backup session.
				if got := actions.controls(protocol.TurnID{7}); !got.NoRewind || got.NoFork || got.ReadOnly {
					t.Fatalf("head turn controls = %+v", got)
				}
				if err := tester.Click(i18n.In("en", "chat_fork")); err != nil {
					t.Fatal(err)
				}
				tester.Frame()
				if err := tester.Click(i18n.In("en", "chat_rewind")); err != nil {
					t.Fatal(err)
				}
				tester.Frame()
				tester.Frame()
				if err := tester.Click(i18n.In("en", "chat_rewind_action")); err != nil {
					t.Fatal(err)
				}
				settle()
				if !tester.HasText(i18n.In("en", "chat_rewound")) {
					t.Fatal("a rewind showed no outcome", tester.Texts())
				}
				if err := tester.Click(i18n.In("en", "chat_rewind_backup")); err != nil {
					t.Fatal(err)
				}
				tester.Frame()
				if r.App.Sidebar.Selected != backup {
					t.Fatal("the rewind toast did not open the backup session", r.App.Sidebar.Selected)
				}
				r.OpenSession(id)
				tester.Frame()
				// A change in the shown conversation redraws it; only a Client change projects the Node snapshot.
				render(shown)
				r.mu.Lock()
				r.dirty = false
				r.mu.Unlock()
				r.conversationsStale.Store(false)
				_ = controller.LoadOlderConversation(t.Context(), session)
				r.mu.Lock()
				dirty := r.dirty
				r.mu.Unlock()
				if !r.conversationsStale.Load() || dirty {
					t.Fatal("a change in the shown conversation projected the Node snapshot again", dirty)
				}
			}
			if row.configure {
				p := commands[len(commands)-1].Command.Data.(protocol.CommandSubmitTurnPayload)
				if p.ExpectedRevision != 7 {
					t.Fatal("admission did not use the confirmed config revision")
				}
			}
			if row.name == "replace_references" {
				frame = nil
			}
			if row.name == "failure_retains_draft" {
				// A send that fails after its text left the composer offers it back from the toast.
				transport.mu.Lock()
				actions.Send(message)
				comp.Draft = ""
				transport.mu.Unlock()
				settle()
				if err := tester.Click(i18n.In("en", "chat_restore_draft")); err != nil {
					t.Fatal(err, tester.Texts())
				}
				tester.Frame()
				tester.Frame()
				if comp.Draft != "Captured draft" {
					t.Fatal("Restore draft did not put the failed submission back", comp.Draft)
				}
			}
			if row.name == "replace_references" {
				// The selection's New conversation opens a draft quoting it; this conversation keeps its own.
				actions.Start(conversation.Quote("Later answer"))
				if started := r.App.Sidebar.Selected; started == id || r.App.Composer(started).Draft != "> Later answer\n\n" || comp.Draft != "" {
					t.Fatal("New conversation did not start a draft with the quote", started)
				}
				r.OpenSession(id)
				tester.Frame()
			}
			if row.failure != "" && comp.Draft != "Captured draft" {
				t.Fatal("failed request discarded the draft")
			}
			if based {
				// The first submission was sent as captured, and what was typed
				// while its worktree was created is still the composer's.
				sent := commands[len(commands)-1].Command.Data.(protocol.CommandSubmitTurnPayload)
				if sent.Message.Text != "Captured draft" || len(sent.Message.References) != 0 || len(sent.Message.Attachments) != 0 {
					t.Fatalf("the send after its base worktree = %#v", sent.Message)
				}
				if comp.Draft != "Typed meanwhile" || len(comp.References) != 1 || len(comp.Attachments) != 1 || comp.Attachments[0].ID != "later" {
					t.Fatalf("input typed while the base worktree was created was lost: %q, %d references, %d attachments", comp.Draft, len(comp.References), len(comp.Attachments))
				}
			} else if row.failure == "" && comp.Draft != "" {
				t.Fatal("successful request retained the submitted draft")
			}
			if row.failure == protocol.ErrorCodeOutcomeUnknown {
				first := commands[len(commands)-1].ID
				actions.Send(message)
				r.workers.Wait()
				tester.Frame()
				// The transport also carries the submission's feature counts.
				retried := turns()
				if last := retried[len(retried)-1]; last.ID != first {
					t.Fatal("explicit retry changed uncertain request identity")
				}
				again := func() protocol.RequestID {
					actions.Send(message)
					r.workers.Wait()
					tester.Frame()
					sent := turns()
					return sent[len(sent)-1].ID
				}
				render(shown)
				if again() != first {
					t.Fatal("an unchanged draft and history lost the uncertain request")
				}
				comp.Draft = "Captured draft, edited"
				render(shown)
				comp.Draft = "Captured draft"
				second := again()
				if second == first {
					t.Fatal("the uncertain request survived a changed draft")
				}
				state, _ := controller.Conversation(session)
				moved := state.Snapshot.Page
				moved.Runs = append(moved.Runs, protocol.ConversationRun{Session: session, Turn: protocol.TurnID{7}, Status: "completed"})
				r.settleTurnRetry(id, comp, nil, moved)
				third := again()
				if third == second {
					t.Fatal("a later send with the same text reused the uncertain request after the history moved")
				}
				r.settleTurnRetry(id, comp, []protocol.TurnSummary{{ID: protocol.TurnID{8}, Session: session, Request: third}}, state.Snapshot.Page)
				if comp.Draft != "" || len(r.turnRetries) != 0 || len(r.uncertain) != 0 {
					t.Fatal("an admitted turn left its uncertain request or draft behind")
				}
				// The Node answers that a restart interrupted the request. That is
				// final, so the request is dropped and the next send is a new one.
				transport.mu.Lock()
				previous := transport.handle
				transport.handle = func(request protocol.Request) (link.Response, bool) {
					var query protocol.CommandInspectRequestPayload
					switch data := request.Command.Data.(type) {
					case protocol.CommandInspectRequestPayload:
						query = data
					case *protocol.CommandInspectRequestPayload:
						query = *data
					default:
						return link.Response{}, false
					}
					return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: query.ID, Outcome: protocol.RequestOutcome{Kind: "unknown"}}}}, true
				}
				transport.mu.Unlock()
				fourth := again()
				if len(r.turnRetries) != 0 || len(r.uncertain) != 0 {
					t.Fatal("a request the Node can never answer was kept for retry")
				}
				if again() == fourth {
					t.Fatal("a released request was sent again")
				}
				transport.mu.Lock()
				transport.handle = previous
				transport.mu.Unlock()
			}
			if slices.ContainsFunc(r.App.Data.Models, func(m composer.Model) bool { return m.Name == "" }) {
				t.Fatal("live model projection contains an empty sample")
			}
			// Readers share one copy of the Node snapshot until the Client reports a change.
			if first, again := r.snapshot(), r.snapshot(); len(first.Providers) == 0 || &first.Providers[0] != &again.Providers[0] {
				t.Fatal("the Node snapshot was copied again without a Client change")
			}
			if row.running {
				// The Host lists the running session and finds its lanes once per Node snapshot.
				listed := r.hosts[node.String()]
				if listed == nil || len(listed.view.Activity) != 1 || listed.view.Activity[0].Session != id || listed.view.Activity[0].Waiting {
					t.Fatalf("Host activity = %#v", listed)
				}
				found := &listed.active[0]
				tester.Frame()
				if len(listed.view.Activity) != 1 || &listed.active[0] != found {
					t.Fatal("an unchanged Node snapshot had its Host lanes found again")
				}
			}
			// An identical package command in flight is not sent again, and its second caller is told so.
			remove := protocol.Command{Kind: "remove_plugin", Data: protocol.CommandRemovePluginPayload{Name: "fixture"}}
			r.busy["packages/"+packageCommandKey(remove)] = true
			refused := 0
			r.packageCommand(remove, func(_ protocol.Output, ok bool) {
				if !ok {
					refused++
				}
			})
			delete(r.busy, "packages/"+packageCommandKey(remove))
			if refused != 1 {
				t.Fatal("a package command dropped as a duplicate left its caller waiting")
			}
			if row.session == "existing" {
				// A session the Node no longer has leaves nothing behind; a draft keeps its state.
				r.composerInitialized[session.String()], r.composerInitialized["draft/9"] = true, true
				r.composerRevisions[session.String()], r.conversationEdits[session.String()] = 7, conversationEdit{}
				r.pruneSessions(r.snapshot())
				if !r.composerInitialized[session.String()] || len(r.conversationEdits) != 1 {
					t.Fatal("a session the Node still has lost its state")
				}
				r.pruneSessions(protocol.Snapshot{})
				if len(r.composerInitialized) != 1 || !r.composerInitialized["draft/9"] || len(r.composerRevisions) != 0 || len(r.composerObserved) != 0 || len(r.conversationEdits) != 0 {
					t.Fatal("a removed session left its state behind", r.composerInitialized, r.composerRevisions)
				}
			}
		})
	}
	t.Run("a session on another Node is forwarded by the local Node", testSessionForwarding)
	t.Run("a failed installation removal keeps its row and toasts", testInstallationRemoval)
	t.Run("python preparations keep the stages the page names", func(t *testing.T) {
		got := pythonPreparations([]protocol.PythonPreparation{
			{Package: "charts", Stage: protocol.PythonStageInstalling},
			{Package: "later", Stage: "linking"},
			{Package: "ocr", Stage: protocol.PythonStageCreating},
		})
		if len(got) != 2 || got[0] != (settings.PythonPreparation{Package: "charts", Stage: "installing"}) || got[1] != (settings.PythonPreparation{Package: "ocr", Stage: "creating"}) {
			t.Fatalf("preparations: %+v", got)
		}
	})
	t.Run("projected_default_effort", func(t *testing.T) {
		provider := protocol.ProviderID{1}
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = protocol.NodeID{1}
		source := protocol.ConversationPresetsSource{Kind: "builtin"}
		snapshot.Providers = []protocol.ConversationProvider{{ID: provider, Enabled: true, Preset: &protocol.ConversationPresetsPresetRef{Source: source, ID: "fixture"}, Models: []protocol.ConversationModel{{ID: "reasoning", Reasoning: true, Efforts: []protocol.Effort{{Kind: "low"}, {Kind: "high"}}, DefaultEffort: protocol.Effort{Kind: "medium"}}}}}
		snapshot.Plugins = []protocol.PluginSummary{{Name: "first"}, {Name: "second"}}
		transport := &conversationTestTransport{snapshot: snapshot}
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
		comp := composer.New("")
		comp.Mode, comp.Permission, comp.Effort = composer.Plan, composer.FullAccess, 0
		r.defaultComposer(comp)
		if comp.Model != modelKey(provider, "reasoning") || comp.EffortName(nil, comp.Chosen(r.App.Data.Models)) != "medium" || comp.Mode != composer.Plan || comp.Permission != composer.FullAccess {
			t.Fatalf("projected model default = %#v", comp)
		}
		// Connector packages are read one pass at a time: calls during a pass ask for one more.
		settle := ui.NewTester(func(c *ui.Context) {
			r.mu.Lock()
			queue := r.queue
			r.queue = nil
			r.mu.Unlock()
			for _, complete := range queue {
				complete(c)
			}
		}, 640, 400)
		reads := func() (count int) {
			for range 4 {
				r.workers.Wait()
				settle.Frame()
			}
			transport.mu.Lock()
			defer transport.mu.Unlock()
			for _, request := range transport.requests {
				if request.Command.Kind == "read_plugin" {
					count++
				}
			}
			return count
		}
		before := reads()
		for range 3 {
			r.readConnectorPackages()
		}
		if passes := (reads() - before) / len(snapshot.Plugins); passes != 2 || r.authorizations.packagesReading || r.authorizations.packagesStale {
			t.Fatal("connector package reads were not coalesced", passes)
		}
		// A provider saved without a route shows the first route of its preset in every projection.
		entry := protocol.ConversationPresetsEntry{Source: source, Preset: protocol.ConversationPresetsPreset{ID: "fixture", Routes: make([]protocol.ConversationPresetsRoute, 4)}}
		r.providerPresets = map[string]protocol.ConversationPresetsEntry{"builtin:fixture/3": entry, "builtin:fixture/0": entry, "builtin:fixture/2": entry, "builtin:fixture/1": entry}
		for range 20 {
			r.projectProviderSnapshot()
			if channels := r.App.Settings.Channels; len(channels) != 1 || channels[0].Method != "builtin:fixture/0" {
				t.Fatalf("provider without a saved route = %#v", channels)
			}
		}
	})
}

func (t *conversationTestTransport) Open(context.Context, protocol.StreamID) (link.Stream, error) {
	return nil, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "fixture stream is unavailable"}
}

// testSessionForwarding sends from drafts whose Host is another Node. A
// model of the local Node's provider is forwarded by the local Node, which
// holds the provider and its credential; a model of the other Node's own
// provider is created there. A refused or unconfirmed forward keeps the
// draft and toasts its outcome.
func testSessionForwarding(t *testing.T) {
	localNode, remoteNode := protocol.NodeID{1}, protocol.NodeID{2}
	project, worktree := protocol.ProjectID{3}, protocol.WorktreeID{4}
	localProvider, remoteProvider, private := protocol.ProviderID{5}, protocol.ProviderID{6}, protocol.ProviderID{7}
	credential := &protocol.CredentialRef{Node: localNode, ID: protocol.CredentialID{8}}
	provider := func(id protocol.ProviderID, name string) protocol.ConversationProvider {
		return protocol.ConversationProvider{ID: id, Revision: 7, Enabled: true, Name: name, Api: "responses", Authentication: "api_key",
			Models: []protocol.ConversationModel{{ID: "fixture", DefaultEffort: protocol.Effort{Kind: "default"}}}}
	}
	held := provider(localProvider, "Local")
	held.Credential = credential
	transports := make([]*conversationTestTransport, 2)
	controllers := make([]*client.Client, 2)
	for index, node := range []protocol.NodeID{localNode, remoteNode} {
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = node
		if index == 0 {
			snapshot.Providers = []protocol.ConversationProvider{held}
		} else {
			snapshot.Providers = []protocol.ConversationProvider{provider(remoteProvider, "Remote")}
			snapshot.Projects = []protocol.Project{{ID: project, Name: "Workspace", Path: "/fixture/project"}}
			snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Project: &project, Path: "/fixture/worktree", Main: true}}
		}
		transports[index] = &conversationTestTransport{snapshot: snapshot}
		transports[index].conversation = protocol.DefaultConversationSnapshot()
		transports[index].conversation.Node = node
		controllers[index] = client.New(transports[index])
		if err := controllers[index].Start(t.Context()); err != nil {
			t.Fatal(err)
		}
		defer controllers[index].Close()
	}
	// refusal answers the next forward; forwarded counts the sessions made.
	var refusal protocol.ErrorCode
	forwarded := byte(0)
	transports[0].handle = func(request protocol.Request) (link.Response, bool) {
		if request.Command.Kind != "create_session_at" {
			return link.Response{}, false
		}
		if refusal != "" {
			return link.Response{Fault: &protocol.Fault{Code: refusal, Message: "Claude sign-in is required on the execution Node"}}, true
		}
		input := request.Command.Data.(protocol.CommandCreateSessionAtPayload)
		forwarded++
		session := protocol.DefaultSession()
		session.ID, session.Project, session.Worktree, session.Revision = protocol.SessionID{20 + forwarded}, input.Project, *input.Worktree, 1
		copied := held
		copied.ID, copied.Credential = private, &protocol.CredentialRef{Node: remoteNode, ID: protocol.CredentialID{9}}
		session.Config = *input.Config
		session.Config.Provider, session.Config.Credential = copied.ID, copied.Credential
		session.Profile = &protocol.SessionProfile{Source: localNode, Provider: copied}
		return link.Response{Output: protocol.Output{Kind: "session", Data: session}}, true
	}
	local, err := NewWithPreferences(t.Context(), controllers[0], preferences.Open(t.TempDir()+"/preferences.json"))
	if err != nil {
		t.Fatal(err)
	}
	defer local.Close()
	remote := local.newBinding(controllers[1])
	defer remote.Close()
	local.bindings = map[string]*Runtime{remoteNode.String(): remote}
	local.project()
	var context *ui.Context
	var toasts []string
	// rendered is the conversation on screen, if any.
	var rendered *viewmodel.Conversation
	tester := ui.NewTester(func(c *ui.Context) {
		context = c
		local.drain(c)
		if rendered != nil {
			local.conversation(c, rendered, local.App.Composer(rendered.ID))
			local.finishFrame()
		}
		ui.ToastViewportBase(c, func(_ ui.Element, shown []ui.Toast) {
			toasts = toasts[:0]
			for _, toast := range shown {
				toasts = append(toasts, toast.Title)
			}
		})
	}, 640, 400)
	tester.SetLocale("en")
	settle := func() {
		for range 4 {
			local.workers.Wait()
			remote.workers.Wait()
			tester.Frame()
		}
	}
	settle()
	sent := func(index int, kind string) (found []protocol.Request) {
		transports[index].mu.Lock()
		defer transports[index].mu.Unlock()
		for _, request := range transports[index].requests {
			if request.Command.Kind == kind {
				found = append(found, request)
			}
		}
		return
	}
	// draft is a new draft on the other Node's project with model chosen.
	draft := func(model string) (*viewmodel.Conversation, *composer.State, *conversationActions) {
		t.Helper()
		var shown *viewmodel.Project
		for _, candidate := range local.App.Data.Projects {
			if candidate.Host == remoteNode.String() {
				shown = candidate
			}
		}
		if shown == nil {
			t.Fatal("the other Node's project is not shown")
		}
		local.newDraft(shown)
		cv := local.App.Data.Find(local.App.Sidebar.Selected)
		comp := local.App.Composer(cv.ID)
		comp.Host, comp.Project, comp.Worktree, comp.Model = remoteNode.String(), project.String(), "/fixture/worktree", model
		comp.Team = defaultTeamID
		if local.conversationRuntime(cv, comp) != remote {
			t.Fatal("the draft is not the other Node's")
		}
		return cv, comp, &conversationActions{r: remote, c: context, cv: cv, comp: comp}
	}
	send := func(actions *conversationActions, comp *composer.State) {
		actions.c = context
		actions.Send(composer.Message{Text: "Captured draft", Model: comp.Model, Mode: composer.Code, Permission: composer.AskFirst})
		settle()
	}
	offered := func(cv *viewmodel.Conversation) (ids []string) {
		for _, model := range remote.offeredModels(cv.ID, remote.models()) {
			ids = append(ids, model.ID)
		}
		return
	}

	cv, comp, actions := draft(modelKey(localProvider, "fixture"))
	if got, want := offered(cv), []string{modelKey(remoteProvider, "fixture"), modelKey(localProvider, "fixture")}; !slices.Equal(got, want) {
		t.Fatalf("a draft on another Node offers %q, want %q", got, want)
	}
	// A team member there takes only that Node's own models; the local Node
	// has no narrower list.
	members := remote.memberModels(remote.offeredModels(cv.ID, remote.models()))
	if len(members) != 1 || members[0].ID != modelKey(remoteProvider, "fixture") {
		t.Fatalf("a member on another Node is offered %v, want only %s", members, modelKey(remoteProvider, "fixture"))
	}
	if own := local.memberModels(local.models()); own != nil {
		t.Fatalf("a member on the local Node is offered a narrower list: %v", own)
	}
	for _, outcome := range []struct {
		code  protocol.ErrorCode
		toast string
	}{
		{protocol.ErrorCodeNotConfigured, "chat_config_unavailable"},
		{protocol.ErrorCodeRevisionConflict, "session_changed"},
		{protocol.ErrorCodeOutcomeUnknown, "chat_action_unknown"},
	} {
		transports[0].mu.Lock()
		refusal = outcome.code
		transports[0].mu.Unlock()
		before := len(sent(0, "create_session_at"))
		send(actions, comp)
		requests := sent(0, "create_session_at")
		if len(requests) != before+1 || !slices.Contains(toasts, i18n.In("en", outcome.toast)) {
			t.Fatalf("%s: %d forwards, toasts %q", outcome.code, len(requests)-before, toasts)
		}
		if comp.Draft != "Captured draft" || !strings.HasPrefix(cv.ID, "draft/") || len(sent(1, "submit_turn")) != 0 {
			t.Fatalf("%s: draft %q, conversation %s, %d turns", outcome.code, comp.Draft, cv.ID, len(sent(1, "submit_turn")))
		}
	}
	// The unconfirmed forward is sent again as the request it was.
	transports[0].mu.Lock()
	refusal = ""
	transports[0].mu.Unlock()
	send(actions, comp)
	requests := sent(0, "create_session_at")
	last := requests[len(requests)-1]
	if last.ID != requests[len(requests)-2].ID || last.Target != localNode {
		t.Fatalf("the retried forward: request %v after %v, to %v", last.ID, requests[len(requests)-2].ID, last.Target)
	}
	create := last.Command.Data.(protocol.CommandCreateSessionAtPayload)
	if create.Target != remoteNode || create.ProviderRevision != 7 || create.Project == nil || *create.Project != project || create.Worktree == nil || *create.Worktree != worktree ||
		create.Config == nil || create.Config.Provider != localProvider || create.Config.Model != "fixture" || create.Config.Credential == nil || *create.Config.Credential != *credential {
		t.Fatalf("forwarded creation = %+v, config %+v", create, create.Config)
	}
	if len(sent(1, "create_session_at"))+len(sent(1, "create_session")) != 0 {
		t.Fatal("the other Node was asked to create the forwarded session")
	}
	created := (protocol.SessionID{21}).String()
	turns := sent(1, "submit_turn")
	if cv.ID != created || comp.Draft != "" || len(turns) != 1 || turns[0].Target != remoteNode {
		t.Fatalf("after the forward: conversation %s, draft %q, turns %+v", cv.ID, comp.Draft, turns)
	}
	// The session shows the private provider it was given and sends on with it.
	if comp.Model != modelKey(private, "fixture") {
		t.Fatalf("the forwarded session's model = %q", comp.Model)
	}
	if got, want := offered(cv), []string{modelKey(remoteProvider, "fixture"), modelKey(private, "fixture")}; !slices.Equal(got, want) {
		t.Fatalf("the forwarded session offers %q, want %q", got, want)
	}
	rendered = cv
	settle()
	if comp.Model != modelKey(private, "fixture") {
		t.Fatalf("the forwarded session's model on screen = %q", comp.Model)
	}
	send(actions, comp)
	rendered = nil
	if turns = sent(1, "submit_turn"); len(turns) != 2 || len(sent(1, "set_session_config")) != 0 || len(sent(0, "create_session_at")) != len(requests) {
		t.Fatalf("a later send: %d turns, %d configuration saves", len(turns), len(sent(1, "set_session_config")))
	}

	// The other Node's own provider creates the session there.
	cv, comp, actions = draft(modelKey(remoteProvider, "fixture"))
	send(actions, comp)
	own := sent(1, "create_session")
	if len(own) != 1 || own[0].Target != remoteNode || len(sent(0, "create_session_at")) != len(requests) || len(sent(0, "create_session")) != 0 {
		t.Fatalf("creation with the Node's own provider: %+v", own)
	}
	if config := own[0].Command.Data.(protocol.CommandCreateSessionPayload).Config; config == nil || config.Provider != remoteProvider {
		t.Fatalf("own creation config = %+v", config)
	}
	if cv.ID != (protocol.SessionID{9}).String() || comp.Draft != "" {
		t.Fatalf("after the own creation: conversation %s, draft %q", cv.ID, comp.Draft)
	}

	// On another Host the model list says whose configuration each provider
	// is. The composer offers no host choice (docs/removed-ui.md): its
	// context names no Hosts and the bar has no control named after one.
	cv, comp, _ = draft(modelKey(remoteProvider, "fixture"))
	// The fixture has no network to list its paired Hosts from: the frame
	// names them.
	paired := []viewmodel.Host{{ID: localNode.String(), Name: "local_host_name", Local: true}, {ID: remoteNode.String(), Name: "Build box"}}
	var shown *composer.Context
	window := ui.NewTesterWith(func(c *ui.Context) {
		local.drain(c)
		local.App.Data.Hosts = paired
		ctx, owner := local.conversation(c, cv, comp)
		shown = ctx
		ui.Column(c).Fill().Justify(ui.End).Children(func() { composer.View(c, comp, ctx, owner) })
		local.finishFrame()
	}, 960, 520, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	window.SetLocale("zh-CN")
	window.SetDark(true)
	for range 4 {
		local.workers.Wait()
		remote.workers.Wait()
		window.Advance(100 * time.Millisecond)
		window.Frame()
	}
	var providers []string
	for _, model := range shown.Models {
		providers = append(providers, model.Provider)
	}
	desktop := i18n.In("zh-CN", "chat_config_desktop")
	if want := []string{"Remote · Build box", "Local · " + desktop}; !slices.Equal(providers, want) {
		t.Fatalf("a draft on another Host names its providers %q, want %q", providers, want)
	}
	if len(shown.Hosts) != 0 {
		t.Errorf("the composer is offered a host choice: %+v", shown.Hosts)
	}
	for _, e := range window.AccessAll() {
		if e.Role != ui.RoleText && (e.Name == "Build box" || e.Name == i18n.In("zh-CN", "local_host_name")) {
			t.Errorf("the composer has a control named after the host %q", e.Name)
		}
	}
	if dir := os.Getenv("SAILRY_UI_SHOTS"); dir != "" {
		if file, err := os.Create(filepath.Join(dir, "parity-live-provider-sources.png")); err == nil {
			_ = png.Encode(file, window.Image())
			file.Close()
		}
	}
}
