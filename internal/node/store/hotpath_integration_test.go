//go:build integration

package store

import (
	"context"
	"database/sql"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const (
	hotPathTurns       = 300
	hotPathCalls       = 2   // tool calls in each closed turn
	hotPathOpenCalls   = 100 // tool calls in the running turn
	hotPathLargeEvery  = 12  // closed turns between large tool outputs
	hotPathLargeOpen   = 25  // large tool outputs in the running turn
	hotPathLargeBytes  = 32 * 1024
	hotPathImageBytes  = 512 * 1024
	hotPathCheckpoints = 100
	hotPathSessions    = 100
)

var errHotPathRollback = errors.New("measurement rolled back")

type hotPathFixture struct {
	storage *Store
	node    protocol.NodeID
	session protocol.Session
	now     time.Time
	// first holds the file checkpoints, middle is a closed turn used as a page
	// cursor and open is the running turn.
	first, middle, open protocol.TurnID
	records, large      int
	lastCall            canonical.ToolIdentity
	lastOutput          protocol.ToolOutputOutputReference
}

// TestStoreHotPaths is the manual measurement of the reads and appends a long
// conversation repeats: one session of 300 turns and about 2,000 records, 50 of
// them large tool outputs, plus 100 file checkpoints and 100 other sessions.
func TestStoreHotPaths(t *testing.T) {
	ctx := context.Background()
	fixture := hotPathSeed(t)
	storage, session := fixture.storage, fixture.session
	report := map[string]float64{}
	read := func(name string, rounds int, operation func(*sql.Conn) error) {
		t.Helper()
		samples := make([]time.Duration, 0, rounds)
		for range rounds {
			if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
				began := time.Now()
				err := operation(db)
				samples = append(samples, time.Since(began))
				return err
			}); err != nil {
				t.Fatalf("%s: %v", name, err)
			}
		}
		report[name] = hotPathMedian(samples)
	}
	// A write is measured inside its transaction and then rolled back, so every
	// round sees the same history.
	write := func(name string, rounds int, operation func(*sql.Conn) error) {
		t.Helper()
		samples := make([]time.Duration, 0, rounds)
		for range rounds {
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				began := time.Now()
				err := operation(db)
				samples = append(samples, time.Since(began))
				if err != nil {
					return err
				}
				return errHotPathRollback
			})
			if !errors.Is(err, errHotPathRollback) {
				t.Fatalf("%s: %v", name, err)
			}
		}
		report[name] = hotPathMedian(samples)
	}

	read("M2 load_agent_state", 15, func(db *sql.Conn) error {
		state, err := LoadAgentState(ctx, db, session.ID)
		if err != nil {
			return err
		}
		if len(state.Records) != fixture.records {
			return fmt.Errorf("loaded %d records, want %d", len(state.Records), fixture.records)
		}
		for i := range state.Records {
			if message := state.Records[i].MutableMessage(); message != nil {
				for _, block := range message.Content {
					if result, ok := block.Data.(*canonical.ToolResponse); ok {
						if _, err := HydrateCanonicalPayload(ctx, db, result.Result); err != nil {
							return err
						}
					}
				}
			}
		}
		return nil
	})
	// A step reloads from the state of the step before it.
	var base AgentState
	read("M2 load_agent_state_first", 1, func(db *sql.Conn) error {
		var err error
		base, err = LoadAgentState(ctx, db, session.ID)
		return err
	})
	read("M2 extend_agent_state", 15, func(db *sql.Conn) error {
		state, reused, err := ExtendAgentState(ctx, db, session.ID, base)
		if err == nil && (reused != fixture.records || len(state.Records) != fixture.records) {
			err = fmt.Errorf("extended %d records, reused %d", len(state.Records), reused)
		}
		return err
	})
	write("M2 extend_agent_state_after_append", 15, func(db *sql.Conn) error {
		record, _ := hotPathResponse(fixture.open, *fixture.lastCall.ID+"-step", `{"text":"step"}`, fixture.now)
		if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
			return err
		}
		extended, reused, err := ExtendAgentState(ctx, db, session.ID, base)
		if err != nil {
			return err
		}
		fresh, err := LoadAgentState(ctx, db, session.ID)
		if err != nil {
			return err
		}
		if reused != fixture.records || len(extended.Records) != fixture.records+1 {
			return fmt.Errorf("extended %d records, reused %d", len(extended.Records), reused)
		}
		for _, pair := range [][2]any{{extended.Records, fresh.Records}, {extended.Context, fresh.Context}, {extended.LoadedTools, fresh.LoadedTools}, {extended.Session, fresh.Session}} {
			left, err := encode(pair[0])
			if err != nil {
				return err
			}
			right, err := encode(pair[1])
			if err != nil {
				return err
			}
			if string(left) != string(right) {
				return errors.New("extended state differs from a complete load")
			}
		}
		return nil
	})
	read("M2 load_agent_context", 15, func(db *sql.Conn) error {
		_, loaded, err := LoadAgentContext(ctx, db, session.ID)
		if err == nil && len(loaded) == 0 {
			err = errors.New("loaded tools are missing")
		}
		return err
	})
	read("M3 read_conversation_latest", 15, func(db *sql.Conn) error {
		history, err := ReadConversation(ctx, db, session.ID, nil, 50)
		if err == nil && (len(history.Page.Runs) == 0 || len(history.Page.Entries) == 0) {
			err = errors.New("latest page is empty")
		}
		return err
	})
	read("M3 read_conversation_older", 15, func(db *sql.Conn) error {
		history, err := ReadConversation(ctx, db, session.ID, &fixture.middle, 50)
		if err == nil && (len(history.Page.Runs) == 0 || len(history.Page.Entries) == 0) {
			err = errors.New("older page is empty")
		}
		return err
	})
	write("M4 begin_approval", 15, func(db *sql.Conn) error {
		_, _, err := BeginApproval(ctx, db, fixture.open, fixture.lastCall, nil, fixture.now)
		return err
	})
	write("M4 append_tool_response", 15, func(db *sql.Conn) error {
		record, _ := hotPathResponse(fixture.open, *fixture.lastCall.ID+"-again", `{"text":"again"}`, fixture.now)
		_, _, err := AppendCanonical(ctx, db, session.ID, record)
		return err
	})
	write("M9 append_image_response", 15, func(db *sql.Conn) error {
		record, err := hotPathImage(ctx, db, fixture.open, "image-again", fixture.now)
		if err != nil {
			return err
		}
		_, _, err = AppendCanonical(ctx, db, session.ID, record)
		return err
	})
	read("M5 read_tool_output", 15, func(db *sql.Conn) error {
		output, err := ReadAgentCommand(ctx, db, fixture.node, protocol.Command{Kind: "read_tool_output", Data: &protocol.CommandReadToolOutputPayload{Turn: fixture.open, Output: fixture.lastOutput, Limit: 32 * 1024}})
		if err == nil && output.Data.(protocol.ToolOutputOutputPage).TotalBytes < hotPathLargeBytes {
			err = errors.New("tool output page is incomplete")
		}
		return err
	})
	read("M10 read_usage", 15, func(db *sql.Conn) error {
		query := protocol.DefaultUsageQuery()
		query.StartMs, query.EndMs = fixture.now.Add(-time.Hour).UnixMilli(), fixture.now.Add(time.Hour).UnixMilli()
		query.Dimension = protocol.UsageDimensionModel
		output, err := ReadAgentCommand(ctx, db, fixture.node, protocol.Command{Kind: "read_usage", Data: &query})
		if err == nil && output.Data.(protocol.UsageReport).Totals.Responses != hotPathTurns-1 {
			err = fmt.Errorf("usage responses: %d", output.Data.(protocol.UsageReport).Totals.Responses)
		}
		return err
	})
	read("M8 list_file_checkpoints", 15, func(db *sql.Conn) error {
		page, err := ListFileCheckpoints(ctx, db, session.ID, fixture.first, nil, 100)
		if err == nil && len(page.Files) != hotPathCheckpoints {
			err = fmt.Errorf("checkpoint files: %d", len(page.Files))
		}
		return err
	})
	read("M8 read_turn_diff", 15, func(db *sql.Conn) error {
		diff, err := ReadTurnDiff(ctx, db, session.ID, fixture.first)
		if err == nil && len(diff.Files) != hotPathCheckpoints {
			err = fmt.Errorf("diff files: %d", len(diff.Files))
		}
		return err
	})
	read("M8 read_turn_hunks", 15, func(db *sql.Conn) error {
		hunks, err := ReadTurnHunks(ctx, db, session.ID, fixture.first, "file-050.txt")
		if err == nil && len(hunks.Hunks) != 1 {
			err = fmt.Errorf("hunks: %d", len(hunks.Hunks))
		}
		return err
	})
	read("M6 read_sessions", 15, func(db *sql.Conn) error {
		sessions, err := ReadSessions(ctx, db)
		if err == nil && len(sessions) != hotPathSessions+1 {
			err = fmt.Errorf("sessions: %d", len(sessions))
		}
		return err
	})
	read("M6 snapshot", 15, func(db *sql.Conn) error {
		_, err := Snapshot(ctx, db, fixture.node)
		return err
	})
	read("M6 read_queue", 15, func(db *sql.Conn) error {
		_, err := ReadQueue(ctx, db, session.ID)
		return err
	})

	// L3: more readers than database/sql keeps idle by default.
	samples := make([]time.Duration, 0, 15)
	for range 15 {
		began := time.Now()
		var readers sync.WaitGroup
		failures := make(chan error, 8)
		for range 8 {
			readers.Add(1)
			go func() {
				defer readers.Done()
				for range 25 {
					if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
						_, err := historyRevision(ctx, db, session.ID)
						return err
					}); err != nil {
						failures <- err
						return
					}
				}
			}()
		}
		readers.Wait()
		close(failures)
		for err := range failures {
			t.Fatal(err)
		}
		samples = append(samples, time.Since(began))
	}
	report["L3 200_reads_on_8_readers"] = hotPathMedian(samples)

	// Startup with nothing to update: the reference schema and the audit.
	samples = samples[:0]
	if err := storage.Close(); err != nil {
		t.Fatal(err)
	}
	for range 7 {
		began := time.Now()
		reopened, err := Open(ctx, storage.Profile, fixture.node)
		if err != nil {
			t.Fatal(err)
		}
		samples = append(samples, time.Since(began))
		if err := reopened.Close(); err != nil {
			t.Fatal(err)
		}
	}
	report["L2 open_current_profile"] = hotPathMedian(samples)

	names := make([]string, 0, len(report))
	for name := range report {
		names = append(names, name)
	}
	slices.Sort(names)
	t.Logf("records=%d turns=%d large_outputs=%d checkpoints=%d sessions=%d", fixture.records, hotPathTurns, fixture.large, hotPathCheckpoints, hotPathSessions+1)
	for _, name := range names {
		t.Logf("%-32s %10.3f ms", name, report[name])
	}
}

