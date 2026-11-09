package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type referenceTruncation struct {
	Truncated bool              `json:"truncated"`
	Entries   []string          `json:"truncated_entries"`
	Inputs    []protocol.TurnID `json:"truncated_inputs"`
}

func newReferenceTruncation() referenceTruncation {
	return referenceTruncation{Entries: []string{}, Inputs: []protocol.TurnID{}}
}
func (t *referenceTruncation) entry(id string) {
	t.Truncated = true
	if !slices.Contains(t.Entries, id) {
		t.Entries = append(t.Entries, id)
		slices.Sort(t.Entries)
	}
}
func referencePrefix(text string, limit int) string {
	end := min(len(text), limit)
	for end > 0 && end < len(text) && !utf8.RuneStart(text[end]) {
		end--
	}
	return text[:end]
}
func referenceTrim(text *string, limit int) bool {
	if len(*text) <= limit {
		return false
	}
	*text = referencePrefix(*text, limit)
	return true
}

type referenceTurn struct {
	Turn     protocol.TurnID                 `json:"turn"`
	Worktree protocol.WorktreeID             `json:"worktree"`
	Sequence uint64                          `json:"sequence"`
	Kind     protocol.ConversationRunKind    `json:"kind"`
	Status   protocol.ConversationStatus     `json:"status"`
	Error    *protocol.Fault                 `json:"error"`
	Started  *int64                          `json:"started_ms"`
	Finished *int64                          `json:"finished_ms"`
	Input    *protocol.ConversationSentInput `json:"input"`
}

func boundedReferenceRun(run protocol.ConversationRun, t *referenceTruncation) referenceTurn {
	if run.Error != nil {
		v := *run.Error
		run.Error = &v
		t.Truncated = referenceTrim(&v.Message, 2048) || t.Truncated
	}
	if run.Input != nil {
		v := *run.Input
		run.Input = &v
		trimmed := referenceTrim(&v.Message.Text, 8192)
		trimmed = trimmed || len(v.Message.References) > 16 || len(v.Message.Attachments) > 16 || len(v.Attachments) > 16
		v.Message.References = v.Message.References[:min(16, len(v.Message.References))]
		v.Message.Attachments = v.Message.Attachments[:min(16, len(v.Message.Attachments))]
		v.Attachments = v.Attachments[:min(16, len(v.Attachments))]
		if trimmed {
			t.Truncated = true
			if !slices.Contains(t.Inputs, run.Turn) {
				t.Inputs = append(t.Inputs, run.Turn)
				slices.SortFunc(t.Inputs, func(a, b protocol.TurnID) int { return strings.Compare(a.String(), b.String()) })
			}
		}
	}
	return referenceTurn{Turn: run.Turn, Worktree: run.Worktree, Sequence: run.Sequence, Kind: run.Kind, Status: run.Status, Error: run.Error, Started: run.StartedMs, Finished: run.FinishedMs, Input: run.Input}
}
func boundedReferenceEntry(ctx context.Context, db *sql.Conn, session protocol.SessionID, entry protocol.ConversationEntry, t *referenceTruncation) (map[string]any, error) {
	parts := []any{}
	size := 0
	trimmed := false
	var record *canonical.Record
	var sequence uint64
	for _, part := range entry.Parts {
		if part.Kind == "tool_call" || part.Kind == "tool_result" {
			var body []byte
			if err := db.QueryRowContext(ctx, "SELECT e.sequence,e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND e.turn=?2 AND e.id=?3", session.String(), entry.Turn.String(), entry.ID).Scan(&sequence, &body); err != nil {
				return nil, err
			}
			var value canonical.Record
			if err := json.Unmarshal(body, &value); err != nil {
				return nil, err
			}
			record = &value
			break
		}
	}
	for index, part := range entry.Parts {
		encoded, err := encode(part)
		if err != nil {
			return nil, err
		}
		value := agentObject(agentJSON(encoded))
		if part.Kind == "tool_call" || part.Kind == "tool_result" {
			var raw *rawAgentTool
			if record != nil {
				if m := record.Message(); m != nil {
					for _, block := range m.Content {
						if block.Index == uint64(index) {
							raw, err = storedAgentTool(ctx, db, *record, sequence, block)
							break
						}
					}
				} else {
					raw, err = reviewRaw(*record, uint64(index))
				}
			}
			if err != nil {
				return nil, err
			}
			if raw == nil {
				return nil, fault(protocol.ErrorCodeNotFound, "history tool value is unavailable")
			}
			data := agentObject(value["data"])
			delete(data, "summary")
			if part.Kind == "tool_call" {
				data["arguments"] = agentJSON(raw.Arguments)
				if raw.Display != nil {
					data["display"] = raw.Display
				}
			} else {
				data["result"] = nil
				if raw.Result != nil {
					data["result"] = agentJSON(*raw.Result)
				}
			}
		}
		encoded, err = encode(value)
		if err != nil {
			return nil, err
		}
		if len(encoded) > 4096 {
			trimmed = true
			switch part.Kind {
			case "text", "thinking":
				text, _ := value["data"].(string)
				value["data"] = referencePrefix(text, 2048)
			case "tool_result":
				data := agentObject(value["data"])
				result, e := encode(data["result"])
				if e != nil {
					return nil, e
				}
				data["result"] = map[string]any{"truncated": true, "preview": referencePrefix(string(result), 2048)}
				data["images"] = []any{}
			default:
				part := resourcePart(map[string]any{"type": "truncated_history_part", "preview": referencePrefix(string(encoded), 2048)})
				body, e := encode(part)
				if e != nil {
					return nil, e
				}
				value = agentObject(agentJSON(body))
			}
		}
		encoded, err = encode(value)
		if err != nil {
			return nil, err
		}
		if size+len(encoded) > 8192 || len(parts) == 32 {
			trimmed = true
			break
		}
		size += len(encoded)
		parts = append(parts, value)
	}
	entry.Parts = []protocol.ConversationPart{}
	if entry.SearchSuggestions != nil {
		v := *entry.SearchSuggestions
		trimmed = referenceTrim(&v, 2048) || trimmed
		entry.SearchSuggestions = &v
	}
	if len(entry.Citations) > 16 {
		entry.Citations = entry.Citations[:16]
		trimmed = true
	}
	if trimmed {
		t.entry(entry.ID)
	}
	body, err := encode(entry)
	if err != nil {
		return nil, err
	}
	value := agentObject(agentJSON(body))
	value["parts"] = parts
	return value, nil
}

