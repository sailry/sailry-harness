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
	batchSymbols = 512
	batchRows    = 256
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
		// A separate allocation lets the parsed file and its syntax data be reclaimed.
		value.Language = new(file.Language)
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
type index struct{ handle indexStorage }

type indexStorage interface {
	Read(context.Context, func(*sql.Conn) error) error
	Write(context.Context, func(*sql.Conn) error) error
	Close() error
}

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

type writeMode uint8

const (
	replaceFiles writeMode = iota
	appendSearchFiles
	appendGraphFiles
)

// replace commits the files' records, symbols, references and imports in one
// transaction. It reports the committed revision, or false without files.
func (i *index) replace(ctx context.Context, files []Parsed, fingerprint string) (uint64, bool, error) {
	return i.writeFiles(ctx, files, fingerprint, replaceFiles)
}

func (i *index) writeFiles(ctx context.Context, files []Parsed, fingerprint string, mode writeMode) (uint64, bool, error) {
	if len(files) == 0 {
		return 0, false, nil
	}
	const insertFile = "INSERT INTO files(path,fingerprint,hash,state) VALUES(?1,?2,?3,?4)"
	statement := insertFile + " RETURNING id"
	if mode == replaceFiles {
		statement = insertFile + " ON CONFLICT(path) DO UPDATE SET fingerprint=excluded.fingerprint,hash=excluded.hash,state=excluded.state RETURNING id"
	}
	var committed uint64
	err := i.handle.Write(ctx, func(db *sql.Conn) error {
		ids := make([]int64, len(files))
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
			if err := db.QueryRowContext(ctx, statement, file.Path, fingerprint, hash, string(body)).Scan(&ids[index]); err != nil {
				return err
			}
			if mode == replaceFiles {
				if err := clearFile(ctx, db, ids[index]); err != nil {
					return err
				}
			}
		}
		references := batch{prefix: "INSERT INTO refs(file_id,name,kind,start_line,start_column,start_byte,receiver,scope,scope_kind,scope_line,scope_end_line) VALUES ", row: "(?,?,?,?,?,?,?,?,?,?,?)"}
		for index := range files {
			file := &files[index]
			for _, usage := range file.References {
				var scope, kind, start, end any
				if usage.Scope != nil {
					scope, kind, start, end = usage.Scope.Name, usage.Scope.Kind, usage.Scope.StartLine, usage.Scope.EndLine
				}
				if err := references.push(ctx, db, batchRows, ids[index], usage.Name, usage.Kind, usage.Line, usage.Column, usage.Start, usage.Receiver, scope, kind, start, end); err != nil {
					return err
				}
			}
		}
		if err := references.flush(ctx, db); err != nil {
			return err
		}
		modules := batch{prefix: "INSERT INTO imports(file_id,module) VALUES ", row: "(?,?)"}
		for index := range files {
			for _, module := range files[index].Imports {
				if err := modules.push(ctx, db, batchRows, ids[index], module); err != nil {
					return err
				}
			}
		}
		if err := modules.flush(ctx, db); err != nil {
			return err
		}
		symbols := batch{prefix: "INSERT INTO symbols(file_id,name,language,kind,start_byte,end_byte,start_line,start_column,end_line,end_column,signature,depth,field0,field1,cjk0,cjk1) VALUES ", row: "(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)"}
		for index := range files {
			file := &files[index]
			for _, symbol := range file.Symbols {
				var fieldName, fieldTerms, cjkName, cjkTerms string
				if mode != appendGraphFiles {
					terms := file.Path + " "
					if symbol.Signature != nil {
						terms += *symbol.Signature
					}
					fieldName, fieldTerms = knowledge.Field(symbol.Name), knowledge.Field(terms)
					cjkName, cjkTerms = knowledge.Index(symbol.Name), knowledge.Index(terms)
				}
				span := symbol.Range
				if err := symbols.push(ctx, db, batchSymbols, ids[index], symbol.Name, symbol.Language, symbol.Kind,
					span.StartByte, span.EndByte, span.StartLine, span.StartColumn, span.EndLine, span.EndColumn, optional(symbol.Signature), symbol.Depth,
					fieldName, fieldTerms, cjkName, cjkTerms); err != nil {
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

func clearFile(ctx context.Context, db *sql.Conn, id int64) error {
	for _, statement := range []string{"DELETE FROM symbols WHERE file_id=?1", "DELETE FROM refs WHERE file_id=?1", "DELETE FROM imports WHERE file_id=?1"} {
		if _, err := db.ExecContext(ctx, statement, id); err != nil {
			return err
		}
	}
	return nil
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
			var id int64
			if err := db.QueryRowContext(ctx, "SELECT id FROM files WHERE path=?1", path).Scan(&id); err != nil {
				if errors.Is(err, sql.ErrNoRows) {
					continue
				}
				return err
			}
			if err := clearFile(ctx, db, id); err != nil {
				return err
			}
			if _, err := db.ExecContext(ctx, "DELETE FROM files WHERE id=?1", id); err != nil {
				return err
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

const symbolSelect = "SELECT f.path,s.name,s.language,s.kind,s.start_byte,s.end_byte,s.start_line,s.start_column,s.end_line,s.end_column,s.signature,s.depth,coalesce(f.hash,'')"
const symbolJoin = " FROM symbols s JOIN files f ON f.id=s.file_id"

func scanSymbol(rows *sql.Rows) (Symbol, error) {
	var symbol Symbol
	span := &symbol.Range
	err := rows.Scan(&symbol.Path, &symbol.Name, &symbol.Language, &symbol.Kind,
		&span.StartByte, &span.EndByte, &span.StartLine, &span.StartColumn, &span.EndLine, &span.EndColumn,
		&symbol.Signature, &symbol.Depth, &symbol.FileRevision)
	return symbol, err
}

func symbols(rows *sql.Rows) ([]Symbol, error) {
	defer rows.Close()
	found := []Symbol{}
	for rows.Next() {
		symbol, err := scanSymbol(rows)
		if err != nil {
			return nil, err
		}
		found = append(found, symbol)
	}
	return found, rows.Err()
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
	searchFilter = "(?1 IS NULL OR instr(lower(s.name),lower(?1))>0) AND (?2 IS NULL OR f.path=?2 OR substr(f.path,1,length(?2)+1)=?2||'/') AND (?3 IS NULL OR s.language=?3)"
	// Exact names rank before prefixes and other hits; declarations outside
	// test paths rank before tests inside each group. The score breaks ties.
	searchOrder = "CASE WHEN ?5 IS NULL THEN 2 WHEN lower(s.name)=lower(?5) THEN 0 WHEN substr(lower(s.name),1,length(?5))=lower(?5) THEN 1 ELSE 2 END,CASE WHEN instr('/'||f.path,'/tests/')>0 OR instr('/'||f.path||'/','/tests.rs/')>0 OR instr(f.path,'_test')>0 OR instr('/'||f.path,'/test_')>0 THEN 1 ELSE 0 END"
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
	statement := symbolSelect + symbolJoin + " WHERE " + searchFilter + " ORDER BY " + searchOrder + ",f.path,s.id LIMIT ?4"
	if terms != "" {
		// Scores are materialized before the joins so every filter applies
		// to the ranked candidates.
		statement = "WITH candidates AS MATERIALIZED (SELECT id,fts_score(field0,field1,cjk0,cjk1,?6) AS score FROM symbols WHERE fts_match(field0,field1,cjk0,cjk1,?6)) " + symbolSelect + symbolJoin + " JOIN candidates c ON c.id=s.id WHERE " + searchFilter + " ORDER BY " + searchOrder + ",c.score DESC,f.path,s.id LIMIT ?4"
		expression := "field0:(" + terms + ") OR field1:(" + terms + ") OR cjk0:(" + terms + ") OR cjk1:(" + terms + ")"
		values = append(values, expression)
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
