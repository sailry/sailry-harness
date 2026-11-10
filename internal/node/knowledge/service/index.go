// Package service indexes knowledge collections and answers queries and
// citation reads. Its retrieval data is rebuildable; source registrations
// and grants stay in the Node database.
package service

import (
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"math"

	"lukechampine.com/blake3"

	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/node/knowledge/source"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const candidates = 100

// Document identifies one indexed source and what its chunks were built from.
type Document struct {
	Locator, Revision string
	ModifiedMs        uint64
	Tags              []string
	Embedded          bool
}

// Part is one chunk to publish.
type Part struct {
	Title, Text string
	Position    protocol.KnowledgePosition
	Vector      *knowledge.Vector
}

// Binding captures what the chunks of a collection are valid for. An empty
// Model is keyword only.
type Binding struct {
	ID         protocol.KnowledgeID
	Generation uint64
	Model      string
	Dimension  int
	Scope      [32]byte
	Rebuild    uint64
	Paused     bool
}

func newFault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}

func invalid(message string) error { return newFault(protocol.ErrorCodeInvalidRequest, message) }

func code(err error) protocol.ErrorCode {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return fault.Code
	}
	return protocol.ErrorCodeInternal
}

func failure(err error) error {
	var fault *protocol.Fault
	if err == nil || errors.As(err, &fault) || errors.Is(err, context.Canceled) {
		return err
	}
	return newFault(protocol.ErrorCodeInternal, fmt.Sprintf("knowledge index: %v", err))
}

func integer(value uint64) (int64, error) {
	if value > math.MaxInt64 {
		return 0, invalid("knowledge integer exceeds the database range")
	}
	return int64(value), nil
}

func encoded(value any) string {
	data, err := json.Marshal(value)
	if err != nil {
		panic(err)
	}
	return string(data)
}

func tagsJSON(tags []string) string {
	if tags == nil {
		tags = []string{}
	}
	return encoded(tags)
}

func (binding Binding) model() (any, any) {
	if binding.Model == "" {
		return nil, nil
	}
	return binding.Model, int64(binding.Dimension)
}

func changedCollection() error {
	return newFault(protocol.ErrorCodeCancelled, "knowledge collection changed")
}

// Index is the derived retrieval data of every collection.
type Index struct{ db *store.Handle }

func (index Index) read(ctx context.Context, callback func(*sql.Conn) error) error {
	return failure(index.db.Read(ctx, callback))
}

func (index Index) write(ctx context.Context, callback func(*sql.Conn) error) error {
	return failure(index.db.Write(ctx, callback))
}

func current(ctx context.Context, db *sql.Conn, binding Binding) (found bool, err error) {
	generation, err := integer(binding.Generation)
	if err != nil {
		return false, err
	}
	rebuild, err := integer(binding.Rebuild)
	if err != nil {
		return false, err
	}
	model, dimension := binding.model()
	err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM collections WHERE id=?1 AND generation=?2 AND model IS ?3 AND dimension IS ?4 AND scope=?5 AND rebuild=?6)",
		binding.ID.String(), generation, model, dimension, binding.Scope[:], rebuild).Scan(&found)
	return found, err
}

// Bind records what a collection is indexed for. A changed source capture
// or rebuild drops its sources; a changed model only drops their vectors.
func (index Index) Bind(ctx context.Context, binding Binding) error {
	generation, err := integer(binding.Generation)
	if err != nil {
		return err
	}
	rebuild, err := integer(binding.Rebuild)
	if err != nil {
		return err
	}
	model, dimension := binding.model()
	id := binding.ID.String()
	return index.write(ctx, func(db *sql.Conn) error {
		var previousModel sql.NullString
		var previousDimension sql.NullInt64
		var previousScope []byte
		var previousRebuild int64
		err := db.QueryRowContext(ctx, "SELECT model,dimension,scope,rebuild FROM collections WHERE id=?1", id).
			Scan(&previousModel, &previousDimension, &previousScope, &previousRebuild)
		exists := err == nil
		if err != nil && !errors.Is(err, sql.ErrNoRows) {
			return err
		}
		err = nil
		if exists && (previousDimension.Int64 < 0 || previousRebuild < 0) {
			return errors.New("stored dimension or rebuild value is negative")
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO collections(id,generation,model,dimension,scope,rebuild) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET generation=excluded.generation,model=excluded.model,dimension=excluded.dimension,scope=excluded.scope,rebuild=excluded.rebuild",
			id, generation, model, dimension, binding.Scope[:], rebuild); err != nil {
			return err
		}
		switch {
		case !exists:
		case string(previousScope) != string(binding.Scope[:]) || previousRebuild != rebuild:
			_, err = db.ExecContext(ctx, "DELETE FROM sources WHERE collection=?1", id)
		case previousModel.String != binding.Model || previousModel.Valid != (binding.Model != "") ||
			previousDimension.Valid != (binding.Model != "") || previousDimension.Valid && previousDimension.Int64 != int64(binding.Dimension):
			if _, err = db.ExecContext(ctx, "UPDATE chunks SET vector=NULL WHERE source IN (SELECT id FROM sources WHERE collection=?1)", id); err == nil {
				_, err = db.ExecContext(ctx, "UPDATE sources SET embedded=0 WHERE collection=?1", id)
			}
		}
		return err
	})
}

