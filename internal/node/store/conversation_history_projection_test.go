package store

import (
	"bytes"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const historyConfigurationFixtureGuide = "sailry.context.guide:fixture"
const historyConfigurationFixtureRows = " FROM agent_events e CROSS JOIN conversation_events h ON h.sequence=e.sequence AND h.session=?1 WHERE e.turn=?2 AND " + visibleCanonical

type historyConfigurationFixture struct {
	storage *Store
	session protocol.SessionID
	turn    protocol.TurnID
	live    []protocol.ConversationEntry
	guide   string
}

func newHistoryConfigurationFixture(t testing.TB) historyConfigurationFixture {
	t.Helper()
	storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	f := historyConfigurationFixture{storage: storage, session: session.ID, turn: turn, guide: strings.Repeat("abcd", 448*1024)}
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		f.live = nil
		for i := range 2 {
			text := fmt.Sprint(i, ": ", strings.Repeat("Full visible text <é>\n", 512))
			message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, text))
			configuration := canonical.Configuration{Context: canonical.Context{
				Captured: true, Policy: new("fixture policy"), Environment: new("fixture environment"), Personal: new("fixture personal instructions"),
				Guides: map[string]canonical.Guide{historyConfigurationFixtureGuide: {Path: "AGENTS.md", Revision: fmt.Sprint(i), Content: f.guide}},
			}}
			for _, record := range []canonical.Record{
				canonical.NewRecord(turn, "message", &message, now),
				canonical.NewRecord(turn, "configuration", &configuration, now),
			} {
				record.Provenance = canonical.Provenance{Author: "assistant", Branch: "main"}
				entry, _, err := AppendCanonical(ctx, db, f.session, record)
				if err != nil {
					return err
				}
				f.live = append(f.live, entry)
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	return f
}

func TestHistoryConfigurationProjectionKeepsLiveEntriesAndTurnCursor(t *testing.T) {
	t.Parallel()
	f := newHistoryConfigurationFixture(t)
	ctx := t.Context()
	if err := f.storage.Node.Read(ctx, func(db *sql.Conn) error {
		var raw, projected int
		const sizes = "SELECT sum(" + historyStoredBytes + "),sum(length(CAST(" + historyEntryBody + " AS BLOB)))" + historyConfigurationFixtureRows
		if err := db.QueryRowContext(ctx, sizes, f.session.String(), f.turn.String()).Scan(&raw, &projected); err != nil {
			return err
		}
		if raw <= 3*1024*1024 || projected == 0 || projected > raw/20 {
			return fmt.Errorf("fixture body bytes: raw %d, projected %d", raw, projected)
		}

		head, err := ReadConversation(ctx, db, f.session, nil, 20)
		if err != nil {
			return err
		}
		if !reflect.DeepEqual(head.Page.Entries, f.live[2:]) || len(head.Missing) != 1 || head.Missing[0] != f.turn {
			return errors.New("history head differs from live entries or lost its missing turn")
		}
		page, err := ReadConversationTurn(ctx, db, f.session, f.turn, head.Page.Revision, nil, 100)
		if err != nil {
			return err
		}
		if !reflect.DeepEqual(page.Entries, f.live[2:]) || page.NextBefore == nil || *page.NextBefore != f.live[2].Sequence {
			return errors.New("newer turn page changed entries or original byte boundary")
		}
		older, err := ReadConversationTurn(ctx, db, f.session, f.turn, head.Page.Revision, page.NextBefore, 100)
		if err != nil {
			return err
		}
		if !reflect.DeepEqual(older.Entries, f.live[:2]) || older.NextBefore != nil {
			return errors.New("older turn page skipped or repeated live entries")
		}

		state, err := LoadAgentState(ctx, db, f.session)
		if err != nil {
			return err
		}
		for i := 1; i < len(f.live); i += 2 {
			if i >= len(state.Records) {
				return errors.New("replay lost a configuration record")
			}
			configuration, ok := state.Records[i].Data.(*canonical.Configuration)
			if !ok || configuration.Context.Guides[historyConfigurationFixtureGuide].Content != f.guide {
				return errors.New("replay lost the captured guide")
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestHistoryConfigurationProjectionDoesNotRepairCorruptRows(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	large := strings.Repeat("x", 4096)
	prefix := fmt.Sprintf(`{"id":"fixture","created":"2026-11-10T00:00:00Z","turn":%q,"provenance":{"author":"assistant","branch":"main"},"metadata":{},"kind":"configuration","context":`, protocol.NewTurnID().String())
	for _, sample := range []struct {
		name      string
		context   string
		unchanged bool
	}{
		{"wrong scalar type", `{"captured":true,"policy":["` + large + `"]}`, true},
		{"wrong guide map type", `{"captured":true,"policy":"` + large + `","guides":[]}`, true},
		{"wrong guide type", `{"captured":true,"guides":{"key":"` + large + `"}}`, true},
		{"missing guide field", `{"captured":true,"guides":{"key":{"path":"AGENTS.md","content":"` + large + `"}}}`, true},
		{"escaped duplicate guide field", `{"captured":true,"guides":{"key":{"path":"AGENTS.md","revision":"1","content":"` + large + `","co\u006etent":"duplicate"}}}`, true},
		{"duplicate scalar", `{"captured":true,"policy":"` + large + `","policy":"duplicate"}`, false},
		{"invalid retained field", `{"captured":17,"policy":"` + large + `"}`, false},
		{"non-JSON syntax", `{captured:true,"policy":"` + large + `"}`, true},
	} {
		t.Run(sample.name, func(t *testing.T) {
			original := []byte(prefix + sample.context + "}")
			var projected []byte
			if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
				return db.QueryRowContext(ctx, "WITH e(body) AS (VALUES (?1)) SELECT "+historyEntryBody+" FROM e", original).Scan(&projected)
			}); err != nil {
				t.Fatal(err)
			}
			var record canonical.Record
			if json.Unmarshal(original, &record) == nil || json.Unmarshal(projected, &record) == nil {
				t.Fatal("corrupt configuration decoded successfully")
			}
			if sample.unchanged && !bytes.Equal(original, projected) {
				t.Fatal("projection rewrote a configuration that must use the original decoder input")
			}
		})
	}
}

func BenchmarkHistoryConfigurationReadDecode(b *testing.B) {
	f := newHistoryConfigurationFixture(b)
	for _, sample := range []struct{ name, body string }{{"stored", "e.body"}, {"projected", historyEntryBody}} {
		b.Run(sample.name, func(b *testing.B) {
			ctx := b.Context()
			query := "SELECT e.sequence," + historyStoredBytes + "," + sample.body + historyConfigurationFixtureRows + " ORDER BY e.sequence"
			b.ReportAllocs()
			var bodyBytes uint64
			for b.Loop() {
				if err := f.storage.Node.Read(ctx, func(db *sql.Conn) error {
					rows, err := db.QueryContext(ctx, query, f.session.String(), f.turn.String())
					if err != nil {
						return err
					}
					defer rows.Close()
					for rows.Next() {
						var seq uint64
						var storedBytes int
						var body sql.RawBytes
						if err = rows.Scan(&seq, &storedBytes, &body); err != nil {
							return err
						}
						var record canonical.Record
						if err = json.Unmarshal(body, &record); err != nil {
							return err
						}
						bodyBytes += uint64(len(body))
					}
					return rows.Err()
				}); err != nil {
					b.Fatal(err)
				}
			}
			b.ReportMetric(float64(bodyBytes)/float64(b.N), "body-B/op")
		})
	}
}
