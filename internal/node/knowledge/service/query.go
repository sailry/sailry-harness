package service

import (
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"math"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Embedding is one model's vector of the query text. It is compared only
// with vectors built by the same model and identity (Binding).
type Embedding struct {
	Model    string
	Identity string
	Values   []float32
}

// filter admits only chunks of captured grants and applies source filters
// before any candidate limit. A paused grant keeps its cached keyword text.
const (
	granted = "JOIN json_each(?1) admitted ON json_extract(admitted.value,'$.id')=k.id AND json_extract(admitted.value,'$.scope')=lower(hex(k.scope)) AND json_extract(admitted.value,'$.rebuild')=k.rebuild AND (json_extract(admitted.value,'$.generation')=k.generation OR (json_extract(admitted.value,'$.semantic')=0 AND k.generation<=json_extract(admitted.value,'$.generation')))"
	sourced = "NOT EXISTS(SELECT 1 FROM json_each(?2) wanted WHERE NOT EXISTS(SELECT 1 FROM json_each(s.tags) actual WHERE actual.value=wanted.value)) AND (?3 IS NULL OR substr(s.locator,1,length(?3))=?3) AND (?4 IS NULL OR s.modified_ms>=?4) AND (?5 IS NULL OR s.modified_ms<=?5) AND (json_array_length(?6)=0 OR EXISTS(SELECT 1 FROM json_each(?6) wanted JOIN json_each(s.tags) actual ON actual.value=wanted.value))"
	filter  = "JOIN sources s ON s.id=c.source JOIN collections k ON k.id=s.collection " + granted + " WHERE " + sourced
	// keywords ranks the chunks that match the query's words. The index
	// scores every match, and the scores are materialized before the joins so
	// that every captured grant and source filter applies before the
	// candidate limit. The grants and filters are evaluated once per source,
	// not once per matching chunk.
	keywords = "WITH candidates AS MATERIALIZED (SELECT rowid AS id,-bm25(chunks_fts,2,1,2,1) AS score FROM chunks_fts WHERE chunks_fts MATCH ?7), admitted_sources AS MATERIALIZED (SELECT s.id FROM sources s JOIN collections k ON k.id=s.collection " + granted + " WHERE " + sourced + ") SELECT c.key,f.score FROM candidates f JOIN chunks c ON c.id=f.id JOIN admitted_sources a ON a.id=c.source ORDER BY f.score DESC,c.key LIMIT 100"
)

func searchCancelled(ctx context.Context) error {
	if ctx.Err() != nil {
		return newFault(protocol.ErrorCodeCancelled, "knowledge search was cancelled")
	}
	return nil
}

// Search ranks the chunks the bindings admit and returns one page.
func (index Index) Search(ctx context.Context, query protocol.KnowledgeQuery, allowed []Binding, embeddings []Embedding, indexing bool) (protocol.KnowledgePage, error) {
	page := protocol.DefaultKnowledgePage()
	stamps := index.vectors.snapshot(allowed)
	err := index.read(ctx, func(db *sql.Conn) error {
		ranked, mode, err := index.ranks(ctx, db, stamps, query, allowed, embeddings, nil)
		if err != nil {
			return err
		}
		offset := int(min(uint64(query.Offset), uint64(len(ranked))))
		limit := int(min(query.Limit, knowledge.PageLimit))
		if offset+limit < len(ranked) && uint64(query.Offset)+uint64(limit) <= math.MaxUint32 {
			next := query.Offset + uint32(limit)
			page.NextOffset = &next
		}
		hits, err := hits(ctx, db, ranked[offset:min(offset+limit, len(ranked))])
		if err != nil {
			return err
		}
		for position := range hits {
			hits[position].Stale = indexing
		}
		page.Hits, page.Mode, page.Indexing, page.Untrusted = hits, mode, indexing, true
		if mode != protocol.KnowledgeModeKeyword && len(embeddings) == 1 {
			page.Model = &embeddings[0].Model
		}
		return nil
	})
	return page, err
}

// Candidates returns the nearest chunks of a query by meaning alone, for a
// caller that fuses them with its own keyword ranking. A source needs every
// query tag and, when any are given, one of the alternatives. Without a
// usable vector there are none.
func (index Index) Candidates(ctx context.Context, query protocol.KnowledgeQuery, allowed []Binding, embeddings []Embedding, alternatives []string) ([]protocol.KnowledgeHit, error) {
	var found []protocol.KnowledgeHit
	query.Mode = protocol.KnowledgeModeSemantic
	stamps := index.vectors.snapshot(allowed)
	err := index.read(ctx, func(db *sql.Conn) error {
		ranked, mode, err := index.ranks(ctx, db, stamps, query, allowed, embeddings, alternatives)
		if err != nil || mode == protocol.KnowledgeModeKeyword {
			return err
		}
		found, err = hits(ctx, db, ranked[:min(len(ranked), candidates)])
		return err
	})
	return found, err
}

// ranks orders the admitted chunks. The stamps are those of the collections
// before the read transaction of db began.
func (index Index) ranks(ctx context.Context, db *sql.Conn, stamps map[string]stamp, query protocol.KnowledgeQuery, allowed []Binding, embeddings []Embedding, alternatives []string) ([]knowledge.Ranked, protocol.KnowledgeMode, error) {
	if err := knowledge.ValidateQuery(query); err != nil {
		return nil, "", err
	}
	if err := searchCancelled(ctx); err != nil {
		return nil, "", err
	}
	admitted := []map[string]any{}
	for _, binding := range allowed {
		if len(query.Collections) != 0 && !slices.Contains(query.Collections, binding.ID) {
			continue
		}
		generation, err := integer(binding.Generation)
		if err != nil {
			return nil, "", err
		}
		rebuild, err := integer(binding.Rebuild)
		if err != nil {
			return nil, "", err
		}
		admitted = append(admitted, map[string]any{"id": binding.ID, "generation": generation, "scope": hex.EncodeToString(binding.Scope[:]), "rebuild": rebuild, "semantic": !binding.Paused})
	}
	var after, before any
	if query.ModifiedAfterMs != nil {
		value, err := integer(*query.ModifiedAfterMs)
		if err != nil {
			return nil, "", err
		}
		after = value
	}
	if query.ModifiedBeforeMs != nil {
		value, err := integer(*query.ModifiedBeforeMs)
		if err != nil {
			return nil, "", err
		}
		before = value
	}
	var path any
	if query.Path != nil {
		path = *query.Path
	}
	grants, tags, choices := encoded(admitted), tagsJSON(query.Tags), tagsJSON(alternatives)
	scores := map[string]float32{}
	var keyword, semantic []string
	if terms := knowledge.Query(query.Query); terms != "" {
		rows, err := db.QueryContext(ctx, keywords, grants, tags, path, after, before, choices, terms)
		if err != nil {
			return nil, "", err
		}
		defer rows.Close()
		for rows.Next() {
			if err := searchCancelled(ctx); err != nil {
				return nil, "", err
			}
			var key string
			var score float64
			if err := rows.Scan(&key, &score); err != nil {
				return nil, "", err
			}
			keyword = append(keyword, key)
			scores[key] = float32(score)
		}
		if err := rows.Close(); err != nil {
			return nil, "", err
		}
	}
	var nearest []knowledge.Ranked
	if query.Mode != protocol.KnowledgeModeKeyword {
		filtered := len(query.Tags) != 0 || path != nil || after != nil || before != nil || len(alternatives) != 0
		for _, embedding := range embeddings {
			found, err := index.nearest(ctx, db, stamps, embedding, filtered, admitted, []any{tags, path, after, before, choices})
			if err != nil {
				return nil, "", err
			}
			nearest = append(nearest, found...)
		}
	}
	slices.SortFunc(nearest, order)
	nearest = nearest[:min(len(nearest), candidates)]
	for _, candidate := range nearest {
		semantic = append(semantic, candidate.Key)
	}
	mode := query.Mode
	if len(semantic) == 0 {
		mode = protocol.KnowledgeModeKeyword
	}
	switch mode {
	case protocol.KnowledgeModeHybrid:
		return knowledge.Fuse(keyword, semantic), mode, nil
	case protocol.KnowledgeModeSemantic:
		return nearest, mode, nil
	}
	ranked := make([]knowledge.Ranked, len(keyword))
	for position, key := range keyword {
		ranked[position] = knowledge.Ranked{Key: key, Score: scores[key]}
	}
	return ranked, mode, nil
}

func hits(ctx context.Context, db *sql.Conn, ranked []knowledge.Ranked) ([]protocol.KnowledgeHit, error) {
	hits := []protocol.KnowledgeHit{}
	for _, candidate := range ranked {
		var collection, position string
		hit := protocol.KnowledgeHit{Citation: protocol.KnowledgeCitation{Chunk: candidate.Key}}
		if err := db.QueryRowContext(ctx, "SELECT c.title,c.text,s.collection,s.revision,s.locator,c.position FROM chunks c JOIN sources s ON s.id=c.source WHERE c.key=?1", candidate.Key).
			Scan(&hit.Title, &hit.Text, &collection, &hit.Citation.Revision, &hit.Citation.Path, &position); err != nil {
			return nil, err
		}
		id, err := protocol.ParseKnowledgeID(collection)
		if err != nil {
			return nil, err
		}
		hit.Citation.Collection = id
		if err := json.Unmarshal([]byte(position), &hit.Citation.Position); err != nil {
			return nil, err
		}
		hit.Citation.Link = knowledge.CitationURL(hit.Citation)
		hit.Score = uint32(min(math.Round(float64(max(candidate.Score, 0))*1_000_000), math.MaxUint32))
		hits = append(hits, hit)
	}
	return hits, nil
}
