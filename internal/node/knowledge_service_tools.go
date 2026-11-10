package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"slices"
	"sync"

	"charm.land/fantasy"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// knowledgeOperation reports a knowledge tool. The Node binds these to the
// ordinary knowledge commands with the ownership captured for the turn.
func knowledgeOperation(operation protocol.ToolOperation) bool {
	switch operation {
	case protocol.ToolOperationKnowledgeQuery, protocol.ToolOperationKnowledgeRead, protocol.ToolOperationKnowledgeCollections,
		protocol.ToolOperationKnowledgeCreate, protocol.ToolOperationKnowledgeRebuild, protocol.ToolOperationKnowledgeRemove:
		return true
	}
	return false
}

func knowledgeArguments(err error) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid native operation arguments: " + err.Error()}
}

// knowledgePage clamps a requested limit to the budget of its read. A limit
// describes requested output, not an allocation.
func knowledgePage(arguments json.RawMessage, budget uint64) (map[string]json.RawMessage, error) {
	var values map[string]json.RawMessage
	if err := json.Unmarshal(arguments, &values); err != nil {
		return nil, knowledgeArguments(err)
	}
	if encoded, present := values["limit"]; present {
		var requested uint64
		if err := json.Unmarshal(encoded, &requested); err != nil || requested == 0 || requested > 9_007_199_254_740_991 {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "limit must be a positive safe integer"}
		}
		values["limit"], _ = json.Marshal(min(requested, budget))
	}
	return values, nil
}

func knowledgeDecode(values any, target any) error {
	encoded, err := json.Marshal(values)
	if err == nil {
		err = json.Unmarshal(encoded, target)
	}
	if err != nil {
		return knowledgeArguments(err)
	}
	return nil
}

// knowledgeToolCommand translates model arguments into a knowledge command.
func knowledgeToolCommand(operation protocol.ToolOperation, arguments json.RawMessage, worktree protocol.WorktreeID) (protocol.Command, error) {
	switch operation {
	case protocol.ToolOperationKnowledgeQuery:
		values, err := knowledgePage(arguments, knowledge.PageLimit)
		if err != nil {
			return protocol.Command{}, err
		}
		var query protocol.KnowledgeQuery
		if err := knowledgeDecode(values, &query); err != nil {
			return protocol.Command{}, err
		}
		return protocol.Command{Kind: "query_knowledge", Data: &query}, knowledge.ValidateQuery(query)
	case protocol.ToolOperationKnowledgeRead:
		values, err := knowledgePage(arguments, knowledge.CitationLimit)
		if err != nil {
			return protocol.Command{}, err
		}
		var input protocol.KnowledgeCitationRequest
		if err := knowledgeDecode(values, &input); err != nil {
			return protocol.Command{}, err
		}
		// A model reads the complete source unless it asks for the excerpt.
		if _, present := values["context"]; !present {
			input.Context = true
		}
		return protocol.Command{Kind: "read_knowledge_citation", Data: &input}, knowledge.ValidateCitation(input)
	case protocol.ToolOperationKnowledgeCollections:
		var values map[string]json.RawMessage
		if err := json.Unmarshal(arguments, &values); err != nil || values == nil || len(values) != 0 {
			return protocol.Command{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "knowledge collection listing takes no arguments"}
		}
		return protocol.Command{Kind: "list_knowledge_collections"}, nil
	case protocol.ToolOperationKnowledgeCreate:
		var draft protocol.KnowledgeDraft
		if err := json.Unmarshal(arguments, &draft); err != nil {
			return protocol.Command{}, knowledgeArguments(err)
		}
		definition := protocol.KnowledgeDefinition{Name: draft.Name, Model: draft.Model, Globs: draft.Globs, Tags: draft.Tags, SharedWith: []string{}}
		switch source := draft.Source.Data.(type) {
		case *protocol.KnowledgeDraftSourceWorktreePayload:
			definition.Source = protocol.KnowledgeSource{Kind: "worktree", Data: &protocol.KnowledgeSourceWorktreePayload{Worktree: worktree, Path: source.Path}}
		case *protocol.KnowledgeDraftSourceStoragePayload:
			definition.Source = protocol.KnowledgeSource{Kind: "storage", Data: &protocol.KnowledgeSourceStoragePayload{Scope: source.Scope, Prefix: source.Prefix}}
		}
		return protocol.Command{Kind: "create_knowledge_collection", Data: &protocol.CommandCreateKnowledgeCollectionPayload{Definition: definition}},
			knowledge.ValidateDefinition(definition)
	}
	var input struct {
		ID               protocol.KnowledgeID `json:"id"`
		ExpectedRevision uint64               `json:"expected_revision"`
	}
	if err := protocol.DecodeObject(arguments, &input, []string{"id", "expected_revision"}, true); err != nil {
		return protocol.Command{}, knowledgeArguments(err)
	}
	if input.ExpectedRevision == 0 {
		return protocol.Command{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "knowledge collection revision must be positive"}
	}
	if operation == protocol.ToolOperationKnowledgeRebuild {
		return protocol.Command{Kind: "rebuild_knowledge_collection", Data: &protocol.CommandRebuildKnowledgeCollectionPayload{ID: input.ID, ExpectedRevision: input.ExpectedRevision}}, nil
	}
	return protocol.Command{Kind: "delete_knowledge_collection", Data: &protocol.CommandDeleteKnowledgeCollectionPayload{ID: input.ID, ExpectedRevision: input.ExpectedRevision}}, nil
}