// Unchanged reports a source already published for this binding.
func (index Index) Unchanged(ctx context.Context, binding Binding, document Document) (found bool, err error) {
	modified, err := integer(document.ModifiedMs)
	if err != nil {
		return false, err
	}
	err = index.read(ctx, func(db *sql.Conn) error {
		if found, err = current(ctx, db, binding); err != nil || !found {
			return err
		}
		return db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM sources s WHERE s.collection=?1 AND s.locator=?2 AND s.revision=?3 AND s.modified_ms=?4 AND s.tags=?5 AND (?6=0 OR s.embedded=1))",
			binding.ID.String(), document.Locator, document.Revision, modified, tagsJSON(document.Tags), document.Embedded).Scan(&found)
	})
	return found, err
}

// chunkKey is the public identity of a chunk. It changes with the source
// revision, the chunk boundaries and the text.
func chunkKey(binding Binding, document Document, ordinal int, part Part) string {
	text := blake3.Sum256([]byte(part.Text))
	key := blake3.Sum256([]byte(encoded([]any{binding.ID, document.Locator, document.Revision, ordinal, part.Position, hex.EncodeToString(text[:])})))
	return hex.EncodeToString(key[:])
}

// Replace publishes one verified source atomically.
func (index Index) Replace(ctx context.Context, binding Binding, document Document, parts []Part) error {
	modified, err := integer(document.ModifiedMs)
	if err != nil {
		return err
	}
	return index.write(ctx, func(db *sql.Conn) error {
		found, err := current(ctx, db, binding)
		if err != nil {
			return err
		}
		if !found {
			return changedCollection()
		}
		var source int64
		if err := db.QueryRowContext(ctx, "INSERT INTO sources(collection,locator,revision,modified_ms,tags,embedded) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(collection,locator) DO UPDATE SET revision=excluded.revision,modified_ms=excluded.modified_ms,tags=excluded.tags,embedded=excluded.embedded RETURNING id",
			binding.ID.String(), document.Locator, document.Revision, modified, tagsJSON(document.Tags), document.Embedded).Scan(&source); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "DELETE FROM file_stamps WHERE source=?1", source); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "DELETE FROM chunks WHERE source=?1", source); err != nil {
			return err
		}
		for ordinal, part := range parts {
			var vector any
			if part.Vector != nil {
				if binding.Model == "" {
					return invalid("knowledge vector requires a captured model")
				}
				bytes := part.Vector.Encode()
				if _, err := knowledge.DecodeVector(bytes, binding.Dimension); err != nil {
					return err
				}
				vector = bytes
			}
			if _, err := db.ExecContext(ctx, "INSERT INTO chunks(key,source,ordinal,title,text,field0,field1,cjk0,cjk1,position,vector) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
				chunkKey(binding, document, ordinal, part), source, ordinal, part.Title, part.Text,
				knowledge.Field(part.Title), knowledge.Field(part.Text), knowledge.Index(part.Title), knowledge.Index(part.Text),
				encoded(part.Position), vector); err != nil {
				return err
			}
		}
		return nil
	})
}

// Prune drops the sources a completed scan no longer found.
func (index Index) Prune(ctx context.Context, binding Binding, locators []string) error {
	return index.write(ctx, func(db *sql.Conn) error {
		found, err := current(ctx, db, binding)
		if err != nil {
			return err
		}
		if !found {
			return changedCollection()
		}
		_, err = db.ExecContext(ctx, "DELETE FROM sources WHERE collection=?1 AND locator NOT IN (SELECT value FROM json_each(?2))", binding.ID.String(), tagsJSON(locators))
		return err
	})
}

// Retain drops the collections that no longer exist.
func (index Index) Retain(ctx context.Context, ids []protocol.KnowledgeID) error {
	if ids == nil {
		ids = []protocol.KnowledgeID{}
	}
	return index.write(ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(ctx, "DELETE FROM collections WHERE id NOT IN (SELECT value FROM json_each(?1))", encoded(ids))
		return err
	})
}

