package node

import (
	"context"
	"database/sql"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestExecutionLocalKnowledgeRemainsUsableWithoutController(t *testing.T) {
	t.Parallel()
	execution := nodeFixture(t)
	controller := protocol.NodeID{88}
	var actor protocol.User
	if err := execution.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		var err error
		actor, err = store.CreateUser(t.Context(), db, "Offline controller's task")
		if err != nil {
			return err
		}
		_, err = db.ExecContext(t.Context(), "UPDATE users SET controller=?2 WHERE id=?1", actor.ID.String(), controller[:])
		actor.Controller = &controller
		return err
	}); err != nil {
		t.Fatal(err)
	}
	ctx := store.WithUser(t.Context(), actor)
	call := func(command protocol.Command) (protocol.Output, error) {
		request := protocol.NewRequest(execution.ID(), command)
		return resourceResult(ctx, func(ctx context.Context, call protocol.ResourceCall) (link.Admission, error) {
			return execution.Dispatch(ctx, execution.ID(), call.Request)
		}, protocol.ResourceCall{Request: request})
	}
	definition := protocol.DefaultKnowledgeDefinition()
	definition.Name = "Execution-local notes"
	definition.Source = protocol.KnowledgeSource{Kind: "directory", Data: &protocol.KnowledgeSourceDirectoryPayload{Path: t.TempDir()}}
	target := execution.ID()
	output, err := call(protocol.Command{Kind: "create_knowledge_collection", Data: &protocol.CommandCreateKnowledgeCollectionPayload{Node: &target, Definition: definition}})
	collection, valid := dispatchPayload[protocol.KnowledgeCollection](output.Data)
	if err != nil || !valid || collection.Node != execution.ID() {
		t.Fatalf("offline controller prevented execution-local creation: %+v: %v", output, err)
	}
	query := protocol.DefaultKnowledgeQuery()
	query.Query, query.Mode, query.Limit = "notes", protocol.KnowledgeModeKeyword, 5
	query.Refs = []protocol.KnowledgeRef{{Node: execution.ID(), ID: collection.ID}}
	output, err = call(protocol.Command{Kind: "query_knowledge", Data: &query})
	if err != nil || output.Kind != "knowledge_search" {
		t.Fatalf("explicit local collection routed through the offline controller: %+v: %v", output, err)
	}
	query.Refs = []protocol.KnowledgeRef{{Node: controller, ID: collection.ID}}
	if _, err := call(protocol.Command{Kind: "query_knowledge", Data: &query}); resourceFault(err) == nil || resourceFault(err).Code != protocol.ErrorCodeWrongTarget {
		t.Fatalf("execution accepted a controller-owned source: %v", err)
	}
	query.Refs = nil
	if output, err := call(protocol.Command{Kind: "query_knowledge", Data: &query}); err != nil || output.Kind != "knowledge_search" {
		t.Fatalf("unfiltered execution query depended on its controller: %+v: %v", output, err)
	}
	output, err = call(protocol.Command{Kind: "list_knowledge_collections"})
	collections, valid := dispatchPayload[[]protocol.KnowledgeCollection](output.Data)
	if err != nil || !valid || len(collections) != 1 || collections[0].ID != collection.ID {
		t.Fatalf("execution library list depended on its controller: %+v: %v", output, err)
	}
}

type resourceKnowledgePageTransport struct {
	*resourceDirectoryTransport
	page protocol.KnowledgePage
}

func (transport *resourceKnowledgePageTransport) Resource(ctx context.Context, call protocol.ResourceCall) (link.Admission, error) {
	if call.Request.Command.Kind != "query_knowledge" {
		return transport.resourceDirectoryTransport.Resource(ctx, call)
	}
	completion := make(chan link.Response, 1)
	completion <- link.Response{Output: protocol.Output{Kind: "knowledge_search", Data: transport.page}}
	close(completion)
	return link.Admission{Completion: completion}, nil
}

func TestKnowledgeSearchKeepsSuccessfulHitsAndUnavailableSources(t *testing.T) {
	t.Parallel()
	controller, source := nodeFixture(t), nodeFixture(t)
	missing := protocol.NodeID{89}
	page := protocol.DefaultKnowledgePage()
	page.Hits = []protocol.KnowledgeHit{{Title: "Available evidence", Citation: protocol.KnowledgeCitation{Node: source.ID(), Collection: protocol.NewKnowledgeID(), Chunk: "fixture"}}}
	transport := &resourceKnowledgePageTransport{resourceDirectoryTransport: newResourceDirectoryTransport(t, source), page: page}
	online, offline := client.New(transport), client.New(&resourceCatalogTransport{target: missing})
	t.Cleanup(func() { _ = online.Close(); _ = offline.Close() })
	ctx := resourceCatalogFixture(t, controller, map[protocol.NodeID]*client.Client{source.ID(): online, missing: offline})
	if err := online.Start(ctx); err != nil {
		t.Fatal(err)
	}
	query := protocol.DefaultKnowledgeQuery()
	query.Query, query.Mode, query.Limit = "evidence", protocol.KnowledgeModeKeyword, 5
	query.Refs = []protocol.KnowledgeRef{{Node: source.ID(), ID: page.Hits[0].Citation.Collection}, {Node: missing, ID: protocol.NewKnowledgeID()}}
	request := protocol.NewRequest(controller.ID(), protocol.Command{Kind: "query_knowledge", Data: &query})
	output, err := controller.aggregateKnowledge(ctx, controller.ID(), protocol.ResourceCall{Request: request})
	combined, valid := dispatchPayload[protocol.KnowledgePage](output.Data)
	if err != nil || !valid || len(combined.Hits) != 1 || combined.Hits[0].Title != "Available evidence" || len(combined.Sources) != 2 {
		t.Fatalf("partial knowledge search discarded successes or failure metadata: %+v: %v", output, err)
	}
	for _, source := range combined.Sources {
		if source.Node == missing && (source.Fault == nil || source.Fault.Code != protocol.ErrorCodeUnavailable) {
			t.Fatalf("missing source was reported as zero hits: %+v", combined)
		}
		if source.Node == transport.source.ID() && source.Fault != nil {
			t.Fatalf("available source was incorrectly failed: %+v", combined)
		}
	}
	request = protocol.NewRequest(controller.ID(), protocol.Command{Kind: "list_knowledge_collections"})
	if _, err := controller.aggregateKnowledge(ctx, controller.ID(), protocol.ResourceCall{Request: request}); resourceFault(err) == nil || resourceFault(err).Code != protocol.ErrorCodeUnavailable || !strings.Contains(err.Error(), missing.String()) {
		t.Fatalf("legacy directory list concealed a missing source: %v", err)
	}
}
