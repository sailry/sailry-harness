package codeindex

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	// batchSymbols bounds one multi-row symbol insert and one writer transaction.
	batchSymbols  = 512
	batchRows     = 256
	symbolColumns = 8
)

// state is the JSON kept in files.state.
type state struct {
	Language     *string      `json:"language"`
	Status       Status       `json:"status"`
	Capabilities Capabilities `json:"capabilities"`
	Partial      bool         `json:"partial"`
	Truncated    bool         `json:"truncated"`
}

func stateOf(file *Parsed) state {
	value := state{Status: file.Status, Capabilities: file.Capabilities, Partial: file.Partial, Truncated: file.Truncated}
	if file.Language != "" {
		value.Language = &file.Language
	}
	return value
}

// cached is one file record: content hash, extraction rules and outcome.
type cached struct {
	hash        *string
	fingerprint string
	state       state
}

// index is one worktree's derived database.
type index struct{ handle *store.Handle }

func openIndex(ctx context.Context, path string) (*index, error) {
	handle, err := store.OpenCodeIndex(ctx, path)
	if err != nil {
		return nil, failure(err)
	}
	return &index{handle: handle}, nil
}

func (i *index) close() error { return failure(i.handle.Close()) }

func revision(ctx context.Context, db *sql.Conn) (uint64, error) {
	var value uint64
	err := db.QueryRowContext(ctx, "SELECT revision FROM metadata WHERE singleton=1").Scan(&value)
	return value, err
}

func (i *index) revision(ctx context.Context) (value uint64, err error) {
	err = i.handle.Read(ctx, func(db *sql.Conn) error {
		value, err = revision(ctx, db)
		return err
	})
	return value, failure(err)
}

// snapshot loads every file record with the revision it reflects.
func (i *index) snapshot(ctx context.Context) (uint64, map[string]cached, error) {
	var current uint64
	entries := map[string]cached{}
	err := i.handle.Read(ctx, func(db *sql.Conn) error {
		rows, err := db.QueryContext(ctx, "SELECT path,fingerprint,hash,state FROM files")
		if err != nil {
			return err
		}
		defer rows.Close()
		for rows.Next() {
			var path, body string
			var entry cached
			if err := rows.Scan(&path, &entry.fingerprint, &entry.hash, &body); err != nil {
				return err
			}
			if err := json.Unmarshal([]byte(body), &entry.state); err != nil {
				return err
			}
			entries[path] = entry
		}
		if err := rows.Err(); err != nil {
			return err
		}
		current, err = revision(ctx, db)
		return err
	})
	return current, entries, failure(err)
}

// batch is a bounded multi-row insert.
type batch struct {
	prefix, row string
	sql         strings.Builder
	values      []any
	rows        int
}

func (b *batch) push(ctx context.Context, db *sql.Conn, limit int, values ...any) error {
	if b.rows == 0 {
		b.sql.WriteString(b.prefix)
	} else {
		b.sql.WriteByte(',')
	}
	b.sql.WriteString(b.row)
	b.values = append(b.values, values...)
	b.rows++
	if b.rows == limit {
		return b.flush(ctx, db)
	}
	return nil
}

