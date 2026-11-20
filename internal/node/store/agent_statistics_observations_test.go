package store

import (
	"cmp"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const legacyAgentStatisticCallsQuery = `WITH rounds AS (
 SELECT t.id,row_number() OVER (ORDER BY t.rowid) AS turn_index
 FROM conversation_turns membership JOIN turns t ON t.id=membership.turn
 WHERE membership.session=?1 AND t.kind='task' AND EXISTS(SELECT 1 FROM agent_events event WHERE event.turn=t.id))
 SELECT h.sequence,json_extract(e.body,'$.metadata.execution.observation'),
 (SELECT json_extract(model.value,'$.context') FROM agent_runs r
  JOIN turns t ON t.id=r.turn JOIN session_revisions revision ON revision.session=t.session AND revision.revision=t.revision,
  json_each(r.provider,'$.models') model WHERE r.turn=e.turn AND json_extract(model.value,'$.id')=json_extract(revision.config,'$.model') LIMIT 1),
 (SELECT turn_index FROM rounds WHERE rounds.id=e.turn)
 FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
 WHERE h.session=?1 AND json_type(e.body,'$.metadata.execution.observation')='object' ORDER BY h.sequence`

func legacyAgentStatisticCalls(ctx context.Context, db *sql.Conn, session protocol.SessionID) ([]agentStatisticCall, error) {
	rows, err := db.QueryContext(ctx, legacyAgentStatisticCallsQuery, session.String())
	if err != nil {
		return nil, err
	}
	result := []agentStatisticCall{}
	indices := map[string]int{}
	for rows.Next() {
		var sequence uint64
		var body []byte
		var limit, turnIndex sql.NullInt64
		if err = rows.Scan(&sequence, &body, &limit, &turnIndex); err != nil {
			rows.Close()
			return nil, err
		}
		var request protocol.ConversationObservationRequest
		if err = json.Unmarshal(body, &request); err != nil {
			rows.Close()
			return nil, err
		}
		request.ContextLimit, request.TurnIndex = nil, nil
		if limit.Valid {
			value := uint32(limit.Int64)
			request.ContextLimit = &value
		}
		if turnIndex.Valid {
			value := uint64(turnIndex.Int64)
			request.TurnIndex = &value
		}
		index, exists := indices[request.ID]
		if !exists {
			index = len(result)
			indices[request.ID] = index
			capture := request
			capture.Attempts = nil
			result = append(result, agentStatisticCall{sequence, capture})
		}
		current := &result[index].request
		for _, attempt := range request.Attempts {
			if i := slices.IndexFunc(current.Attempts, func(previous protocol.ConversationObservationAttempt) bool { return previous.Index == attempt.Index }); i >= 0 {
				if current.Attempts[i].ElapsedUs == nil || attempt.ElapsedUs != nil {
					current.Attempts[i] = attempt
				}
			} else {
				current.Attempts = append(current.Attempts, attempt)
			}
		}
		slices.SortFunc(current.Attempts, func(a, b protocol.ConversationObservationAttempt) int { return cmp.Compare(a.Index, b.Index) })
	}
	return result, errors.Join(rows.Err(), rows.Close())
}

func compareAgentStatisticCalls(ctx context.Context, handle *Handle, session protocol.SessionID) ([]agentStatisticCall, error) {
	var result []agentStatisticCall
	err := handle.Read(ctx, func(db *sql.Conn) error {
		legacy, err := legacyAgentStatisticCalls(ctx, db, session)
		if err != nil {
			return fmt.Errorf("legacy calls: %w", err)
		}
		result, err = agentStatisticCalls(ctx, db, session)
		if err != nil {
			return fmt.Errorf("split calls: %w", err)
		}
		if len(result) != len(legacy) {
			return fmt.Errorf("got %d calls, legacy got %d", len(result), len(legacy))
		}
		for i, call := range result {
			if !reflect.DeepEqual(call, legacy[i]) {
				return fmt.Errorf("call %d: split = %+v, legacy = %+v", i, call, legacy[i])
			}
		}
		return nil
	})
	return result, err
}

func insertStatisticFixtureTurn(ctx context.Context, db *sql.Conn, template, turn protocol.TurnID, member protocol.SessionID, kind string, provider any) error {
	position, err := NextCounter(ctx, db, CounterTurns)
	if err != nil {
		return err
	}
	if _, err = db.ExecContext(ctx, `INSERT INTO turns(rowid,id,kind,plugins,roles,session,revision,request,caller)
 SELECT ?1,?2,?3,plugins,roles,session,revision,request,caller FROM turns WHERE id=?4`, position, turn.String(), kind, template.String()); err != nil {
		return err
	}
	if _, err = db.ExecContext(ctx, `INSERT INTO agent_runs(turn,ready,body,provider,hooks) VALUES(?1,0,?2,?3,X'5B5D')`, turn.String(), []byte(`{"status":"completed"}`), provider); err != nil {
		return err
	}
	if member != (protocol.SessionID{}) {
		_, err = db.ExecContext(ctx, `INSERT INTO conversation_turns(session,turn) VALUES(?1,?2)`, member.String(), turn.String())
	}
	return err
}

func insertStatisticFixtureEvent(ctx context.Context, db *sql.Conn, member protocol.SessionID, turn protocol.TurnID, body []byte) (uint64, error) {
	sequence, err := NextCounter(ctx, db, CounterAgentEvents)
	if err != nil {
		return 0, err
	}
	if _, err = db.ExecContext(ctx, `INSERT INTO agent_events(sequence,session,id,turn,body)
 SELECT ?1,session,?2,id,?3 FROM turns WHERE id=?4`, sequence, fmt.Sprintf("statistics-%d", sequence), body, turn.String()); err != nil {
		return 0, err
	}
	if member != (protocol.SessionID{}) {
		_, err = db.ExecContext(ctx, `INSERT INTO conversation_events(session,sequence) VALUES(?1,?2)`, member.String(), sequence)
	}
	return sequence, err
}

func insertStatisticFixtureObservation(ctx context.Context, db *sql.Conn, member protocol.SessionID, turn protocol.TurnID, request protocol.ConversationObservationRequest) (uint64, error) {
	body, err := json.Marshal(map[string]any{"metadata": map[string]any{"execution": map[string]any{"observation": request}}})
	if err != nil {
		return 0, err
	}
	return insertStatisticFixtureEvent(ctx, db, member, turn, body)
}

func TestAgentStatisticCallsMatchLegacy(t *testing.T) {
	t.Parallel()
	storage, _, source, template, _ := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	turn := func(i byte) protocol.TurnID { return protocol.TurnID{0: i, 6: 0x40, 8: 0x80} }
	prior, empty, compaction, captured := turn(8), turn(7), turn(6), turn(5)
	unmatched, nullMatch, nullProvider, outside := turn(4), turn(3), turn(2), turn(1)
	request := func(id string, turn protocol.TurnID, attempts ...protocol.ConversationObservationAttempt) protocol.ConversationObservationRequest {
		return protocol.ConversationObservationRequest{ID: id, Turn: turn, ContextLimit: ptr(uint32(1)), TurnIndex: ptr(uint64(99)), Model: "helper", Attempts: attempts}
	}
	initial := protocol.DefaultConversationObservationAttempt()
	initial.Index, initial.Transport, initial.Outcome = 1, "http", protocol.ConversationObservationOutcomeCancelled
	complete := initial
	complete.ElapsedUs, complete.Outcome = ptr(uint64(10)), protocol.ConversationObservationOutcomeSucceeded
	second := complete
	second.Index, second.ElapsedUs = 2, ptr(uint64(20))
	snapshots := []struct {
		turn    protocol.TurnID
		request protocol.ConversationObservationRequest
	}{
		{unmatched, request("unmatched", unmatched)},
		{captured, request("repeated", outside, second, initial)},
		{outside, request("repeated", unmatched, complete)},
		{captured, request("repeated", captured, initial)},
		{compaction, request("compaction", compaction)},
		{nullMatch, request("null-match", nullMatch)},
		{nullProvider, request("null-provider", nullProvider)},
		{outside, request("event-only-member", outside)},
	}
	var fork protocol.Session
	sequences := make([]uint64, len(snapshots))
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		fork, err = insertBranchSession(ctx, db, source, source.Worktree)
		if err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, `UPDATE session_revisions SET config=json_set(json(config),'$.model','fork') WHERE session=?1`, fork.ID.String()); err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, `INSERT INTO session_revisions(session,revision,worktree,config,profile,roles)
 SELECT session,revision+1,worktree,json_set(json(config),'$.model','current'),profile,roles FROM session_revisions WHERE session=?1 AND revision=?2`, source.ID.String(), source.Revision); err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, `UPDATE sessions SET revision=?1 WHERE id=?2`, source.Revision+1, source.ID.String()); err != nil {
			return err
		}
		for _, row := range []struct {
			turn     protocol.TurnID
			kind     string
			provider any
		}{
			{prior, "task", []byte(`{"models":[{"id":"fixture","context":"not an integer"}]}`)},
			{empty, "task", nil},
			{compaction, "compaction", []byte(`{"models":[{"id":"fixture","context":20000}]}`)},
			{captured, "task", []byte(`{"models":[{"id":"fork","context":8000},{"id":"current","context":16000},{"id":"fixture","context":32000},{"id":"fixture","context":64000}]}`)},
			{unmatched, "task", []byte(`{"models":[{"id":"helper","context":4000}]}`)},
			{nullMatch, "task", []byte(`{"models":[{"id":"fixture","context":null},{"id":"fixture","context":48000}]}`)},
			{nullProvider, "task", nil},
			{outside, "task", []byte(`{"models":[{"id":"fixture","context":50000}]}`)},
		} {
			member := fork.ID
			if row.turn == outside {
				member = protocol.SessionID{}
			}
			if err = insertStatisticFixtureTurn(ctx, db, template, row.turn, member, row.kind, row.provider); err != nil {
				return err
			}
		}
		if _, err = insertStatisticFixtureObservation(ctx, db, protocol.SessionID{}, prior, request("not visible", prior)); err != nil {
			return err
		}
		if _, err = insertStatisticFixtureEvent(ctx, db, fork.ID, captured, []byte(`{"metadata":{"execution":{"observation":[]}}}`)); err != nil {
			return err
		}
		for i, snapshot := range snapshots {
			if sequences[i], err = insertStatisticFixtureObservation(ctx, db, fork.ID, snapshot.turn, snapshot.request); err != nil {
				return err
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	wantCall := func(i int, limit *uint32, round *uint64) agentStatisticCall {
		request := snapshots[i].request
		request.ContextLimit, request.TurnIndex = limit, round
		return agentStatisticCall{sequences[i], request}
	}
	want := []agentStatisticCall{
		wantCall(0, nil, ptr(uint64(3))),
		wantCall(1, ptr(uint32(32000)), ptr(uint64(2))),
		wantCall(4, ptr(uint32(20000)), nil),
		wantCall(5, nil, ptr(uint64(4))),
		wantCall(6, nil, ptr(uint64(5))),
		wantCall(7, ptr(uint32(50000)), nil),
	}
	want[1].request.Attempts = []protocol.ConversationObservationAttempt{complete, second}
	got, err := compareAgentStatisticCalls(ctx, storage.Node, fork.ID)
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("calls = %+v, want %+v", got, want)
	}
}

