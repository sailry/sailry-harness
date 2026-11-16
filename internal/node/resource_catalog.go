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
			if state, exists := resourceProjections.Load(resourceProjectionKey{node, id}); exists {
				cached := state.(*resourceProjection)
				cached.mu.Lock()
				source.Projects, source.Worktrees, source.Knowledge = cached.source.Projects, cached.source.Worktrees, cached.source.Knowledge
				cached.mu.Unlock()
			}
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
			if source.Fault == nil {
				state, _ := resourceProjections.LoadOrStore(resourceProjectionKey{node, id}, &resourceProjection{})
				cached := state.(*resourceProjection)
				cached.mu.Lock()
				cached.source = source
				cached.mu.Unlock()
			}
			mu.Lock()
			catalog.Sources = append(catalog.Sources, source)
			mu.Unlock()
		})
	}
	workers.Wait()
	slices.SortStableFunc(catalog.Sources, func(a, b protocol.ResourceSource) int {
		if a.Node == b.Node {
			return 0
		}
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
		command := request.Command
		if searching {
			selected := *query
			selected.Offset, selected.Limit = 0, knowledge.PageLimit
			command.Data = &selected
		}
		selected := protocol.NewRequest(id, command)
		selected.Plugin = request.Plugin
		return protocol.ResourceCall{Local: true, Request: selected, Scope: call.Scope}
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
	ranks := map[string]float64{}
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
			for position, hit := range part.Hits {
				key := hit.Citation.Node.String() + "/" + hit.Citation.Collection.String() + "/" + hit.Citation.Chunk
				ranks[key] = 1 / float64(60+position+1)
				page.Hits = append(page.Hits, hit)
			}
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
	for index := range page.Hits {
		hit := &page.Hits[index]
		key := hit.Citation.Node.String() + "/" + hit.Citation.Collection.String() + "/" + hit.Citation.Chunk
		hit.Score = uint32(ranks[key] * 1_000_000)
	}
	slices.SortFunc(page.Hits, func(a, b protocol.KnowledgeHit) int {
		left := a.Citation.Node.String() + "/" + a.Citation.Collection.String() + "/" + a.Citation.Chunk
		right := b.Citation.Node.String() + "/" + b.Citation.Collection.String() + "/" + b.Citation.Chunk
		return cmp.Or(cmp.Compare(ranks[right], ranks[left]), cmp.Compare(a.Citation.Node.String(), b.Citation.Node.String()), cmp.Compare(a.Citation.Collection.String(), b.Citation.Collection.String()), cmp.Compare(a.Citation.Chunk, b.Citation.Chunk))
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

// resourceProjections retains direct-source projections and coalesces changes
// into one reader per source. Derived invalidations cannot circulate because
// only a changed local resource projection publishes another notification.
var resourceProjections sync.Map

type resourceProjectionKey struct {
	owner  *Node
	source protocol.NodeID
}

type resourceProjection struct {
	mu      sync.Mutex
	running bool
	dirty   bool
	known   bool
	digest  [32]byte
	source  protocol.ResourceSource
}

func (node *Node) notifyResourceCatalog(source protocol.NodeID, remote *client.Client) {
	key := resourceProjectionKey{node, source}
	value, _ := resourceProjections.LoadOrStore(key, &resourceProjection{})
	state := value.(*resourceProjection)
	state.mu.Lock()
	state.dirty = true
	if state.running {
		state.mu.Unlock()
		return
	}
	state.running = true
	state.mu.Unlock()
	err := node.tasks.Submit(func(ctx context.Context) {
		ctx, cancel := context.WithCancel(ctx)
		defer cancel()
		defer context.AfterFunc(node.ctx, cancel)()
		for {
			if ctx.Err() != nil {
				state.mu.Lock()
				state.running = false
				state.mu.Unlock()
				return
			}
			state.mu.Lock()
			if !state.dirty {
				state.running = false
				state.mu.Unlock()
				return
			}
			state.dirty = false
			projected := state.source
			state.mu.Unlock()
			projected.Node, projected.Route = source, protocol.RouteKey{Entry: node.ID(), Target: source}
			projected.Fault = nil
			if !remote.Connected() {
				projected.Fault = resourceUnavailable()
			} else {
				snapshot := remote.Snapshot()
				projected.Projects, projected.Worktrees = snapshot.Projects, snapshot.Worktrees
				request := protocol.NewRequest(source, protocol.Command{Kind: "list_knowledge_collections"})
				if output, err := resourceResult(ctx, remote.Resource, protocol.ResourceCall{Request: request, Local: true}); err == nil {
					projected.Knowledge, _ = dispatchPayload[[]protocol.KnowledgeCollection](output.Data)
				} else {
					projected.Fault = resourceFault(err)
				}
			}
			encoded, err := json.Marshal(projected)
			if err != nil {
				continue
			}
			digest := blake3.Sum256(encoded)
			state.mu.Lock()
			changed := !state.known || state.digest != digest
			state.known, state.digest, state.source = true, digest, projected
			state.mu.Unlock()
			if !changed {
				continue
			}
			if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
				_, err := store.RecordEvents(ctx, db, node.ID(), []protocol.Event{{Kind: "resource_catalog_changed"}})
				return err
			}); err == nil {
				node.publish()
			}
		}
	})
	if err != nil {
		state.mu.Lock()
		state.running = false
		state.mu.Unlock()
	}
}