func hotPathMedian(samples []time.Duration) float64 {
	slices.Sort(samples)
	return float64(samples[len(samples)/2]) / float64(time.Millisecond)
}

func hotPathResponse(turn protocol.TurnID, call, body string, now time.Time) (canonical.Record, protocol.ToolOutputOutputReference) {
	reference := protocol.ToolOutputOutputReference{Call: call, Revision: fmt.Sprintf("%x", blake3.Sum256([]byte(body)))}
	response := canonical.ToolResponse{ID: call, Name: "read_file", Format: canonical.JSON, Status: canonical.ToolSuccess, Revision: reference.Revision, Result: canonical.Payload{Value: json.RawMessage(body)}}
	record := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.User, canonical.Block{Index: 0, Kind: "tool_response", Data: &response})), now)
	record.Provenance.Author = "tool"
	return record, reference
}

func hotPathImage(ctx context.Context, db *sql.Conn, turn protocol.TurnID, call string, now time.Time) (canonical.Record, error) {
	image := make([]byte, hotPathImageBytes)
	for index := range image {
		image[index] = byte(index * 31)
	}
	body, err := encode(map[string]any{"content": []any{map[string]any{"type": "image", "mimeType": "image/png", "data": base64.StdEncoding.EncodeToString(image)}}})
	if err != nil {
		return canonical.Record{}, err
	}
	canonicalBody, err := encode(agentJSON(body))
	if err != nil {
		return canonical.Record{}, err
	}
	payload, err := CaptureCanonicalPayload(ctx, db, body)
	if err != nil {
		return canonical.Record{}, err
	}
	response := canonical.ToolResponse{ID: call, Name: "read_file", Format: canonical.JSON, Status: canonical.ToolSuccess, Revision: fmt.Sprintf("%x", blake3.Sum256(canonicalBody)), Result: payload}
	record := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.User, canonical.Block{Index: 0, Kind: "tool_response", Data: &response})), now)
	record.Provenance.Author = "tool"
	return record, nil
}

