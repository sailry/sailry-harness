package service

import (
	"context"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestKnowledgeQueryAndCitationRetainStorageNode(t *testing.T) {
	t.Parallel()
	fixture := newHarness(t)
	fixture.service.options.Node = fixture.store.NodeID
	path := filepath.Join(t.TempDir(), "guide.md")
	write(t, path, "# Guide\n\nsource routing")
	collection := fixture.create("Guide", protocol.KnowledgeSource{Kind: "file", Data: &protocol.KnowledgeSourceFilePayload{Path: path}})
	fixture.sweep()
	page := fixture.query("routing", func(query *protocol.KnowledgeQuery) {
		query.Mode = protocol.KnowledgeModeKeyword
		query.Refs = []protocol.KnowledgeRef{{Node: fixture.store.NodeID, ID: collection.ID}}
	})
	if len(page.Hits) != 1 || page.Hits[0].Citation.Node != fixture.store.NodeID || page.Hits[0].Citation.Link != knowledge.CitationURL(page.Hits[0].Citation) {
		t.Fatalf("query lost storage identity: %+v", page)
	}
	citation := page.Hits[0].Citation
	input := protocol.KnowledgeCitationRequest{Node: citation.Node, Collection: citation.Collection, Chunk: citation.Chunk, Revision: citation.Revision, Limit: knowledge.CitationLimit}
	cited, err := fixture.service.Citation(t.Context(), fixture.request("read_knowledge_citation", &input), input)
	if err != nil || cited.Citation.Node != citation.Node {
		t.Fatalf("citation lost storage identity: %+v: %v", cited, err)
	}
	input.Node = protocol.NodeID{9}
	if _, err := fixture.service.Citation(t.Context(), fixture.request("read_knowledge_citation", &input), input); code(err) != protocol.ErrorCodeWrongTarget {
		t.Fatalf("foreign citation was read locally: %v", err)
	}
	page = fixture.query("routing", func(query *protocol.KnowledgeQuery) {
		query.Mode = protocol.KnowledgeModeKeyword
		query.Refs = []protocol.KnowledgeRef{{Node: protocol.NodeID{9}, ID: collection.ID}}
	})
	if len(page.Hits) != 0 {
		t.Fatalf("foreign collection ref selected local same-ID collection: %+v", page)
	}
}

func TestKnowledgeQueryRechecksCurrentAccess(t *testing.T) {
	t.Parallel()
	fixture := newHarness(t)
	path := filepath.Join(t.TempDir(), "guide.md")
	write(t, path, "# Guide\n\nrevoked source")
	fixture.create("Guide", protocol.KnowledgeSource{Kind: "file", Data: &protocol.KnowledgeSourceFilePayload{Path: path}})
	fixture.sweep()
	allowed := fixture.service.options.Allowed
	reads := 0
	fixture.service.options.Allowed = func(ctx context.Context, request protocol.Request) ([]store.KnowledgeStored, error) {
		reads++
		if reads > 1 {
			return nil, nil
		}
		return allowed(ctx, request)
	}
	query := protocol.DefaultKnowledgeQuery()
	query.Query, query.Limit, query.Mode = "revoked", 5, protocol.KnowledgeModeKeyword
	if page, err := fixture.service.Query(t.Context(), fixture.request("query_knowledge", &query), query); code(err) != protocol.ErrorCodePermissionDenied || len(page.Hits) != 0 {
		t.Fatalf("query exposed a revoked collection: %+v: %v", page, err)
	}
}
