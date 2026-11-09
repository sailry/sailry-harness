//go:build integration

package store

import (
	"context"
	"database/sql"
	"database/sql/driver"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"path/filepath"
	"runtime"
	"slices"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	knowledgeText "github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
	sqlite "modernc.org/sqlite"
)

const storageLoadSessions = 20
const storageLoadTransactions = 250
const storageLoadParts = 4
const storageLoadCodeFiles = 1000
const storageLoadCollection = "01890f00-1234-7000-8000-000000000002"
const storageLoadWorktree = "01890f00-1234-7000-8000-000000000001"

const storageLoadFilter = "JOIN sources s ON s.id=c.source JOIN collections k ON k.id=s.collection JOIN json_each(?1) admitted ON json_extract(admitted.value,'$.id')=k.id AND json_extract(admitted.value,'$.scope')=lower(hex(k.scope)) AND json_extract(admitted.value,'$.rebuild')=k.rebuild AND (json_extract(admitted.value,'$.generation')=k.generation OR (json_extract(admitted.value,'$.semantic')=0 AND k.generation<=json_extract(admitted.value,'$.generation'))) WHERE NOT EXISTS(SELECT 1 FROM json_each(?2) wanted WHERE NOT EXISTS(SELECT 1 FROM json_each(s.tags) actual WHERE actual.value=wanted.value)) AND (?3 IS NULL OR substr(s.locator,1,length(?3))=?3) AND (?4 IS NULL OR s.modified_ms>=?4) AND (?5 IS NULL OR s.modified_ms<=?5)"
const storageLoadKeywordSQL = "WITH candidates AS MATERIALIZED (SELECT rowid AS id,-bm25(chunks_fts,2,1,2,1) AS score FROM chunks_fts WHERE chunks_fts MATCH ?6) SELECT c.key,f.score FROM candidates f JOIN chunks c ON c.id=f.id " + storageLoadFilter + " AND (json_array_length(?9)=0 OR EXISTS(SELECT 1 FROM json_each(?9) wanted JOIN json_each(s.tags) actual ON actual.value=wanted.value)) ORDER BY f.score DESC,c.key LIMIT 100"

type storageLoadMeasurements struct {
	mu             sync.Mutex
	commits        []time.Duration
	operations     []time.Duration
	commitFailures int
	writes         atomic.Int64
	history        atomic.Int64
	search         atomic.Int64
}

type storageLoadReport struct {
	Platform             string  `json:"platform"`
	GoVersion            string  `json:"go_version"`
	Sessions             int     `json:"sessions"`
	Transactions         int     `json:"transactions_per_session"`
	CommittedRecords     int64   `json:"committed_records"`
	RecordWritesSecond   float64 `json:"record_writes_per_second"`
	CommitP99MS          float64 `json:"commit_p99_ms"`
	OperationP99MS       float64 `json:"operation_p99_ms"`
	CommitAttempts       int     `json:"commit_attempts"`
	CommitFailures       int     `json:"commit_failures"`
	HistoryReads         int64   `json:"history_reads"`
	SearchReads          int64   `json:"search_reads"`
	CodeRebuildMS        float64 `json:"code_rebuild_ms"`
	IndexedSymbols       int     `json:"indexed_symbols"`
	WriterElapsedSeconds float64 `json:"writer_elapsed_seconds"`
}