// Counts returns the sources and chunks published for this binding.
func (index Index) Counts(ctx context.Context, binding Binding) (sources, chunks uint64, err error) {
	err = index.read(ctx, func(db *sql.Conn) error {
		found, err := current(ctx, db, binding)
		if err != nil || !found {
			return err
		}
		return db.QueryRowContext(ctx, "SELECT (SELECT count(*) FROM sources WHERE collection=?1),(SELECT count(*) FROM chunks c JOIN sources s ON s.id=c.source WHERE s.collection=?1)", binding.ID.String()).Scan(&sources, &chunks)
	})
	return sources, chunks, err
}

// Cached reports a file whose metadata witness matches its published
// source. Ordinary writers are witnessed by size and time; citation reads
// still hash the content.
func (index Index) Cached(ctx context.Context, binding Binding, locator string, stat source.Stat, tags []string, embedded bool) (found bool, err error) {
	size, err := integer(stat.Size)
	if err != nil {
		return false, err
	}
	seconds, err := integer(stat.ModifiedSeconds)
	if err != nil {
		return false, err
	}
	err = index.read(ctx, func(db *sql.Conn) error {
		if found, err = current(ctx, db, binding); err != nil || !found {
			return err
		}
		return db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM sources s JOIN file_stamps f ON f.source=s.id WHERE s.collection=?1 AND s.locator=?2 AND f.size=?3 AND f.modified_seconds=?4 AND f.modified_nanos=?5 AND s.tags=?6 AND (?7=0 OR s.embedded=1) AND f.extractor=?8)",
			binding.ID.String(), locator, size, seconds, int64(stat.ModifiedNanos), tagsJSON(tags), embedded, stat.Extractor[:]).Scan(&found)
	})
	return found, err
}

// Remember keeps the metadata witness of a published file.
func (index Index) Remember(ctx context.Context, binding Binding, locator string, stat source.Stat) error {
	size, err := integer(stat.Size)
	if err != nil {
		return err
	}
	seconds, err := integer(stat.ModifiedSeconds)
	if err != nil {
		return err
	}
	return index.write(ctx, func(db *sql.Conn) error {
		found, err := current(ctx, db, binding)
		if err != nil {
			return err
		}
		if !found {
			return changedCollection()
		}
		_, err = db.ExecContext(ctx, "INSERT INTO file_stamps(source,size,modified_seconds,modified_nanos,extractor) SELECT id,?3,?4,?5,?6 FROM sources WHERE collection=?1 AND locator=?2 ON CONFLICT(source) DO UPDATE SET size=excluded.size,modified_seconds=excluded.modified_seconds,modified_nanos=excluded.modified_nanos,extractor=excluded.extractor",
			binding.ID.String(), locator, size, seconds, int64(stat.ModifiedNanos), stat.Extractor[:])
		return err
	})
}

// record is an indexed citation and the title of its chunk.
type record struct {
	citation protocol.KnowledgeCitation
	title    string
}

func readable(ctx context.Context, db *sql.Conn, binding Binding) (found bool, err error) {
	if !binding.Paused {
		return current(ctx, db, binding)
	}
	generation, err := integer(binding.Generation)
	if err != nil {
		return false, err
	}
	rebuild, err := integer(binding.Rebuild)
	if err != nil {
		return false, err
	}
	err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM collections WHERE id=?1 AND generation<=?2 AND scope=?3 AND rebuild=?4)",
		binding.ID.String(), generation, binding.Scope[:], rebuild).Scan(&found)
	return found, err
}

func changedSource() error {
	return newFault(protocol.ErrorCodeRevisionConflict, "knowledge source changed; refresh its results")
}

// citation resolves a cited chunk against the binding it was indexed for.
func (index Index) citation(ctx context.Context, binding Binding, input protocol.KnowledgeCitationRequest) (found record, err error) {
	err = index.read(ctx, func(db *sql.Conn) error {
		ok, err := readable(ctx, db, binding)
		if err != nil {
			return err
		}
		if binding.ID != input.Collection || !ok {
			return newFault(protocol.ErrorCodeRevisionConflict, "knowledge collection changed; refresh its results")
		}
		var position string
		citation := protocol.KnowledgeCitation{Collection: input.Collection, Chunk: input.Chunk}
		err = db.QueryRowContext(ctx, "SELECT c.title,s.locator,s.revision,c.position FROM chunks c JOIN sources s ON s.id=c.source WHERE s.collection=?1 AND c.key=?2",
			input.Collection.String(), input.Chunk).Scan(&found.title, &citation.Path, &citation.Revision, &position)
		if errors.Is(err, sql.ErrNoRows) {
			return newFault(protocol.ErrorCodeNotFound, "knowledge citation is no longer indexed")
		}
		if err != nil {
			return err
		}
		if citation.Revision != input.Revision {
			return changedSource()
		}
		if err := json.Unmarshal([]byte(position), &citation.Position); err != nil {
			return err
		}
		citation.Link = knowledge.CitationURL(citation)
		found.citation = citation
		return nil
	})
	return found, err
}