// knowledgeTools binds the knowledge operations a turn's packages declare.
// Every call runs as a request of the declaring package in the captured
// worktree and conversation, so the store applies the package's grants.
func (node *Node) knowledgeTools(inv *store.AgentInvocation, catalog *operations.Catalog) ([]engine.Tool, error) {
	var tools []engine.Tool
	for _, declaration := range catalog.Tools {
		if !knowledgeOperation(declaration.Operation) || !slices.Contains(inv.Turn.Plugins, declaration.Owner) ||
			!declaration.Available(inv.Worktree.Project != nil, connectionBound(inv)) ||
			inv.Turn.Config.Resource != nil && inv.Turn.Config.Assistant == nil && len(declaration.Contexts) == 0 {
			continue
		}
		var schema map[string]any
		if err := json.Unmarshal(declaration.Parameters, &schema); err != nil {
			return nil, err
		}
		scope := protocol.PluginContext{Turn: &inv.Run.Turn, Package: declaration.Owner, Worktree: &inv.Run.Worktree, Session: &inv.Session.ID, Surface: protocol.PluginDesktopSurface{Kind: "workspace"}}
		var mu sync.Mutex
		calls := map[string]*protocol.Request{}
		tool := engine.Tool{Definition: fantasy.FunctionTool{Name: declaration.Name, Description: declaration.Description, InputSchema: schema}, SchemaJSON: declaration.Parameters, ReadOnly: declaration.ReadOnly, Concurrent: declaration.Concurrent, Deferred: !declaration.Initial, Approval: !declaration.ReadOnly, Binding: &store.ApprovalBinding{Owner: declaration.Owner, Tool: declaration.Name, Operation: &declaration.Operation}}
		tool.Prepare = func(_ context.Context, call canonical.ToolRequest) error {
			command, err := knowledgeToolCommand(declaration.Operation, call.Arguments, inv.Run.Worktree)
			if err != nil {
				return err
			}
			request := protocol.NewRequest(node.ID(), command)
			request.Plugin = &scope
			mu.Lock()
			calls[call.ID] = &request
			mu.Unlock()
			return nil
		}
		tool.ConfigureApproval = func(ctx context.Context, db *sql.Conn, call canonical.ToolRequest, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
			mu.Lock()
			request := calls[call.ID]
			mu.Unlock()
			return store.ConfigureWorkspaceApproval(ctx, db, inv.Run.Turn, value, declaration.Operation, request.Command, nil)
		}
		tool.Approved = func(call canonical.ToolRequest, _ protocol.ApprovalID, request protocol.RequestID) {
			mu.Lock()
			defer mu.Unlock()
			calls[call.ID].ID = request
		}
		tool.Execute = func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
			mu.Lock()
			request := calls[call.ID]
			delete(calls, call.ID)
			mu.Unlock()
			if request == nil {
				return workspaceToolFault(errors.New("native tool was not prepared"))
			}
			stopped := connectionFault(protocol.ErrorCodeCancelled, "plugin tool was cancelled")
			if request.Command.Durable() {
				stopped = connectionFault(protocol.ErrorCodeOutcomeUnknown, "knowledge collection change was cancelled; list the collections before retrying")
			}
			output, err := node.connectionDispatch(ctx, *request, stopped, connectionFault(protocol.ErrorCodeOutcomeUnknown, "knowledge operation outcome is unavailable"))
			if err != nil {
				return workspaceToolFault(err)
			}
			return engine.ToolResult{Value: output}
		}
		tools = append(tools, tool)
	}
	return tools, nil
}