// TestStorageLoad is the manual storage measurement, including real file I/O.
// It creates a disposable SQL corpus rather than a conversation lifecycle fixture.
func TestStorageLoad(t *testing.T) {
	ctx := context.Background()
	profile, err := core.AcquireProfile(filepath.Join(t.TempDir(), "profile"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := profile.Close(); err != nil {
			t.Error(err)
		}
	})
	storage, err := Open(ctx, profile.Path, protocol.NodeID{1})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := storage.Close(); err != nil {
			t.Error(err)
		}
	})
	codeDirectory := filepath.Join(profile.Path, "cache", "code-index", storageLoadWorktree)
	if err := core.PrivateDirectory(codeDirectory); err != nil {
		t.Fatal(err)
	}
	code, err := OpenHandle(ctx, filepath.Join(codeDirectory, "index.sqlite"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := code.Close(); err != nil {
			t.Error(err)
		}
	})
	if err := code.Write(ctx, func(connection *sql.Conn) error {
		_, err := connection.ExecContext(ctx, sqliteSchema(CodeSchema))
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := storageLoadSeedKnowledge(ctx, storage.Knowledge); err != nil {
		t.Fatal(err)
	}
	if err := storageLoadKnowledgeChecks(ctx, storage.Knowledge); err != nil {
		t.Fatal(err)
	}
	if err := storageLoadSeedSessions(ctx, storage); err != nil {
		t.Fatal(err)
	}
	measurements := &storageLoadMeasurements{}
	storageLoadObserveCommits(t, storage.Node, measurements)
	report, err := storageLoadRun(ctx, storage, code, measurements)
	encoded, encodeErr := json.Marshal(report)
	if encodeErr != nil {
		t.Fatal(encodeErr)
	}
	t.Log(string(encoded))
	if err != nil {
		t.Fatal(err)
	}
	if report.CommittedRecords != storageLoadSessions*storageLoadTransactions*storageLoadParts {
		t.Errorf("committed %d records, want %d", report.CommittedRecords, storageLoadSessions*storageLoadTransactions*storageLoadParts)
	}
	if report.HistoryReads == 0 || report.SearchReads == 0 || report.IndexedSymbols != storageLoadCodeFiles {
		t.Error("load did not include concurrent history/search reads and the complete code index")
	}
	if report.CommitAttempts == 0 || report.CommitFailures != 0 {
		t.Errorf("commit observations: %d attempts, %d failures", report.CommitAttempts, report.CommitFailures)
	}
	if report.CommitP99MS > 20 {
		t.Errorf("commit p99 %.3f ms exceeds 20 ms", report.CommitP99MS)
	}
	if report.RecordWritesSecond < 2000 {
		t.Errorf("%.1f record writes/s is below 2000", report.RecordWritesSecond)
	}
}

// The decorator observes the actual COMMIT at the SQL driver boundary. It does
// not replace the production writer's batching, savepoints or acknowledgment.
type storageLoadConnector struct {
	base         driver.Connector
	measurements *storageLoadMeasurements
}

func (connector *storageLoadConnector) Connect(ctx context.Context) (driver.Conn, error) {
	connection, err := connector.base.Connect(ctx)
	if err != nil {
		return nil, err
	}
	return &storageLoadConnection{Conn: connection, measurements: connector.measurements}, nil
}

func (connector *storageLoadConnector) Driver() driver.Driver { return connector.base.Driver() }

type storageLoadConnection struct {
	driver.Conn
	measurements *storageLoadMeasurements
}

func (connection *storageLoadConnection) ExecContext(ctx context.Context, query string, arguments []driver.NamedValue) (driver.Result, error) {
	executor, ok := connection.Conn.(driver.ExecerContext)
	if !ok {
		return nil, driver.ErrSkip
	}
	if query != "COMMIT" {
		return executor.ExecContext(ctx, query, arguments)
	}
	began := time.Now()
	result, err := executor.ExecContext(ctx, query, arguments)
	elapsed := time.Since(began)
	connection.measurements.mu.Lock()
	connection.measurements.commits = append(connection.measurements.commits, elapsed)
	if err != nil {
		connection.measurements.commitFailures++
	}
	connection.measurements.mu.Unlock()
	return result, err
}

func (connection *storageLoadConnection) QueryContext(ctx context.Context, query string, arguments []driver.NamedValue) (driver.Rows, error) {
	querier, ok := connection.Conn.(driver.QueryerContext)
	if !ok {
		return nil, driver.ErrSkip
	}
	return querier.QueryContext(ctx, query, arguments)
}

func (connection *storageLoadConnection) PrepareContext(ctx context.Context, query string) (driver.Stmt, error) {
	if preparer, ok := connection.Conn.(driver.ConnPrepareContext); ok {
		return preparer.PrepareContext(ctx, query)
	}
	return connection.Conn.Prepare(query)
}

func (connection *storageLoadConnection) CheckNamedValue(value *driver.NamedValue) error {
	if checker, ok := connection.Conn.(driver.NamedValueChecker); ok {
		return checker.CheckNamedValue(value)
	}
	return driver.ErrSkip
}

func storageLoadObserveCommits(t *testing.T, handle *Handle, measurements *storageLoadMeasurements) {
	t.Helper()
	ctx := context.Background()
	connector, err := sqlite.NewConnector(databaseDSN(handle.Path(), false))
	if err != nil {
		t.Fatal(err)
	}
	observed := sql.OpenDB(&storageLoadConnector{base: connector, measurements: measurements})
	observed.SetMaxOpenConns(1)
	observed.SetMaxIdleConns(1)
	connection, err := observed.Conn(ctx)
	if err != nil {
		observed.Close()
		t.Fatal(err)
	}
	exclusive, err := handle.Exclusive(ctx)
	if err != nil {
		connection.Close()
		observed.Close()
		t.Fatal(err)
	}
	handle.mu.Lock()
	writer := handle.state.writer
	original := writer.connection
	writer.connection = connection
	handle.mu.Unlock()
	if err := exclusive.Close(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		exclusive, err := handle.Exclusive(context.Background())
		if err != nil {
			t.Error(err)
			return
		}
		handle.mu.Lock()
		writer.connection = original
		handle.mu.Unlock()
		err = errors.Join(connection.Close(), observed.Close(), exclusive.Close())
		if err != nil {
			t.Error(err)
		}
	})
}