func BenchmarkAgentStatisticCalls(b *testing.B) {
	const rounds, callsPerTurn, snapshots = 80, 8, 4
	storage, _, session, template, _ := agentHistoryFixture(b, protocol.WorkModeCode)
	ctx := b.Context()
	models := make([]protocol.ConversationModel, 97)
	for i := range models {
		models[i] = protocol.DefaultConversationModel()
		models[i].ID, models[i].Context = fmt.Sprintf("model-%d", i), 32000
	}
	models[len(models)-1].ID = session.Config.Model
	provider, err := json.Marshal(map[string]any{"models": models})
	if err != nil {
		b.Fatal(err)
	}
	toolResult := []byte(`{"kind":"message","content":[{"type":"tool_response","result":"` + strings.Repeat("tool output ", 2048) + `"}]}`)
	err = storage.Node.Write(ctx, func(db *sql.Conn) error {
		for round := range rounds {
			turn := protocol.NewTurnID()
			if err := insertStatisticFixtureTurn(ctx, db, template, turn, session.ID, "task", provider); err != nil {
				return err
			}
			if _, err := insertStatisticFixtureEvent(ctx, db, session.ID, turn, toolResult); err != nil {
				return err
			}
			for call := range callsPerTurn {
				request := protocol.ConversationObservationRequest{ID: fmt.Sprintf("%d/%d", round, call), Turn: turn, Model: session.Config.Model, Attempts: []protocol.ConversationObservationAttempt{protocol.DefaultConversationObservationAttempt()}}
				attempt := &request.Attempts[0]
				attempt.Transport, attempt.Outcome = "http", protocol.ConversationObservationOutcomeSucceeded
				for snapshot := range snapshots {
					attempt.Index, attempt.ElapsedUs = uint64(1+snapshot/2), nil
					if snapshot%2 == 1 {
						attempt.ElapsedUs = ptr(uint64(100 + snapshot))
					}
					if _, err := insertStatisticFixtureObservation(ctx, db, session.ID, turn, request); err != nil {
						return err
					}
				}
			}
		}
		return nil
	})
	if err != nil {
		b.Fatal(err)
	}
	got, err := compareAgentStatisticCalls(ctx, storage.Node, session.ID)
	if err != nil {
		b.Fatal(err)
	}
	if len(got) != rounds*callsPerTurn || !reflect.DeepEqual(got[0].request.ContextLimit, ptr(uint32(32000))) {
		b.Fatalf("unexpected benchmark fixture: %d calls or missing context limit", len(got))
	}
	for _, implementation := range []struct {
		name string
		read func(context.Context, *sql.Conn, protocol.SessionID) ([]agentStatisticCall, error)
	}{
		{"legacy", legacyAgentStatisticCalls},
		{"split", agentStatisticCalls},
	} {
		b.Run(implementation.name, func(b *testing.B) {
			ctx := b.Context()
			b.ReportAllocs()
			for b.Loop() {
				if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
					_, err := implementation.read(ctx, db, session.ID)
					return err
				}); err != nil {
					b.Fatal(err)
				}
			}
		})
	}
}
