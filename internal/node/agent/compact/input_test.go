package compact

import (
	"encoding/json"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/tokens"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// longTurn is one user message followed by calls, each answered by a result
// too long to keep whole at the first detail level.
func longTurn(calls int) []canonical.Record {
	turn := protocol.NewTurnID()
	created := time.Date(2026, 11, 12, 0, 0, 0, 0, time.UTC)
	record := func(role canonical.Role, blocks ...canonical.Block) canonical.Record {
		return canonical.NewRecord(turn, "message", canonical.NewMessage(role, blocks...), created)
	}
	records := []canonical.Record{record(canonical.User, canonical.Text(0, "Kick off the migration exactly as specified"))}
	for i := range calls {
		id := fmt.Sprintf("call-%d", i)
		result, _ := json.Marshal(fmt.Sprintf("start-%d %s MIDDLE-%d %s end-%d", i, strings.Repeat("lorem ipsum ", 200), i, strings.Repeat("dolor sit ", 200), i))
		records = append(records,
			record(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: &canonical.ToolRequest{ID: id, Name: "read_file", Arguments: json.RawMessage(fmt.Sprintf(`{"path":"file-%d.go"}`, i))}}),
			record(canonical.User, canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{ID: id, Status: canonical.ToolSuccess, Revision: strings.Repeat("a", 64), Result: canonical.Payload{Value: result}}}))
	}
	return records
}

func TestPrepareShortensLatestTurn(t *testing.T) {
	const output = 8000
	base, err := Prepare(nil, 64000, output)
	if err != nil {
		t.Fatal(err)
	}
	// The prompt's numeric cap changes with small windows. Recount it so each
	// fixture still leaves exactly budget tokens for the transcript.
	limit := func(budget uint64) uint64 {
		contextLimit := output + tokens.Text(base) + 256 + budget
		for range 8 {
			prompt := instructions(Cap(contextLimit, output), "")
			adjusted := output + tokens.Text(prompt) + 256 + budget
			if adjusted == contextLimit {
				return contextLimit
			}
			contextLimit = adjusted
		}
		t.Fatal("could not construct the exact transcript budget")
		return 0
	}
	turn := longTurn(12)
	whole, edged, omitted := tokens.Text(transcript(turn, -1)), tokens.Text(transcript(turn, 0)), tokens.Text(transcript(turn, 1))
	if !(whole > edged && edged > omitted) {
		t.Fatalf("fixture sizes do not descend: %d, %d, %d", whole, edged, omitted)
	}
	const kickoff = "[user]: Kick off the migration exactly as specified"
	for _, row := range []struct {
		name    string
		budget  uint64
		want    []string
		absent  []string
		failure string
	}{
		{"whole", whole, []string{kickoff, "MIDDLE-0", "MIDDLE-11"}, []string{"characters omitted", "[omitted]", "earlier records"}, ""},
		{"tool responses to their edges", whole - 1, []string{kickoff, "start-0", "end-0", "end-11", "characters omitted", `"call":"call-0"`}, []string{"MIDDLE-0", "MIDDLE-11", "tool_response: [omitted]", "earlier records"}, ""},
		{"tool responses omitted", edged - 1, []string{kickoff, "tool_response: [omitted]", "file-0.go", "file-11.go", `"call":"call-0"`, `"call":"call-11"`}, []string{"start-0", "end-11", "earlier records"}, ""},
		{"oldest records dropped", omitted - 1, []string{kickoff, "earlier records of this turn omitted]", "file-11.go", `"call":"call-11"`}, []string{"file-0.go", `"call":"call-0"`, "start-11"}, ""},
		{"user message alone", tokens.Text(kickoff) + 24, []string{kickoff, "earlier records of this turn omitted]"}, []string{"file-0.go"}, ""},
		{"user message does not fit", tokens.Text(kickoff) - 1, nil, nil, "latest user message exceeds the summary context limit"},
	} {
		t.Run(row.name, func(t *testing.T) {
			prepared, err := Prepare(turn, limit(row.budget), output)
			if row.failure != "" {
				if err == nil || err.Error() != row.failure {
					t.Fatalf("error = %v, want %q", err, row.failure)
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			body := strings.TrimPrefix(prepared[strings.Index(prepared, "[user]: "):], "[user]: ")
			body = body[:strings.Index(body, "\n\n## What to keep")]
			if size := tokens.Text(body); size > row.budget {
				t.Fatalf("transcript is %d tokens, over its budget of %d", size, row.budget)
			}
			for _, want := range row.want {
				if !strings.Contains(body, want) {
					t.Errorf("transcript lost %q", want)
				}
			}
			for _, absent := range row.absent {
				if strings.Contains(body, absent) {
					t.Errorf("transcript kept %q", absent)
				}
			}
			if strings.Count(body, "earlier records of this turn omitted]") > 1 {
				t.Error("the omission is marked more than once")
			}
		})
	}
	t.Run("marker counts the dropped records", func(t *testing.T) {
		prepared, err := Prepare(turn, limit(omitted-1), output)
		if err != nil {
			t.Fatal(err)
		}
		var dropped int
		at := strings.Index(prepared, " earlier records of this turn omitted]")
		if _, err := fmt.Sscanf(prepared[strings.LastIndex(prepared[:at], "["):at], "[%d", &dropped); err != nil || dropped < 1 {
			t.Fatalf("marker count: %v", err)
		}
		// Every record after the user's is one part; the kept ones follow the marker.
		kept := strings.Count(prepared[at:], "tool_request(") + strings.Count(prepared[at:], "tool_response: ")
		if dropped+kept != len(turn)-1 {
			t.Fatalf("%d dropped and %d kept of %d records", dropped, kept, len(turn)-1)
		}
	})
	t.Run("older turns go first", func(t *testing.T) {
		older := longTurn(2)
		older[0] = canonical.NewRecord(older[0].Turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Older request")), older[0].Created)
		prepared, err := Prepare(append(older, turn...), limit(whole), output)
		if err != nil {
			t.Fatal(err)
		}
		if strings.Contains(prepared, "Older request") || !strings.Contains(prepared, "MIDDLE-11") || strings.Contains(prepared, "characters omitted") {
			t.Fatal("the latest turn was shortened while an older turn could still be dropped")
		}
	})
	t.Run("turn without a user message", func(t *testing.T) {
		prepared, err := Prepare(turn[1:], limit(omitted/2), output)
		if err != nil || !strings.Contains(prepared, "earlier records of this turn omitted]") || !strings.Contains(prepared, "file-11.go") {
			t.Fatalf("prepared = %v", err)
		}
		if _, err = Prepare(turn[len(turn)-1:], limit(3), output); err == nil || err.Error() != "latest record exceeds the summary context limit" {
			t.Fatalf("error = %v", err)
		}
	})
}

func TestPrepareFitsSmallWindows(t *testing.T) {
	for _, contextLimit := range []uint64{4096, 8192} {
		t.Run(fmt.Sprint(contextLimit), func(t *testing.T) {
			output := Output(contextLimit)
			prepared, err := Prepare(longTurn(12), contextLimit, output, Request)
			if err != nil {
				t.Fatal(err)
			}
			request := canonical.NewRecord(protocol.NewTurnID(), "message", canonical.NewMessage(canonical.User, canonical.Text(0, Request)), time.Time{})
			if total := tokens.Chat(prepared, []canonical.Record{request}) + output; total > contextLimit {
				t.Fatalf("summary input and output use %d tokens in a %d-token window", total, contextLimit)
			}
			if !strings.Contains(prepared, "Kick off the migration exactly as specified") {
				t.Fatal("fitting the small window lost the current user request")
			}
		})
	}
}