func storageLoadSeedKnowledge(ctx context.Context, handle *Handle) error {
	return handle.Write(ctx, func(connection *sql.Conn) error {
		if err := storageLoadExec(ctx, connection, "INSERT INTO collections(id,generation,scope,rebuild) VALUES(?,1,?,0)", storageLoadCollection, make([]byte, 32)); err != nil {
			return err
		}
		for index, document := range []struct{ path, text, tags string }{
			{"docs/selected/vector.md", "我们使用向量数据库", `["selected"]`},
			{"docs/excluded/vector.md", "向量数据库", `["excluded"]`},
			{"docs/literal/complete.md", "Rust C++ café language", `["selected"]`},
			{"docs/literal/unrelated.md", "secretive", `[]`},
			{"docs/rank/complete.md", "Rust handles cancellation of running async operations", `[]`},
			{"docs/rank/partial.md", "Rust language syntax", `[]`},
			{"docs/rank/unrelated.md", "Database pool limits", `[]`},
			{"docs/cjk/complete.md", "管理资料，使用关键词完成检索，也提供有效方法", `[]`},
			{"docs/cjk/partial.md", "资料归档", `[]`},
			{"docs/cjk/unrelated.md", "資方交付料品，检查锁方与索引方位", `[]`},
		} {
			id := index + 1
			if err := storageLoadExec(ctx, connection, "INSERT INTO sources(id,collection,locator,revision,modified_ms,tags,embedded) VALUES(?,?,?,'1',20,?,0)", id, storageLoadCollection, document.path, document.tags); err != nil {
				return err
			}
			if err := storageLoadExec(ctx, connection, "INSERT INTO chunks(id,key,source,ordinal,title,text,field0,field1,cjk0,cjk1,position) VALUES(?,?,?,0,'',?,'',?,'',?,'{}')", id, document.path, id, document.text, knowledgeText.Field(document.text), knowledgeText.Index(document.text)); err != nil {
				return err
			}
		}
		return nil
	})
}

func storageLoadKeyword(ctx context.Context, connection *sql.Conn, text, tags, prefix string) ([]string, error) {
	return storageLoadKeywordExpression(ctx, connection, knowledgeText.Query(text), tags, prefix)
}

