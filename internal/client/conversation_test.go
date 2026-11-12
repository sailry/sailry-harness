package client

import (
	"context"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func conversationFixture() protocol.ConversationSnapshot {
	s := protocol.DefaultConversationSnapshot()
	s.Node = protocol.NodeID{1}
	s.Sequence = 10
	s.Page.Session = protocol.SessionID{1}
	s.Page.Revision = 1
	s.Page.Runs = []protocol.ConversationRun{{Session: s.Page.Session, Turn: protocol.TurnID{1}, Sequence: 1, Revision: 1, Kind: "task", Status: "running"}}
	s.Page.Entries = []protocol.ConversationEntry{{ID: "user", Turn: protocol.TurnID{1}, Sequence: 1, Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Request"}}}}
	return s
}

func conversationFrame(s protocol.ConversationSnapshot, kind string, data any) protocol.Update {
	return protocol.Update{Kind: "conversation_frame", Data: protocol.ConversationFrame{Node: s.Node, Session: s.Page.Session, Sequence: s.Sequence + 1, Change: protocol.ConversationChange{Kind: kind, Data: data}}}
}

func testConversationProjectionOrder(t *testing.T) {
	t.Run("bounded_search_and_large_turn_reveal", testConversationSearchAndContinuation)
	for _, row := range []struct {
		name   string
		change func(*protocol.ConversationSnapshot, *protocol.Update)
		want   Apply
		code   protocol.ErrorCode
	}{
		{"stale_generation", func(_ *protocol.ConversationSnapshot, _ *protocol.Update) {}, Ignored, ""},
		{"wrong_node", func(_ *protocol.ConversationSnapshot, u *protocol.Update) {
			v := u.Data.(protocol.ConversationFrame)
			v.Node = protocol.NodeID{2}
			u.Data = v
		}, Ignored, protocol.ErrorCodeWrongTarget},
		{"wrong_session", func(_ *protocol.ConversationSnapshot, u *protocol.Update) {
			v := u.Data.(protocol.ConversationFrame)
			v.Session = protocol.SessionID{2}
			u.Data = v
		}, Ignored, protocol.ErrorCodeWrongTarget},
		{"duplicate_frame", func(s *protocol.ConversationSnapshot, u *protocol.Update) {
			v := u.Data.(protocol.ConversationFrame)
			v.Sequence = s.Sequence
			u.Data = v
		}, Ignored, ""},
		{"sequence_gap", func(s *protocol.ConversationSnapshot, u *protocol.Update) {
			v := u.Data.(protocol.ConversationFrame)
			v.Sequence = s.Sequence + 2
			u.Data = v
		}, Recover, ""},
		{"canonical_identity", func(s *protocol.ConversationSnapshot, u *protocol.Update) {
			v := s.Page.Entries[0]
			v.Author = "assistant"
			*u = conversationFrame(*s, "entry", v)
		}, Ignored, protocol.ErrorCodeInvalidRequest},
		{"run_identity", func(s *protocol.ConversationSnapshot, u *protocol.Update) {
			v := s.Page.Runs[0]
			v.Revision++
			*u = conversationFrame(*s, "run", v)
		}, Ignored, protocol.ErrorCodeInvalidRequest},
		{"streaming_identity", func(s *protocol.ConversationSnapshot, u *protocol.Update) {
			d := protocol.ConversationDraft{ID: "stream", Turn: s.Page.Runs[0].Turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "First"}}}
			s.Drafts = []protocol.ConversationDraft{d}
			d.Author = "user"
			*u = conversationFrame(*s, "delta", d)
		}, Ignored, protocol.ErrorCodeInvalidRequest},
		{"queue_revision", func(s *protocol.ConversationSnapshot, u *protocol.Update) {
			s.Page.Queue = protocol.ConversationQueue{Revision: 2}
			*u = conversationFrame(*s, "queue", protocol.ConversationQueue{Revision: 1})
		}, Ignored, protocol.ErrorCodeInvalidRequest},
		{"approval_terminal", func(s *protocol.ConversationSnapshot, u *protocol.Update) {
			a := protocol.ConversationApproval{ID: protocol.ApprovalID{1}, Session: s.Page.Session, Turn: protocol.TurnID{1}, Entry: "call", State: "denied", Source: "user"}
			s.Page.Approvals = []protocol.ConversationApproval{a}
			a.State = "pending"
			*u = conversationFrame(*s, "approval", a)
		}, Ignored, protocol.ErrorCodeInvalidRequest},
		{"question_terminal", func(s *protocol.ConversationSnapshot, u *protocol.Update) {
			q := protocol.ConversationQuestion{ID: protocol.QuestionID{1}, Session: s.Page.Session, Turn: protocol.TurnID{1}, Entry: "call", State: protocol.ConversationQuestionState{Kind: "cancelled"}}
			s.Page.Questions = []protocol.ConversationQuestion{q}
			q.State.Kind = "pending"
			*u = conversationFrame(*s, "question", q)
		}, Ignored, protocol.ErrorCodeInvalidRequest},
	} {
		t.Run(row.name, func(t *testing.T) {
			s := conversationFixture()
			entry := protocol.ConversationEntry{ID: "answer", Turn: protocol.TurnID{1}, Sequence: 2, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Answer"}}}
			update := conversationFrame(s, "entry", entry)
			row.change(&s, &update)
			p := NewConversationProjection(s.Node, s.Page.Session, 1)
			if _, err := p.Apply(1, protocol.Update{Kind: "conversation_snapshot", Data: s}); err != nil {
				t.Fatal(err)
			}
			before := p.Snapshot()
			generation := uint64(1)
			if row.name == "stale_generation" {
				generation = 0
			}
			got, err := p.Apply(generation, update)
			if got != row.want || (err == nil) != (row.code == "") || err != nil && faultCode(err) != row.code {
				t.Fatalf("apply=%v,%v;want %v,%v", got, err, row.want, row.code)
			}
			if !reflect.DeepEqual(before, p.Snapshot()) {
				t.Fatal("ignored or invalid frame changed canonical data")
			}
		})
	}
	t.Run("undeclared_changes_keep_the_sequence", func(t *testing.T) {
		s := conversationFixture()
		p := NewConversationProjection(s.Node, s.Page.Session, 1)
		if _, err := p.Apply(1, protocol.Update{Kind: "conversation_snapshot", Data: s}); err != nil {
			t.Fatal(err)
		}
		// A change and a streamed part of kinds a newer Node declares, as the
		// wire reader decodes them.
		later := protocol.UnknownVariant(`{"kind":"future_change","data":{"count":1}}`)
		if got, err := p.Apply(1, conversationFrame(*p.Snapshot(), "future_change", later)); got != Applied || err != nil {
			t.Fatalf("undeclared change: %v, %v", got, err)
		}
		draft := protocol.ConversationDraft{ID: "stream", Turn: s.Page.Runs[0].Turn, Author: "assistant", Parts: []protocol.ConversationPart{
			{Kind: "future_part", Data: protocol.UnknownVariant(`{"kind":"future_part","data":1}`)},
			{Kind: "text", Data: "Shown"},
		}}
		if got, err := p.Apply(1, conversationFrame(*p.Snapshot(), "delta", draft)); got != Applied || err != nil {
			t.Fatalf("undeclared part: %v, %v", got, err)
		}
		if got := p.Snapshot(); got.Sequence != s.Sequence+2 || len(got.Drafts) != 1 {
			t.Fatalf("sequence %d, drafts %d", got.Sequence, len(got.Drafts))
		}
		// The same kinds without a peer's content are a defect of this build.
		if _, err := p.Apply(1, conversationFrame(*p.Snapshot(), "future_change", nil)); err == nil {
			t.Fatal("a change this build cannot have decoded was applied")
		}
	})
	t.Run("live_delta_terminal_and_reconnect", func(t *testing.T) {
		s := conversationFixture()
		p := NewConversationProjection(s.Node, s.Page.Session, 1)
		_, err := p.Apply(1, protocol.Update{Kind: "conversation_snapshot", Data: s})
		if err != nil {
			t.Fatal(err)
		}
		if p.artifactCursor != 1 || p.liveStart != nil || p.statisticsErr != nil {
			t.Fatal("snapshot did not reset observation metadata")
		}
		for _, text := range []string{"First", " second"} {
			snapshot := *p.Snapshot()
			u := conversationFrame(snapshot, "delta", protocol.ConversationDraft{ID: "stream", Turn: s.Page.Runs[0].Turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: text}}})
			if got, err := p.Apply(1, u); got != Applied || err != nil {
				t.Fatal(got, err)
			}
		}
		held := p.Snapshot()
		draft := held.Drafts[0]
		if draft.Parts[0].Data != "First second" {
			t.Fatal(draft)
		}
		if p.artifactCursor != 1 || p.liveStart != nil {
			t.Fatal("text deltas advanced committed observation metadata")
		}
		entry := protocol.ConversationEntry{ID: "stream", Turn: s.Page.Runs[0].Turn, Sequence: 2, Author: "assistant", Parts: draft.Parts}
		if _, err := p.Apply(1, conversationFrame(*p.Snapshot(), "entry", entry)); err != nil {
			t.Fatal(err)
		}
		if p.liveStart == nil || *p.liveStart != 2 || p.artifactCursor != 1 {
			t.Fatal("committed text lost its live boundary")
		}
		statisticsFailure := protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "statistics unavailable"}
		if _, err := p.Apply(1, conversationFrame(*p.Snapshot(), "statistics_failed", statisticsFailure)); err != nil {
			t.Fatal(err)
		}
		if p.snapshot.Statistics != nil || faultCode(p.statisticsErr) != statisticsFailure.Code {
			t.Fatal("statistics failure did not retain its fault")
		}
		// A controller reads the failure with the conversation's state.
		session := p.snapshot.Page.Session
		watching := &Client{conversations: map[protocol.SessionID]*conversationWatch{session: {projection: p}}}
		if state, ok := watching.Conversation(session); !ok || faultCode(state.StatisticsErr) != statisticsFailure.Code {
			t.Fatal("the conversation state omits the statistics failure")
		}
		if _, err := p.Apply(1, conversationFrame(*p.Snapshot(), "statistics", protocol.DefaultConversationStatistics())); err != nil {
			t.Fatal(err)
		}
		if p.snapshot.Statistics == nil || p.statisticsErr != nil {
			t.Fatal("statistics recovery retained an obsolete fault")
		}
		artifact := protocol.DefaultArtifactSummary()
		artifact.ID, artifact.Session, artifact.Turn, artifact.Revision = protocol.ArtifactID{1}, s.Page.Session, s.Page.Runs[0].Turn, 1
		if _, err := p.Apply(1, conversationFrame(*p.Snapshot(), "artifact", artifact)); err != nil {
			t.Fatal(err)
		}
		if p.artifactCursor != 2 || p.liveStart == nil || *p.liveStart != 2 {
			t.Fatal("artifact commit lost observation metadata")
		}
		run := s.Page.Runs[0]
		run.Status = "completed"
		if _, err := p.Apply(1, conversationFrame(*p.Snapshot(), "run", run)); err != nil {
			t.Fatal(err)
		}
		if len(p.Snapshot().Drafts) != 0 {
			t.Fatal("closed run retained stream")
		}
		if len(held.Drafts) != 1 || held.Drafts[0].Parts[0].Data != "First second" || len(held.Page.Entries) != len(s.Page.Entries) || held.Page.Runs[0].Status != s.Page.Runs[0].Status {
			t.Fatal("later frames changed a snapshot a reader holds")
		}
		if err := p.Reconnect(2); err != nil {
			t.Fatal(err)
		}
		s.Sequence = 1
		if got, err := p.Apply(2, protocol.Update{Kind: "conversation_snapshot", Data: s}); got != Applied || err != nil {
			t.Fatal(got, err)
		}
		if p.Snapshot().Sequence != 1 {
			t.Fatal("new generation kept the previous cursor")
		}
		if p.artifactCursor != 3 || p.liveStart != nil || p.statisticsErr != nil {
			t.Fatal("new snapshot retained old live observation metadata")
		}
	})
	t.Run("older_page_keeps_live_metadata", func(t *testing.T) {
		s := conversationFixture()
		a := protocol.ConversationApproval{ID: protocol.ApprovalID{1}, Session: s.Page.Session, Turn: protocol.TurnID{1}, Entry: "call", State: "denied", Source: "user"}
		q := protocol.ConversationQuestion{ID: protocol.QuestionID{1}, Session: s.Page.Session, Turn: protocol.TurnID{1}, Entry: "question", State: protocol.ConversationQuestionState{Kind: "cancelled"}}
		s.Page.Approvals = []protocol.ConversationApproval{a}
		s.Page.Questions = []protocol.ConversationQuestion{q}
		older := copyValue(s.Page)
		older.Runs[0].Status = "queued"
		older.Approvals[0].State = "pending"
		older.Questions[0].State.Kind = "pending"
		if err := mergeConversationPage(&s.Page, older, true, nil); err != nil {
			t.Fatal(err)
		}
		if s.Page.Runs[0].Status != "running" || s.Page.Approvals[0].State != "denied" || s.Page.Questions[0].State.Kind != "cancelled" {
			t.Fatal("stale history replaced live metadata")
		}
		older.Approvals[0].State = "approved"
		if err := mergeConversationPage(&s.Page, older, true, nil); faultCode(err) != protocol.ErrorCodeInvalidRequest {
			t.Fatal("history changed a terminal decision", err)
		}
	})
	t.Run("history_pages_match_entries_by_sequence_or_id", func(t *testing.T) {
		s := conversationFixture()
		older := copyValue(s.Page)
		older.Entries = append(older.Entries, protocol.ConversationEntry{ID: "reply", Turn: protocol.TurnID{1}, Sequence: 2, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Reply"}}})
		// A page read twice adds its entries once.
		for range 2 {
			if err := mergeConversationPage(&s.Page, older, true, nil); err != nil {
				t.Fatal(err)
			}
		}
		if len(s.Page.Entries) != 2 || s.Page.Entries[1].ID != "reply" {
			t.Fatalf("merged entries = %+v", s.Page.Entries)
		}
		for name, change := range map[string]func(*protocol.ConversationEntry){
			"sequence": func(e *protocol.ConversationEntry) { e.ID = "other" },
			"ID":       func(e *protocol.ConversationEntry) { e.Sequence = 3 },
		} {
			changed := copyValue(older)
			change(&changed.Entries[1])
			if err := mergeConversationPage(&s.Page, changed, true, nil); faultCode(err) != protocol.ErrorCodeInvalidRequest {
				t.Fatalf("an entry that reuses a held %s was merged: %v", name, err)
			}
		}
		if len(s.Page.Entries) != 2 {
			t.Fatal("a refused page changed the conversation")
		}
	})
	t.Run("reveals_wait_for_loaded_extent", func(t *testing.T) {
		s := conversationFixture()
		s.Page.Entries[0].Sequence = 100
		s.Missing = []protocol.TurnID{s.Page.Runs[0].Turn}
		p := NewConversationProjection(s.Node, s.Page.Session, 1)
		if _, err := p.Apply(1, protocol.Update{Kind: "conversation_snapshot", Data: s}); err != nil {
			t.Fatal(err)
		}
		entry := protocol.ConversationEntry{ID: "reveal", Turn: s.Page.Runs[0].Turn, Sequence: 20, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Visible again"}}}
		if _, err := p.Apply(1, conversationFrame(s, "revealed", entry)); err != nil {
			t.Fatal(err)
		}
		if len(p.Snapshot().Page.Entries) != 1 {
			t.Fatal("reveal skipped an unloaded interval")
		}
		incoming := copyValue(s.Page)
		incoming.Entries = []protocol.ConversationEntry{{ID: "older", Turn: entry.Turn, Sequence: 10, Author: "assistant"}}
		if err := mergeConversationPage(&p.snapshot.Page, incoming, true, p.hidden); err != nil {
			t.Fatal(err)
		}
		p.settle()
		if len(p.Snapshot().Page.Entries) != 3 || p.Snapshot().Page.Entries[1].ID != "reveal" {
			t.Fatal("loaded reveal did not settle")
		}
		if p.liveStart != nil {
			t.Fatal("older revealed history became live text")
		}
	})
}

func testConversationSearchAndContinuation(t *testing.T) {
	s := conversationFixture()
	s.Page.Entries[0].Sequence = 100
	s.Missing = []protocol.TurnID{s.Page.Runs[0].Turn}
	searches, reads := 0, 0
	transport := &clientTransport{}
	transport.dispatch = func(request protocol.Request) (link.Admission, error) {
		var output protocol.Output
		switch request.Command.Kind {
		case "search_conversation":
			query := request.Command.Data.(protocol.CommandSearchConversationPayload).Query
			if query.Limit != 20 {
				t.Fatal("search lost its logical limit")
			}
			searches++
			page := protocol.ConversationSearchPage{Session: s.Page.Session, Revision: 1}
			if searches == 1 {
				page.NextBefore = new(uint64(100))
			} else {
				if query.Before == nil || *query.Before != 100 {
					t.Fatal("scan cursor was not retained")
				}
				page.Matches = []protocol.ConversationSearchMatch{{Turn: s.Page.Runs[0].Turn, TurnSequence: 1, Entry: "older10", Sequence: 10, Author: "assistant", Snippet: "Target", Highlight: protocol.Range[uint64]{Start: 0, End: 6}}}
			}
			output = protocol.Output{Kind: "conversation_matches", Data: page}
		case "read_turn":
			query := request.Command.Data.(protocol.CommandReadTurnPayload)
			if query.Limit != 500 || query.ExpectedRevision != 1 || query.Turn != s.Page.Runs[0].Turn || query.Before == nil {
				t.Fatal("turn continuation lost its captured scope")
			}
			reads++
			run := s.Page.Runs[0]
			run.Status = "completed"
			page := protocol.ConversationTurnHistory{Sequence: 10, Revision: 1, Run: run}
			if *query.Before == 100 {
				page.Entries = []protocol.ConversationEntry{{ID: "older10", Turn: run.Turn, Sequence: 10, Author: "assistant"}}
				page.NextBefore = new(uint64(10))
			} else if *query.Before == 10 {
				page.Entries = []protocol.ConversationEntry{{ID: "oldest1", Turn: run.Turn, Sequence: 1, Author: "assistant"}}
			} else {
				t.Fatal("continuation did not advance")
			}
			output = protocol.Output{Kind: "turn_history", Data: page}
		default:
			t.Fatal("observation replayed a business command")
		}
		done := make(chan link.Response, 1)
		done <- link.Response{Output: output}
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: done}, nil
	}
	c := New(transport)
	projection := NewConversationProjection(s.Node, s.Page.Session, 1)
	if _, err := projection.Apply(1, protocol.Update{Kind: "conversation_snapshot", Data: s}); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	done := make(chan struct{})
	close(done)
	c.conversations = map[protocol.SessionID]*conversationWatch{s.Page.Session: {session: s.Page.Session, projection: projection, ctx: ctx, cancel: cancel, done: done, signal: make(chan struct{})}}
	defer c.Close()
	page, err := c.SearchConversation(t.Context(), s.Page.Session, protocol.ConversationSearchQuery{Text: "Target", Limit: 20})
	if err != nil || len(page.Matches) != 1 || searches != 2 {
		t.Fatal("bounded scans did not fill the logical search page", err)
	}
	if err := c.RevealConversation(t.Context(), s.Page.Session, page.Revision, page.Matches[0]); err != nil {
		t.Fatal(err)
	}
	if reads != 1 || len(projection.snapshot.Missing) != 1 {
		t.Fatal("reveal over-read its exact target")
	}
	if err := c.LoadOlderConversation(t.Context(), s.Page.Session); err != nil {
		t.Fatal(err)
	}
	if reads != 2 || len(projection.snapshot.Page.Entries) != 3 || len(projection.snapshot.Missing) != 0 || projection.snapshot.Page.Runs[0].Status != "running" {
		t.Fatal("large turn continuation lost history or live metadata")
	}
}
