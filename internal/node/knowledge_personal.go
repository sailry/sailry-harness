package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) searchKnowledgeStorage(ctx context.Context, request protocol.Request) (protocol.PluginStorageSearchPage, error) {
	var controller *protocol.NodeID
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		controller, err = store.PersonalKnowledgeController(ctx, db, request)
		return err
	})
	if err != nil {
		return protocol.PluginStorageSearchPage{}, nodeFault(err)
	}
	if controller == nil {
		return node.knowledge.SearchStorage(ctx, request)
	}
	query, _ := request.Command.Data.(*protocol.PluginStorageSearch)
	candidates := protocol.KnowledgeCandidates{Candidates: []protocol.KnowledgeCandidate{}}
	if query.Mode != protocol.KnowledgeModeKeyword && query.Query != nil && *query.Query != "" {
		scope, err := node.captureResourceScope(ctx, request)
		if err != nil {
			return protocol.PluginStorageSearchPage{}, err
		}
		queryRequest := protocol.NewRequest(*controller, protocol.Command{Kind: "read_knowledge_candidates", Data: query})
		queryRequest.Plugin = request.Plugin
		accepted, err := node.sendResource(ctx, *controller, protocol.ResourceCall{Request: queryRequest, Scope: scope}, false)
		if err == nil {
			select {
			case result := <-accepted.Completion:
				if result.Fault == nil && result.Output.Kind == "knowledge_candidates" {
					candidates, _ = dispatchPayload[protocol.KnowledgeCandidates](result.Output.Data)
				} else {
					if result.Fault != nil && result.Fault.Code != protocol.ErrorCodeUnavailable && result.Fault.Code != protocol.ErrorCodeNotConfigured {
						return protocol.PluginStorageSearchPage{}, result.Fault
					}
					candidates.Indexing = true
				}
			case <-ctx.Done():
				return protocol.PluginStorageSearchPage{}, nodeFault(ctx.Err())
			}
		} else {
			if failure := resourceFault(err); failure.Code != protocol.ErrorCodeUnavailable && failure.Code != protocol.ErrorCodeNotConfigured {
				return protocol.PluginStorageSearchPage{}, err
			}
			candidates.Indexing = true
		}
	}
	var page protocol.PluginStorageSearchPage
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		page, err = store.MaterializePersonalKnowledge(ctx, db, request, candidates)
		return err
	})
	return page, nodeFault(err)
}