func (b *batch) flush(ctx context.Context, db *sql.Conn) error {
	if b.rows == 0 {
		return nil
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	_, err := db.ExecContext(ctx, b.sql.String(), b.values...)
	b.sql.Reset()
	b.values, b.rows = b.values[:0], 0
	return err
}

// replace commits the files' records, symbols, references and imports in one
// transaction. It reports the committed revision, or false without files.
func (i *index) replace(ctx context.Context, files []Parsed, fingerprint string) (uint64, bool, error) {
	if len(files) == 0 {
		return 0, false, nil
	}
	var committed uint64
	err := i.handle.Write(ctx, func(db *sql.Conn) error {
		for index := range files {
			file := &files[index]
			if err := ctx.Err(); err != nil {
				return err
			}
			body, err := json.Marshal(stateOf(file))
			if err != nil {
				return err
			}
			var hash any
			if file.Hash != "" {
				hash = file.Hash
			}
			if _, err := db.ExecContext(ctx, "INSERT INTO files(path,fingerprint,hash,state) VALUES(?1,?2,?3,?4) ON CONFLICT(path) DO UPDATE SET fingerprint=excluded.fingerprint,hash=excluded.hash,state=excluded.state", file.Path, fingerprint, hash, string(body)); err != nil {
				return err
			}
			for _, statement := range []string{"DELETE FROM symbols WHERE path=?1", "DELETE FROM refs WHERE path=?1", "DELETE FROM imports WHERE path=?1"} {
				if _, err := db.ExecContext(ctx, statement, file.Path); err != nil {
					return err
				}
			}
		}
		references := batch{prefix: "INSERT INTO refs(path,name,kind,start_line,start_column,start_byte,receiver,scope,scope_kind,scope_line,scope_end_line) VALUES ", row: "(?,?,?,?,?,?,?,?,?,?,?)"}
		for index := range files {
			file := &files[index]
			for _, usage := range file.References {
				var scope, kind, start, end any
				if usage.Scope != nil {
					scope, kind, start, end = usage.Scope.Name, usage.Scope.Kind, usage.Scope.StartLine, usage.Scope.EndLine
				}
				if err := references.push(ctx, db, batchRows, file.Path, usage.Name, usage.Kind, usage.Line, usage.Column, usage.Start, usage.Receiver, scope, kind, start, end); err != nil {
					return err
				}
			}
		}
		if err := references.flush(ctx, db); err != nil {
			return err
		}
		modules := batch{prefix: "INSERT INTO imports(path,module) VALUES ", row: "(?,?)"}
		for index := range files {
			for _, module := range files[index].Imports {
				if err := modules.push(ctx, db, batchRows, files[index].Path, module); err != nil {
					return err
				}
			}
		}
		if err := modules.flush(ctx, db); err != nil {
			return err
		}
		symbols := batch{prefix: "INSERT INTO symbols(path,name,language,body,field0,field1,cjk0,cjk1) VALUES ", row: "(?,?,?,?,?,?,?,?)"}
		for index := range files {
			file := &files[index]
			for _, symbol := range file.Symbols {
				body, err := json.Marshal(symbol)
				if err != nil {
					return err
				}
				context := file.Path + " "
				if symbol.Signature != nil {
					context += *symbol.Signature
				}
				if err := symbols.push(ctx, db, batchSymbols, file.Path, symbol.Name, symbol.Language, string(body), knowledge.Field(symbol.Name), knowledge.Field(context), knowledge.Index(symbol.Name), knowledge.Index(context)); err != nil {
					return err
				}
			}
		}
		if err := symbols.flush(ctx, db); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "UPDATE metadata SET revision=revision+1 WHERE singleton=1"); err != nil {
			return err
		}
		var err error
		committed, err = revision(ctx, db)
		return err
	})
	return committed, err == nil, failure(err)
}

// remove deletes records proven absent by a complete traversal in one
// transaction and returns the committed revision.
func (i *index) remove(ctx context.Context, paths []string) (uint64, error) {
	var committed uint64
	err := i.handle.Write(ctx, func(db *sql.Conn) error {
		for _, path := range paths {
			if err := ctx.Err(); err != nil {
				return err
			}
			for _, statement := range []string{"DELETE FROM symbols WHERE path=?1", "DELETE FROM refs WHERE path=?1", "DELETE FROM imports WHERE path=?1", "DELETE FROM files WHERE path=?1"} {
				if _, err := db.ExecContext(ctx, statement, path); err != nil {
					return err
				}
			}
		}
		if _, err := db.ExecContext(ctx, "UPDATE metadata SET revision=revision+1 WHERE singleton=1"); err != nil {
			return err
		}
		var err error
		committed, err = revision(ctx, db)
		return err
	})
	return committed, failure(err)
}

func symbols(rows *sql.Rows) ([]Symbol, error) {
	defer rows.Close()
	found := []Symbol{}
	for rows.Next() {
		var body string
		if err := rows.Scan(&body); err != nil {
			return nil, err
		}
		var symbol Symbol
		if err := json.Unmarshal([]byte(body), &symbol); err != nil {
			return nil, err
		}
		found = append(found, symbol)
	}
	return found, rows.Err()
}

// outline returns one indexed file's declarations in source order.
func (i *index) outline(ctx context.Context, path string) (protocol.CodeIndexOutline, error) {
	outline := protocol.CodeIndexOutline{Path: path}
	err := i.handle.Read(ctx, func(db *sql.Conn) error {
		var body string
		if err := db.QueryRowContext(ctx, "SELECT hash,state FROM files WHERE path=?1", path).Scan(&outline.FileRevision, &body); err != nil {
			return err
		}
		var state state
		if err := json.Unmarshal([]byte(body), &state); err != nil {
			return err
		}
		rows, err := db.QueryContext(ctx, "SELECT body FROM symbols WHERE path=?1 ORDER BY id LIMIT ?2", path, maxOutlineSymbols+1)
		if err != nil {
			return err
		}
		if outline.Symbols, err = symbols(rows); err != nil {
			return err
		}
		outline.Truncated = state.Truncated || len(outline.Symbols) > maxOutlineSymbols
		outline.Symbols = outline.Symbols[:min(len(outline.Symbols), maxOutlineSymbols)]
		outline.Language, outline.Status, outline.Capabilities, outline.Partial = state.Language, state.Status, state.Capabilities, state.Partial
		outline.IndexRevision, err = revision(ctx, db)
		return err
	})
	return outline, failure(err)
}

