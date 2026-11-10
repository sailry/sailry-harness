package service

import (
	"cmp"
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"math"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Embedding is one model's vector of the query text.
type Embedding struct {
	Model  string
	Values []float32
}

// filter admits only chunks of captured grants and applies source filters
// before any candidate limit. A paused grant keeps its cached keyword text.
const filter = "JOIN sources s ON s.id=c.source JOIN collections k ON k.id=s.collection JOIN json_each(?1) admitted ON json_extract(admitted.value,'$.id')=k.id AND json_extract(admitted.value,'$.scope')=lower(hex(k.scope)) AND json_extract(admitted.value,'$.rebuild')=k.rebuild AND (json_extract(admitted.value,'$.generation')=k.generation OR (json_extract(admitted.value,'$.semantic')=0 AND k.generation<=json_extract(admitted.value,'$.generation'))) WHERE NOT EXISTS(SELECT 1 FROM json_each(?2) wanted WHERE NOT EXISTS(SELECT 1 FROM json_each(s.tags) actual WHERE actual.value=wanted.value)) AND (?3 IS NULL OR substr(s.locator,1,length(?3))=?3) AND (?4 IS NULL OR s.modified_ms>=?4) AND (?5 IS NULL OR s.modified_ms<=?5)"

func searchCancelled(ctx context.Context) error {
	if ctx.Err() != nil {
		return newFault(protocol.ErrorCodeCancelled, "knowledge search was cancelled")
	}
	return nil
}

// Search ranks the chunks the bindings admit and returns one page.
func (index Index) Search(ctx context.Context, query protocol.KnowledgeQuery, allowed []Binding, embeddings []Embedding, indexing bool) (protocol.KnowledgePage, error) {
	page := protocol.DefaultKnowledgePage()
	err := index.read(ctx, func(db *sql.Conn) error {
		ranked, mode, err := ranks(ctx, db, query, allowed, embeddings)
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

func ranks(ctx context.Context, db *sql.Conn, query protocol.KnowledgeQuery, allowed []Binding, embeddings []Embedding) ([]knowledge.Ranked, protocol.KnowledgeMode, error) {
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
	grants, tags := encoded(admitted), tagsJSON(query.Tags)
	scores := map[string]float32{}
	var keyword, semantic []string
	if terms := knowledge.Query(query.Query); terms != "" {
		// Scores are materialized before the joins so that every captured
		// grant and source filter applies before the candidate limit.
		rows, err := db.QueryContext(ctx, "WITH candidates AS MATERIALIZED (SELECT rowid AS id,-bm25(chunks_fts,2,1,2,1) AS score FROM chunks_fts WHERE chunks_fts MATCH ?6) SELECT c.key,f.score FROM candidates f JOIN chunks c ON c.id=f.id "+filter+" ORDER BY f.score DESC,c.key LIMIT 100",
			grants, tags, path, after, before, terms)
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
		for _, embedding := range embeddings {
			rows, err := db.QueryContext(ctx, "SELECT c.key,c.vector FROM chunks c "+filter+" AND json_extract(admitted.value,'$.semantic')=1 AND k.model=?6 AND k.dimension=?7 AND c.vector IS NOT NULL",
				grants, tags, path, after, before, embedding.Model, len(embedding.Values))
			if err != nil {
				return nil, "", err
			}
			defer rows.Close()
			for rows.Next() {
				if err := searchCancelled(ctx); err != nil {
					return nil, "", err
				}
				var key string
				var bytes []byte
				if err := rows.Scan(&key, &bytes); err != nil {
					return nil, "", err
				}
				vector, err := knowledge.DecodeVector(bytes, len(embedding.Values))
				if err != nil {
					return nil, "", err
				}
				score, err := vector.Cosine(embedding.Values)
				if err != nil {
					return nil, "", err
				}
				nearest = append(nearest, knowledge.Ranked{Key: key, Score: score})
			}
			if err := rows.Close(); err != nil {
				return nil, "", err
			}
		}
	}
	slices.SortFunc(nearest, func(left, right knowledge.Ranked) int {
		return cmp.Or(cmp.Compare(right.Score, left.Score), cmp.Compare(left.Key, right.Key))
	})
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