func storageLoadKeywordExpression(ctx context.Context, connection *sql.Conn, terms, tags, prefix string) ([]string, error) {
	if terms == "" {
		return []string{}, nil
	}
	var path *string
	if prefix != "" {
		path = &prefix
	}
	admitted := fmt.Sprintf(`[{"id":%q,"scope":%q,"generation":1,"rebuild":0,"semantic":true}]`, storageLoadCollection, strings.Repeat("0", 64))
	rows, err := connection.QueryContext(ctx, storageLoadKeywordSQL, admitted, tags, path, nil, nil, terms, nil, nil, "[]")
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	keys := []string{}
	previous := math.Inf(1)
	for rows.Next() {
		var key string
		var score float64
		if err := rows.Scan(&key, &score); err != nil {
			return nil, err
		}
		if math.IsNaN(score) || math.IsInf(score, 0) || score <= 0 || score > previous {
			return nil, fmt.Errorf("invalid FTS score ordering: %.6f after %.6f", score, previous)
		}
		previous = score
		keys = append(keys, key)
	}
	return keys, rows.Err()
}

func storageLoadKnowledgeChecks(ctx context.Context, handle *Handle) error {
	return handle.Read(ctx, func(connection *sql.Conn) error {
		for _, fixture := range []struct {
			text, tags, prefix string
			want               []string
		}{
			{"向量", `["selected"]`, "docs/selected/", []string{"docs/selected/vector.md"}},
			{"向量", `["missing"]`, "", []string{}},
			{"c++", `[]`, "docs/literal/", []string{"docs/literal/complete.md"}},
			{"CAFE", `[]`, "docs/literal/", []string{"docs/literal/complete.md"}},
			{"text:rust", `[]`, "docs/literal/", []string{"docs/literal/complete.md"}},
			{"secret*", `[]`, "docs/literal/", []string{}},
			{"rust async cancellation", `[]`, "docs/rank/", []string{"docs/rank/complete.md", "docs/rank/partial.md"}},
			{"cancellation rust async", `[]`, "docs/rank/", []string{"docs/rank/complete.md", "docs/rank/partial.md"}},
			{"资料检索方法", `[]`, "docs/cjk/", []string{"docs/cjk/complete.md", "docs/cjk/partial.md"}},
			{"方法检索资料", `[]`, "docs/cjk/", []string{"docs/cjk/complete.md", "docs/cjk/partial.md"}},
			{"档", `[]`, "docs/cjk/", []string{"docs/cjk/partial.md"}},
		} {
			keys, err := storageLoadKeyword(ctx, connection, fixture.text, fixture.tags, fixture.prefix)
			if err != nil {
				return fmt.Errorf("knowledge query %q: %w", fixture.text, err)
			}
			if !slices.Equal(keys, fixture.want) {
				return fmt.Errorf("knowledge query %q = %v, want %v", fixture.text, keys, fixture.want)
			}
		}
		for _, fixture := range []struct {
			text, tags, prefix string
			want               []string
		}{
			{"C++", `[]`, "docs/literal/", []string{"docs/literal/complete.md"}},
			{"Rust C++ café", `[]`, "docs/literal/", []string{"docs/literal/complete.md"}},
			{"向量", `["selected"]`, "docs/selected/", []string{"docs/selected/vector.md"}},
		} {
			keys, err := storageLoadKeywordExpression(ctx, connection, knowledgeText.Phrase(fixture.text), fixture.tags, fixture.prefix)
			if err != nil {
				return fmt.Errorf("knowledge phrase %q: %w", fixture.text, err)
			}
			if !slices.Equal(keys, fixture.want) {
				return fmt.Errorf("knowledge phrase %q = %v, want %v", fixture.text, keys, fixture.want)
			}
		}
		return nil
	})
}

