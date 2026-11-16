package node

import (
	"cmp"
	"context"
	"database/sql"
	"encoding/json"
	"os"
	"runtime"
	"slices"
	"sync"

	"lukechampine.com/blake3"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) resourceClients(ctx context.Context) map[protocol.NodeID]*client.Client {
	user, present := store.UserFromContext(ctx)
	if !present || !user.Local {
		return map[protocol.NodeID]*client.Client{}
	}
	return node.controllerClients()
}

func (node *Node) resourceCatalog(ctx context.Context) (protocol.ResourceCatalog, error) {
	catalog := protocol.ResourceCatalog{Entry: node.ID(), Sources: []protocol.ResourceSource{}}
	local := protocol.ResourceSource{Node: node.ID(), Route: protocol.RouteKey{Entry: node.ID(), Target: node.ID()}, Platform: runtime.GOOS}
	local.Name, _ = os.Hostname()
	snapshot, err := node.Snapshot(ctx)
	if err != nil {
		return catalog, err
	}
	local.Projects, local.Worktrees = snapshot.Projects, snapshot.Worktrees
	output, err := node.readKnowledge(ctx, protocol.NewRequest(node.ID(), protocol.Command{Kind: "list_knowledge_collections"}))
	if err != nil {
		local.Fault = resourceFault(err)
	} else if values, valid := dispatchPayload[[]protocol.KnowledgeCollection](output.Data); valid {
		local.Knowledge = values
	}
	catalog.Sources = append(catalog.Sources, local)
	peers := node.resourceClients(ctx)
	names := map[protocol.NodeID]string{}
	if node.network != nil {
		if relationships, err := node.network.Relationships(ctx); err == nil {
			for _, relationship := range relationships {
				names[relationship.ID] = relationship.Name
			}
		}
	}
	var mu sync.Mutex
	var workers sync.WaitGroup
	for id, peer := range peers {
		workers.Go(func() {
			source := protocol.ResourceSource{Node: id, Route: protocol.RouteKey{Entry: node.ID(), Target: id}, Name: names[id]}
			if !peer.Connected() {
				source.Fault = resourceUnavailable()
			} else {
				snapshot := peer.Snapshot()
				source.Projects, source.Worktrees = snapshot.Projects, snapshot.Worktrees
				request := protocol.NewRequest(id, protocol.Command{Kind: "list_knowledge_collections"})
				output, err := resourceResult(ctx, peer.Resource, protocol.ResourceCall{Request: request, Local: true})
				if err != nil {
					source.Fault = resourceFault(err)
				} else if values, valid := dispatchPayload[[]protocol.KnowledgeCollection](output.Data); valid {
					source.Knowledge = values
				} else {
					source.Fault = resourceUnavailable()
				}
			}
			mu.Lock()
			catalog.Sources = append(catalog.Sources, source)
			mu.Unlock()
		})
	}
	workers.Wait()
	slices.SortStableFunc(catalog.Sources, func(a, b protocol.ResourceSource) int {
		if a.Node == node.ID() {
			return -1
		}
		if b.Node == node.ID() {
			return 1
		}
		return cmp.Compare(a.Node.String(), b.Node.String())
	})
	return catalog, nil
}

func resourceResult(ctx context.Context, send func(context.Context, protocol.ResourceCall) (link.Admission, error), call protocol.ResourceCall) (protocol.Output, error) {
	accepted, err := send(ctx, call)
	if err != nil {
		return protocol.Output{}, err
	}
	select {
	case result, open := <-accepted.Completion:
		if !open {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "resource completion is unavailable"}
		}
		if result.Fault != nil {
			return protocol.Output{}, result.Fault
		}
		return result.Output, nil
	case <-ctx.Done():
		return protocol.Output{}, ctx.Err()
	}
}