func hotPathSeed(t *testing.T) hotPathFixture {
	t.Helper()
	ctx := context.Background()
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	fixture := hotPathFixture{storage: storage, node: node, session: session, now: now, first: turn}
	hotPathCheckpointFiles(t, &fixture)
	large := `{"text":"` + strings.Repeat("retained tool output ", hotPathLargeBytes/21) + `"}`
	for index := range hotPathTurns {
		open := index == hotPathTurns-1
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			add := func(record canonical.Record) error {
				_, _, err := AppendCanonical(ctx, db, session.ID, record)
				if err == nil {
					fixture.records++
				}
				return err
			}
			input := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.User, canonical.Text(0, fmt.Sprintf("Request %d", index)))), now)
			input.Provenance.Author = "user"
			if err := add(input); err != nil {
				return err
			}
			calls := hotPathCalls
			if open {
				calls = hotPathOpenCalls
			}
			for call := range calls {
				id := fmt.Sprintf("call-%d-%d", index, call)
				arguments := json.RawMessage(fmt.Sprintf(`{"path":"file-%d.txt"}`, call))
				request := canonical.ToolRequest{ID: id, Name: "read_file", Arguments: arguments}
				record := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_request", Data: &request})), now)
				record.Provenance.Author = "assistant"
				if err := add(record); err != nil {
					return err
				}
				body := fmt.Sprintf(`{"text":"output %d"}`, call)
				if !open && call == 0 && index%hotPathLargeEvery == 0 || open && call >= calls-hotPathLargeOpen {
					body = large
					fixture.large++
				}
				response, reference := hotPathResponse(turn, id, body, now)
				if err := add(response); err != nil {
					return err
				}
				fixture.lastCall = canonical.ToolIdentity{ID: &id, Name: request.Name, Arguments: arguments}
				fixture.lastOutput = reference
			}
			if open {
				image, err := hotPathImage(ctx, db, turn, "image", now)
				if err != nil {
					return err
				}
				fixture.open = turn
				return add(image)
			}
			message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, fmt.Sprintf("Answer %d", index)))
			message.Usage = &canonical.Usage{Input: ptr(uint64(100)), Output: ptr(uint64(20)), CacheRead: ptr(uint64(2)), Reasoning: ptr(uint64(1))}
			final := canonical.NewRecord(turn, "message", &message, now)
			final.Provenance.Author = "assistant"
			final.Metadata.Execution = &canonical.Execution{Timing: &canonical.Timing{ElapsedUs: ptr(uint64(10)), FirstTokenUs: ptr(uint64(4))}}
			if err := add(final); err != nil {
				return err
			}
			if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
				return err
			}
			if index == hotPathTurns/2 {
				fixture.middle = turn
			}
			var err error
			turn, err = delegationFixtureParent(ctx, db, node, session, now)
			return err
		}); err != nil {
			t.Fatalf("turn %d: %v", index, err)
		}
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := db.ExecContext(ctx, "INSERT INTO model_catalog(provider,id,body) VALUES('openai','fixture',?1)", []byte(`{"cost":{"input":2,"output":8,"cache_read":0.5}}`)); err != nil {
			return err
		}
		for range hotPathSessions {
			if _, _, err := createSession(ctx, db, node, storage.Profile, protocol.CommandCreateSessionPayload{Config: &session.Config}); err != nil {
				return err
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	return fixture
}

// hotPathCheckpointFiles records one confirmed write per file in the first turn.
func hotPathCheckpointFiles(t *testing.T, fixture *hotPathFixture) {
	t.Helper()
	ctx := context.Background()
	storage, node, session, turn := fixture.storage, fixture.node, fixture.session, fixture.first
	var root string
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		root, err = ReadWorkspaceRoot(ctx, db, session.Worktree, false)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(root, 0o700); err != nil {
		t.Fatal(err)
	}
	resolved, err := filepath.EvalSymlinks(root)
	if err != nil {
		t.Fatal(err)
	}
	root = resolved
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(ctx, "UPDATE worktrees SET path=?2 WHERE id=?1", session.Worktree.String(), root)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	before := strings.Repeat("unchanged line\n", 200) + "before\n"
	revision := fmt.Sprintf("%x", blake3.Sum256([]byte(before)))
	for index := range hotPathCheckpoints {
		name := fmt.Sprintf("file-%03d.txt", index)
		if err := os.WriteFile(filepath.Join(root, name), []byte(before), 0o600); err != nil {
			t.Fatal(err)
		}
		request := protocol.NewRequest(node, protocol.Command{Kind: "write_file", Data: &protocol.CommandWriteFilePayload{Worktree: session.Worktree, Path: name, Text: strings.Repeat("unchanged line\n", 200) + "after\n", ExpectedRevision: &revision}})
		prepared, err := files.Prepare(ctx, root, request.Command, core.RestrictedFiles)
		if err != nil {
			t.Fatal(err)
		}
		approval := protocol.DefaultConversationApproval()
		approval.ID, approval.Session, approval.Turn = protocol.NewApprovalID(), session.ID, turn
		approval.State, approval.Source = protocol.ConversationApprovalStateApproved, protocol.ConversationApprovalSourceUser
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			body, err := encode(request)
			if err != nil {
				return err
			}
			if _, err = admission.Admit(ctx, db, node, request, body); err != nil {
				return err
			}
			if body, err = encode(approval); err != nil {
				return err
			}
			if _, err = db.ExecContext(ctx, "INSERT INTO agent_approvals(id,turn,call,body,binding,request,claimed) VALUES(?1,?2,?3,?4,?5,?6,1)", approval.ID.String(), turn.String(), "files-"+name, body, []byte("null"), request.ID.String()); err != nil {
				return err
			}
			return CaptureFileCheckpointsRequest(ctx, db, turn, approval.ID, request, prepared)
		}); err != nil {
			t.Fatal(err)
		}
		output, err := files.Apply(ctx, root, prepared)
		if err != nil {
			t.Fatal(err)
		}
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			return admission.Complete(ctx, db, node, request.ID, link.Response{Output: output})
		}); err != nil {
			t.Fatal(err)
		}
	}
}