func storageLoadSeedSessions(ctx context.Context, storage *Store) error {
	configuration := protocol.DefaultSessionConfig()
	configuration.Provider = protocol.NewProviderID()
	configuration.Model = "fixture"
	configuration.Mode = protocol.WorkModeCode
	configuration.Permission = protocol.PermissionProject
	config, err := json.Marshal(configuration)
	if err != nil {
		return err
	}
	selection := protocol.DefaultRoleSnapshot()
	selection.Execution = protocol.ConversationExecutionModel
	roles, err := json.Marshal(selection)
	if err != nil {
		return err
	}
	outcome, err := json.Marshal(protocol.Result[protocol.Output, protocol.Fault]{
		Err: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Fixture request completed"},
	})
	if err != nil {
		return err
	}
	return storage.Node.Write(ctx, func(connection *sql.Conn) error {
		if err := storageLoadExec(ctx, connection, "INSERT INTO worktrees(id,path,main) VALUES(?,?,1)", storageLoadWorktree, filepath.Join(storage.Profile, "fixture-workspace")); err != nil {
			return err
		}
		for index := 0; index < storageLoadSessions; index++ {
			session, turn := storageLoadIDs(index)
			sessionRow, err := NextCounter(ctx, connection, CounterSessions)
			if err != nil {
				return err
			}
			if err := storageLoadExec(ctx, connection, "INSERT INTO sessions(rowid,id,worktree,revision) VALUES(?,?,?,1)", sessionRow, session, storageLoadWorktree); err != nil {
				return err
			}
			if err := storageLoadExec(ctx, connection, "INSERT INTO session_revisions(session,revision,worktree,config,roles) VALUES(?,1,?,?,?)", session, storageLoadWorktree, config, roles); err != nil {
				return err
			}
			requestID, err := protocol.ParseRequestID(turn)
			if err != nil {
				return err
			}
			request, err := json.Marshal(protocol.Request{Version: 1, ID: requestID, Target: storage.NodeID, Command: protocol.Command{Kind: "snapshot"}})
			if err != nil {
				return err
			}
			if err := storageLoadExec(ctx, connection, "INSERT INTO requests(caller,id,body,status,result) VALUES(?,?,?,'completed',?)", storage.NodeID[:], turn, request, string(outcome)); err != nil {
				return err
			}
			turnRow, err := NextCounter(ctx, connection, CounterTurns)
			if err != nil {
				return err
			}
			if err := storageLoadExec(ctx, connection, "INSERT INTO turns(rowid,id,kind,plugins,roles,session,revision,request,caller) VALUES(?,?,'task',X'5B5D',?,?,1,?,?)", turnRow, turn, roles, session, turn, storage.NodeID[:]); err != nil {
				return err
			}
			if err := storageLoadExec(ctx, connection, "INSERT INTO conversation_turns(session,turn) VALUES(?,?)", session, turn); err != nil {
				return err
			}
		}
		return nil
	})
}

func storageLoadIDs(index int) (string, string) {
	return fmt.Sprintf("01890f00-1234-7000-8000-%012d", 100+index), fmt.Sprintf("01890f00-1234-7000-8000-%012d", 200+index)
}

func storageLoadRecord(sequence uint64, session, turn string, part int) ([]byte, error) {
	block := map[string]any{"index": 0, "type": "text", "text": strings.Repeat("streamed text delta ", 64)}
	call := fmt.Sprintf("tool-%d", sequence-uint64(part))
	if part == 2 {
		block = map[string]any{"index": 0, "type": "tool_request", "id": call, "name": "read_file", "arguments": map[string]any{"path": "fixture.go"}}
	} else if part == 3 {
		value := map[string]any{"text": "fixture output"}
		body, err := json.Marshal(value)
		if err != nil {
			return nil, err
		}
		digest := blake3.Sum256(body)
		block = map[string]any{"index": 0, "type": "tool_response", "id": call, "name": "read_file", "status": "success", "result": map[string]any{"value": value}, "revision": hex.EncodeToString(digest[:])}
	}
	return json.Marshal(map[string]any{
		"id": fmt.Sprintf("record-%d", sequence), "created": "2026-01-01T00:00:00Z", "turn": turn,
		"provenance": map[string]any{"author": "assistant", "branch": session}, "metadata": map[string]any{},
		"kind": "message", "role": "assistant", "part_count": 1, "content": []any{block}, "sources": map[string]any{},
	})
}