// graph runs one graph view against a single read transaction.
func (i *index) graph(ctx context.Context, query *protocol.CodeIndexGraph) (protocol.CodeIndexGraphGraphPage, error) {
	var page protocol.CodeIndexGraphGraphPage
	err := i.handle.Read(ctx, func(db *sql.Conn) error {
		reader := &reader{ctx: ctx, db: db}
		var err error
		if page, err = run(reader, query); err != nil {
			return err
		}
		page.Partial = page.Partial || reader.limited
		page.Truncated = page.Truncated || reader.limited
		page.IndexRevision, err = revision(ctx, db)
		return err
	})
	return page, failure(err)
}

const (
	searchFilter = "(?1 IS NULL OR instr(lower(s.name),lower(?1))>0) AND (?2 IS NULL OR s.path=?2 OR substr(s.path,1,length(?2)+1)=?2||'/') AND (?3 IS NULL OR s.language=?3)"
	// Exact names rank before prefixes and other hits; declarations outside
	// test paths rank before tests inside each group. The score breaks ties.
	searchOrder = "CASE WHEN ?5 IS NULL THEN 2 WHEN lower(s.name)=lower(?5) THEN 0 WHEN substr(lower(s.name),1,length(?5))=lower(?5) THEN 1 ELSE 2 END,CASE WHEN instr('/'||s.path,'/tests/')>0 OR instr('/'||s.path||'/','/tests.rs/')>0 OR instr(s.path,'_test')>0 OR instr('/'||s.path,'/test_')>0 THEN 1 ELSE 0 END"
)

func optional(value *string) any {
	if value == nil {
		return nil
	}
	return *value
}

// search ranks indexed declarations by name and full-text terms.
func (i *index) search(ctx context.Context, query *protocol.CodeIndexSearch) (protocol.CodeIndexSearchPage, error) {
	page := protocol.CodeIndexSearchPage{Issues: []protocol.CodeIndexIssue{}}
	terms := knowledge.Query(query.Query)
	limit := int(min(query.Limit, maxSearchSymbols))
	var wanted any
	if name := strings.TrimSpace(deref(query.Name)); name != "" {
		wanted = name
	} else if text := strings.TrimSpace(query.Query); text != "" {
		wanted = text
	}
	values := []any{optional(query.Name), optional(query.Path), optional(query.Language), limit + 1, wanted}
	statement := "SELECT s.body FROM symbols s WHERE " + searchFilter + " ORDER BY " + searchOrder + ",s.path,s.id LIMIT ?4"
	if terms != "" {
		// Scores are materialized before the joins so every filter applies
		// to the ranked candidates.
		statement = "WITH candidates AS MATERIALIZED (SELECT rowid AS id,-bm25(symbols_fts) AS score FROM symbols_fts WHERE symbols_fts MATCH ?6) SELECT s.body FROM candidates f JOIN symbols s ON s.id=f.id WHERE " + searchFilter + " ORDER BY " + searchOrder + ",f.score DESC,s.path,s.id LIMIT ?4"
		values = append(values, terms)
	}
	err := i.handle.Read(ctx, func(db *sql.Conn) error {
		rows, err := db.QueryContext(ctx, statement, values...)
		if err != nil {
			return err
		}
		if page.Symbols, err = symbols(rows); err != nil {
			return err
		}
		page.Truncated = len(page.Symbols) > limit
		page.Symbols = page.Symbols[:min(len(page.Symbols), limit)]
		page.IndexRevision, err = revision(ctx, db)
		return err
	})
	return page, failure(err)
}

func deref(value *string) string {
	if value == nil {
		return ""
	}
	return *value
}

func fault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}

func cancelled() error { return fault(protocol.ErrorCodeCancelled, "code query was cancelled") }

// failure keeps faults and reports any other storage or parsing error as
// an internal code index fault.
func failure(err error) error {
	var known *protocol.Fault
	switch {
	case err == nil:
		return nil
	case errors.As(err, &known):
		return err
	case errors.Is(err, context.Canceled):
		return cancelled()
	case errors.Is(err, context.DeadlineExceeded):
		return fault(protocol.ErrorCodeBusy, "code query deadline exceeded")
	}
	return fault(protocol.ErrorCodeInternal, "code index: "+err.Error())
}

func code(err error) protocol.ErrorCode {
	var known *protocol.Fault
	if errors.As(err, &known) {
		return known.Code
	}
	return protocol.ErrorCodeInternal
}
