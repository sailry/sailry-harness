package protocol

// KnowledgeCommand identifies the commands served by a Node's knowledge
// service. It does not include the separate resource directory.
func KnowledgeCommand(kind string) bool {
	switch kind {
	case "query_knowledge", "read_knowledge_citation", "read_knowledge_candidates", "list_knowledge_collections",
		"create_knowledge_collection", "update_knowledge_collection", "delete_knowledge_collection",
		"rebuild_knowledge_collection", "pause_knowledge_collection":
		return true
	}
	return false
}

// KnowledgeTarget reads an explicitly selected storage Node. It is used before
// preparing a request and again at admission; it never changes that request.
func KnowledgeTarget(command Command) NodeID {
	var selected *NodeID
	var source *KnowledgeSource
	switch input := command.Data.(type) {
	case CommandCreateKnowledgeCollectionPayload:
		selected, source = input.Node, &input.Definition.Source
	case *CommandCreateKnowledgeCollectionPayload:
		selected, source = input.Node, &input.Definition.Source
	case CommandUpdateKnowledgeCollectionPayload:
		selected, source = input.Node, &input.Definition.Source
	case *CommandUpdateKnowledgeCollectionPayload:
		selected, source = input.Node, &input.Definition.Source
	case CommandDeleteKnowledgeCollectionPayload:
		selected = input.Node
	case *CommandDeleteKnowledgeCollectionPayload:
		selected = input.Node
	case CommandRebuildKnowledgeCollectionPayload:
		selected = input.Node
	case *CommandRebuildKnowledgeCollectionPayload:
		selected = input.Node
	case CommandPauseKnowledgeCollectionPayload:
		selected = input.Node
	case *CommandPauseKnowledgeCollectionPayload:
		selected = input.Node
	case KnowledgeCitationRequest:
		return input.Node
	case *KnowledgeCitationRequest:
		return input.Node
	}
	if selected != nil {
		return *selected
	}
	if source != nil {
		switch input := source.Data.(type) {
		case KnowledgeSourceWorktreePayload:
			selected = input.Node
		case *KnowledgeSourceWorktreePayload:
			selected = input.Node
		case KnowledgeSourceDirectoryPayload:
			selected = input.Node
		case *KnowledgeSourceDirectoryPayload:
			selected = input.Node
		case KnowledgeSourceFilePayload:
			selected = input.Node
		case *KnowledgeSourceFilePayload:
			selected = input.Node
		case KnowledgeSourceStoragePayload:
			selected = input.Node
		case *KnowledgeSourceStoragePayload:
			selected = input.Node
		}
	}
	if selected != nil {
		return *selected
	}
	return NodeID{}
}

// KnowledgeRequestTarget selects the initial target from the captured execution
// and controller identities. Existing collection references take precedence;
// personal storage and an unfiltered catalog or query use the controller view.
func KnowledgeRequestTarget(command Command, execution, controller NodeID) NodeID {
	if selected := KnowledgeTarget(command); selected != (NodeID{}) {
		return selected
	}
	if controller == (NodeID{}) {
		controller = execution
	}
	switch command.Kind {
	case "query_knowledge":
		var query *KnowledgeQuery
		switch input := command.Data.(type) {
		case KnowledgeQuery:
			query = &input
		case *KnowledgeQuery:
			query = input
		}
		if query != nil && KnowledgeQueryLocal(*query, execution) {
			return execution
		}
		return controller
	case "list_knowledge_collections", "read_knowledge_candidates":
		return controller
	case "create_knowledge_collection":
		var source *KnowledgeSource
		switch input := command.Data.(type) {
		case CommandCreateKnowledgeCollectionPayload:
			source = &input.Definition.Source
		case *CommandCreateKnowledgeCollectionPayload:
			source = &input.Definition.Source
		}
		if source != nil && source.Kind == "worktree" {
			return execution
		}
		if source != nil && source.Kind == "storage" {
			var scope PluginStorageScope
			switch input := source.Data.(type) {
			case KnowledgeSourceStoragePayload:
				scope = input.Scope
			case *KnowledgeSourceStoragePayload:
				scope = input.Scope
			}
			if scope == PluginStorageScopeNode {
				return controller
			}
			return execution
		}
		return NodeID{}
	}
	return execution
}

// KnowledgeQueryLocal keeps an explicitly local selection usable while its
// controller is offline. Empty selections always mean the controller view.
func KnowledgeQueryLocal(query KnowledgeQuery, node NodeID) bool {
	if len(query.Refs) == 0 {
		return false
	}
	for _, reference := range query.Refs {
		if reference.Node != node {
			return false
		}
	}
	return true
}