func storageLoadRun(parent context.Context, storage *Store, code *Handle, measurements *storageLoadMeasurements) (storageLoadReport, error) {
	ctx, cancel := context.WithCancel(parent)
	defer cancel()
	start, done := make(chan struct{}), make(chan struct{})
	failures := make(chan error, storageLoadSessions+3)
	var writers, readers sync.WaitGroup
	var codeElapsed time.Duration
	var indexedSymbols int
	for index := 0; index < storageLoadSessions; index++ {
		writers.Add(1)
		go func(index int) {
			defer writers.Done()
			<-start
			session, turn := storageLoadIDs(index)
			for step := 0; step < storageLoadTransactions; step++ {
				began := time.Now()
				err := storage.Node.Write(ctx, func(connection *sql.Conn) error {
					for part := 0; part < storageLoadParts; part++ {
						sequence, err := NextCounter(ctx, connection, CounterAgentEvents)
						if err != nil {
							return err
						}
						body, err := storageLoadRecord(sequence, session, turn, part)
						if err != nil {
							return err
						}
						if err := storageLoadExec(ctx, connection, "INSERT INTO agent_events(sequence,session,id,turn,body) VALUES(?,?,?,?,?)", sequence, session, fmt.Sprintf("record-%d", sequence), turn, body); err != nil {
							return err
						}
						if err := storageLoadExec(ctx, connection, "INSERT INTO conversation_events(session,sequence) VALUES(?,?)", session, sequence); err != nil {
							return err
						}
					}
					return nil
				})
				if err != nil {
					failures <- fmt.Errorf("session %d step %d: %w", index, step, err)
					cancel()
					return
				}
				measurements.writes.Add(storageLoadParts)
				measurements.mu.Lock()
				measurements.operations = append(measurements.operations, time.Since(began))
				measurements.mu.Unlock()
			}
		}(index)
	}
	for index := 0; index < 2; index++ {
		readers.Add(1)
		go func(index int) {
			defer readers.Done()
			<-start
			ticker := time.NewTicker(5 * time.Millisecond)
			defer ticker.Stop()
			for {
				if err := storageLoadRead(ctx, storage, index, measurements); err != nil {
					if ctx.Err() == nil {
						failures <- err
						cancel()
					}
					return
				}
				select {
				case <-done:
					return
				case <-ctx.Done():
					return
				case <-ticker.C:
				}
			}
		}(index)
	}
	readers.Add(1)
	go func() {
		defer readers.Done()
		<-start
		began := time.Now()
		if err := storageLoadRebuildCode(ctx, code); err != nil {
			failures <- fmt.Errorf("code rebuild: %w", err)
			cancel()
		} else {
			indexedSymbols = storageLoadCodeFiles
		}
		codeElapsed = time.Since(began)
	}()
	began := time.Now()
	close(start)
	writers.Wait()
	elapsed := time.Since(began)
	close(done)
	readers.Wait()
	report := storageLoadReport{
		Platform: runtime.GOOS + "/" + runtime.GOARCH, GoVersion: runtime.Version(), Sessions: storageLoadSessions,
		Transactions: storageLoadTransactions, CommittedRecords: measurements.writes.Load(),
		RecordWritesSecond: float64(measurements.writes.Load()) / elapsed.Seconds(),
		CommitP99MS:        storageLoadPercentile(measurements.commits), OperationP99MS: storageLoadPercentile(measurements.operations),
		CommitAttempts: len(measurements.commits), CommitFailures: measurements.commitFailures,
		HistoryReads: measurements.history.Load(), SearchReads: measurements.search.Load(),
		CodeRebuildMS: float64(codeElapsed) / float64(time.Millisecond), IndexedSymbols: indexedSymbols,
		WriterElapsedSeconds: elapsed.Seconds(),
	}
	close(failures)
	for err := range failures {
		return report, err
	}
	err := storage.Node.Read(parent, func(connection *sql.Conn) error {
		if err := storageLoadInteger(parent, connection, "SELECT count(*) FROM agent_events", storageLoadSessions*storageLoadTransactions*storageLoadParts); err != nil {
			return err
		}
		if err := storageLoadInteger(parent, connection, "SELECT (SELECT max(sequence) FROM agent_events)=(SELECT value FROM counters WHERE namespace='agent_events')", 1); err != nil {
			return err
		}
		if err := storageLoadInteger(parent, connection, "SELECT count(*) FROM agent_events e WHERE NOT EXISTS(SELECT 1 FROM conversation_events h WHERE h.sequence=e.sequence AND h.session=e.session)", 0); err != nil {
			return err
		}
		return validateCounters(parent, connection)
	})
	return report, err
}