func (node *Node) aggregateKnowledge(ctx context.Context, caller protocol.NodeID, call protocol.ResourceCall) (protocol.Output, error) {
	request := call.Request
	query, searching := request.Command.Data.(*protocol.KnowledgeQuery)
	if searching {
		if err := knowledge.ValidateQuery(*query); err != nil {
			return protocol.Output{}, err
		}
	}
	type result struct {
		node   protocol.NodeID
		output protocol.Output
		err    error
	}
	peers := node.resourceClients(ctx)
	results := make(chan result, len(peers)+1)
	var workers sync.WaitGroup
	searchNode := func(id protocol.NodeID) bool {
		return !searching || len(query.Refs) == 0 || slices.ContainsFunc(query.Refs, func(reference protocol.KnowledgeRef) bool { return reference.Node == id })
	}
	leaf := func(id protocol.NodeID) protocol.ResourceCall {
		copy := call
		copy.Local = true
		copy.Request.Target = id
		if searching {
			selected := *query
			selected.Offset, selected.Limit = 0, knowledge.PageLimit
			copy.Request.Command.Data = &selected
		}
		return copy
	}
	if searchNode(node.ID()) {
		workers.Go(func() {
			output, err := node.readKnowledge(ctx, leaf(node.ID()).Request)
			results <- result{node.ID(), output, err}
		})
	}
	for id, peer := range peers {
		if !searchNode(id) {
			continue
		}
		workers.Go(func() {
			if !peer.Connected() {
				results <- result{node: id, err: resourceUnavailable()}
				return
			}
			output, err := resourceResult(ctx, peer.Resource, leaf(id))
			results <- result{id, output, err}
		})
	}
	workers.Wait()
	close(results)
	collections := []protocol.KnowledgeCollection{}
	page := protocol.DefaultKnowledgePage()
	page.Untrusted = true
	if searching {
		page.Mode = query.Mode
	}
	if searching {
		for _, ref := range query.Refs {
			if ref.Node != node.ID() && peers[ref.Node] == nil && !slices.ContainsFunc(page.Sources, func(source protocol.ResourceSourceStatus) bool { return source.Node == ref.Node }) {
				page.Sources = append(page.Sources, protocol.ResourceSourceStatus{Node: ref.Node, Fault: resourceUnavailable()})
			}
		}
	}
	modelSeen := false
	for result := range results {
		page.Sources = append(page.Sources, protocol.ResourceSourceStatus{Node: result.node, Fault: resourceFault(result.err)})
		if result.err != nil {
			continue
		}
		if searching {
			part, valid := dispatchPayload[protocol.KnowledgePage](result.output.Data)
			if !valid || result.output.Kind != "knowledge_search" {
				page.Sources[len(page.Sources)-1].Fault = resourceUnavailable()
				continue
			}
			page.Hits = append(page.Hits, part.Hits...)
			page.Indexing = page.Indexing || part.Indexing
			if !modelSeen {
				page.Model, page.Mode, modelSeen = part.Model, part.Mode, true
			} else {
				if page.Model == nil || part.Model == nil || *page.Model != *part.Model {
					page.Model = nil
				}
				if page.Mode != part.Mode {
					page.Mode = protocol.KnowledgeModeHybrid
				}
			}
		} else if part, valid := dispatchPayload[[]protocol.KnowledgeCollection](result.output.Data); valid && result.output.Kind == "knowledge_collections" {
			collections = append(collections, part...)
		}
	}
	if !searching {
		slices.SortFunc(collections, func(a, b protocol.KnowledgeCollection) int {
			return cmp.Or(cmp.Compare(a.Definition.Name, b.Definition.Name), cmp.Compare(a.Node.String(), b.Node.String()), cmp.Compare(a.ID.String(), b.ID.String()))
		})
		return protocol.Output{Kind: "knowledge_collections", Data: collections}, nil
	}
	slices.SortFunc(page.Sources, func(a, b protocol.ResourceSourceStatus) int { return cmp.Compare(a.Node.String(), b.Node.String()) })
	slices.SortFunc(page.Hits, func(a, b protocol.KnowledgeHit) int {
		return cmp.Or(cmp.Compare(b.Score, a.Score), cmp.Compare(a.Citation.Node.String(), b.Citation.Node.String()), cmp.Compare(a.Citation.Collection.String(), b.Citation.Collection.String()), cmp.Compare(a.Citation.Chunk, b.Citation.Chunk))
	})
	start := min(uint64(query.Offset), uint64(len(page.Hits)))
	end := min(start+uint64(min(query.Limit, knowledge.PageLimit)), uint64(len(page.Hits)))
	if end < uint64(len(page.Hits)) {
		next := uint32(end)
		page.NextOffset = &next
	}
	page.Hits = page.Hits[start:end]
	return protocol.Output{Kind: "knowledge_search", Data: page}, nil
}

// resourceProjections tracks only direct-source content, so a propagated
// invalidation cannot circulate between Nodes connected in both directions.
var resourceProjections sync.Map

type resourceProjectionKey struct {
	owner  *Node
	source protocol.NodeID
}

func (node *Node) notifyResourceCatalog(source protocol.NodeID, remote *client.Client) {
	_ = node.tasks.Submit(func(ctx context.Context) {
		snapshot := remote.Snapshot()
		projection := struct {
			Connected bool
			Projects  []protocol.Project
			Worktrees []protocol.Worktree
			Knowledge []protocol.KnowledgeCollection
		}{Connected: remote.Connected(), Projects: snapshot.Projects, Worktrees: snapshot.Worktrees}
		if projection.Connected {
			request := protocol.NewRequest(source, protocol.Command{Kind: "list_knowledge_collections"})
			if output, err := resourceResult(ctx, remote.Resource, protocol.ResourceCall{Request: request, Local: true}); err == nil {
				projection.Knowledge, _ = dispatchPayload[[]protocol.KnowledgeCollection](output.Data)
			}
		}
		encoded, err := json.Marshal(projection)
		if err != nil {
			return
		}
		digest := blake3.Sum256(encoded)
		key := resourceProjectionKey{node, source}
		if previous, present := resourceProjections.Swap(key, digest); present && previous == digest {
			return
		}
		if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			_, err := store.RecordEvents(ctx, db, node.ID(), []protocol.Event{{Kind: "resource_catalog_changed"}})
			return err
		}); err == nil {
			node.publish()
		}
	})
}