type referencePage struct {
	Session    protocol.SessionID `json:"session"`
	Revision   uint64             `json:"revision"`
	Entries    []map[string]any   `json:"entries"`
	Runs       []referenceTurn    `json:"runs"`
	NextBefore *protocol.TurnID   `json:"next_before"`
}
type referenceConversation struct {
	Page    referencePage     `json:"page"`
	Missing []protocol.TurnID `json:"missing"`
	referenceTruncation
}

func boundedReferenceConversation(ctx context.Context, db *sql.Conn, session protocol.SessionID, history protocol.ConversationHistory) ([]byte, error) {
	response := referenceConversation{Page: referencePage{Session: history.Page.Session, Revision: history.Page.Revision, Entries: []map[string]any{}, Runs: []referenceTurn{}, NextBefore: history.Page.NextBefore}, Missing: append([]protocol.TurnID{}, history.Missing...), referenceTruncation: newReferenceTruncation()}
	for _, entry := range history.Page.Entries {
		value, err := boundedReferenceEntry(ctx, db, session, entry, &response.referenceTruncation)
		if err != nil {
			return nil, err
		}
		response.Page.Entries = append(response.Page.Entries, value)
	}
	for _, run := range history.Page.Runs {
		response.Page.Runs = append(response.Page.Runs, boundedReferenceRun(run, &response.referenceTruncation))
	}
	for {
		slices.SortFunc(response.Missing, func(a, b protocol.TurnID) int { return strings.Compare(a.String(), b.String()) })
		encoded, err := encode(response)
		if err != nil {
			return nil, err
		}
		if len(encoded) <= 40*1024 {
			return encoded, nil
		}
		response.Truncated = true
		if len(response.Page.Entries) > 1 {
			entry := response.Page.Entries[0]
			response.Page.Entries = response.Page.Entries[1:]
			turn, e := protocol.ParseTurnID(entry["turn"].(string))
			if e != nil {
				return nil, e
			}
			if !slices.Contains(response.Missing, turn) {
				response.Missing = append(response.Missing, turn)
			}
			response.Entries = slices.DeleteFunc(response.Entries, func(id string) bool { return id == entry["id"] })
			continue
		}
		if len(response.Page.Runs) > 1 {
			run := response.Page.Runs[0]
			response.Page.Runs = response.Page.Runs[1:]
			response.Page.Entries = slices.DeleteFunc(response.Page.Entries, func(entry map[string]any) bool { return entry["turn"] == run.Turn.String() })
			response.Missing = slices.DeleteFunc(response.Missing, func(id protocol.TurnID) bool { return id == run.Turn })
			response.Inputs = slices.DeleteFunc(response.Inputs, func(id protocol.TurnID) bool { return id == run.Turn })
			next := response.Page.Runs[0].Turn
			response.Page.NextBefore = &next
			continue
		}
		return nil, invalid("history metadata exceeds the model output budget")
	}
}

type referenceContinuation struct {
	Revision   uint64           `json:"revision"`
	Run        referenceTurn    `json:"run"`
	Entries    []map[string]any `json:"entries"`
	NextBefore *uint64          `json:"next_before"`
	referenceTruncation
}

func boundedReferenceTurn(ctx context.Context, db *sql.Conn, session protocol.SessionID, history protocol.ConversationTurnHistory) ([]byte, error) {
	response := referenceContinuation{Revision: history.Revision, Entries: []map[string]any{}, NextBefore: history.NextBefore, referenceTruncation: newReferenceTruncation()}
	response.Run = boundedReferenceRun(history.Run, &response.referenceTruncation)
	for _, entry := range history.Entries {
		value, err := boundedReferenceEntry(ctx, db, session, entry, &response.referenceTruncation)
		if err != nil {
			return nil, err
		}
		response.Entries = append(response.Entries, value)
	}
	for {
		encoded, err := encode(response)
		if err != nil {
			return nil, err
		}
		if len(encoded) <= 40*1024 {
			return encoded, nil
		}
		if len(response.Entries) <= 1 {
			return nil, invalid("turn metadata exceeds the model output budget")
		}
		entry := response.Entries[0]
		response.Entries = response.Entries[1:]
		response.referenceTruncation.Entries = slices.DeleteFunc(response.referenceTruncation.Entries, func(id string) bool { return id == entry["id"] })
		response.Truncated = true
		value := response.Entries[0]["sequence"].(json.Number)
		n, err := value.Int64()
		if err != nil {
			return nil, err
		}
		v := uint64(n)
		response.NextBefore = &v
	}
}