func storageLoadRead(ctx context.Context, storage *Store, index int, measurements *storageLoadMeasurements) error {
	if index == 1 {
		return storage.Knowledge.Read(ctx, func(connection *sql.Conn) error {
			keys, err := storageLoadKeyword(ctx, connection, "向量", `["selected"]`, "docs/selected/")
			if err != nil {
				return err
			}
			if !slices.Equal(keys, []string{"docs/selected/vector.md"}) {
				return fmt.Errorf("concurrent knowledge query = %v", keys)
			}
			measurements.search.Add(1)
			return nil
		})
	}
	return storage.Node.Read(ctx, func(connection *sql.Conn) error {
		session, _ := storageLoadIDs(int(measurements.history.Load() % storageLoadSessions))
		rows, err := connection.QueryContext(ctx, "SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=? ORDER BY h.sequence DESC LIMIT 50", session)
		if err != nil {
			return err
		}
		defer rows.Close()
		for rows.Next() {
			var body []byte
			if err := rows.Scan(&body); err != nil {
				return err
			}
			if !json.Valid(body) {
				return errors.New("history read an incomplete JSON record")
			}
		}
		if err := rows.Err(); err != nil {
			return err
		}
		measurements.history.Add(1)
		return nil
	})
}

func storageLoadRebuildCode(ctx context.Context, handle *Handle) error {
	for start := 0; start < storageLoadCodeFiles; start += 100 {
		if err := handle.Write(ctx, func(connection *sql.Conn) error {
			for index := start; index < min(start+100, storageLoadCodeFiles); index++ {
				path := fmt.Sprintf("src/fixture_%04d.go", index)
				if err := storageLoadExec(ctx, connection, "INSERT INTO files(path,fingerprint,state) VALUES(?,'fixture','{}')", path); err != nil {
					return err
				}
				body := fmt.Sprintf(`{"name":"Symbol%d","path":%q}`, index, path)
				if err := storageLoadExec(ctx, connection, "INSERT INTO symbols(path,name,language,body,field0,field1,cjk0,cjk1) VALUES(?,?,'go',?,?,?,'','')", path, fmt.Sprintf("Symbol%d", index), body, fmt.Sprintf("symbol%d", index), "rust async cancellation fixture"); err != nil {
					return err
				}
			}
			return nil
		}); err != nil {
			return err
		}
	}
	if err := handle.Write(ctx, func(connection *sql.Conn) error {
		return storageLoadExec(ctx, connection, "INSERT INTO symbols_fts(symbols_fts) VALUES('rebuild')")
	}); err != nil {
		return err
	}
	return handle.Read(ctx, func(connection *sql.Conn) error {
		return storageLoadInteger(ctx, connection, `SELECT count(*) FROM symbols_fts WHERE symbols_fts MATCH '"fixture"'`, storageLoadCodeFiles)
	})
}

func storageLoadInteger(ctx context.Context, connection *sql.Conn, query string, want int64) error {
	var actual int64
	if err := connection.QueryRowContext(ctx, query).Scan(&actual); err != nil {
		return err
	}
	if actual != want {
		return fmt.Errorf("%s returned %d, want %d", query, actual, want)
	}
	return nil
}

func storageLoadExec(ctx context.Context, connection *sql.Conn, query string, arguments ...any) error {
	_, err := connection.ExecContext(ctx, query, arguments...)
	return err
}

func storageLoadPercentile(values []time.Duration) float64 {
	if len(values) == 0 {
		return 0
	}
	ordered := append([]time.Duration(nil), values...)
	sort.Slice(ordered, func(a, b int) bool { return ordered[a] < ordered[b] })
	return float64(ordered[int(math.Ceil(float64(len(ordered))*.99))-1]) / float64(time.Millisecond)
}
