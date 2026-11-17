package protocol

import "testing"

func TestKnowledgeCreationRequiresStorageOrContainerContext(t *testing.T) {
	execution, controller, storage := NodeID{1}, NodeID{2}, NodeID{3}
	for _, test := range []struct {
		name   string
		source KnowledgeSource
		node   *NodeID
		want   NodeID
	}{
		{"ambiguous file", KnowledgeSource{Kind: "file", Data: &KnowledgeSourceFilePayload{Path: "/notes.txt"}}, nil, NodeID{}},
		{"explicit collection target", KnowledgeSource{Kind: "file", Data: &KnowledgeSourceFilePayload{Path: "/notes.txt"}}, &storage, storage},
		{"explicit source target", KnowledgeSource{Kind: "file", Data: &KnowledgeSourceFilePayload{Node: &storage, Path: "/notes.txt"}}, nil, storage},
		{"captured worktree", KnowledgeSource{Kind: "worktree", Data: &KnowledgeSourceWorktreePayload{Worktree: NewWorktreeID()}}, nil, execution},
		{"personal storage", KnowledgeSource{Kind: "storage", Data: &KnowledgeSourceStoragePayload{Scope: PluginStorageScopeNode}}, nil, controller},
		{"project storage", KnowledgeSource{Kind: "storage", Data: &KnowledgeSourceStoragePayload{Scope: PluginStorageScopeProject}}, nil, execution},
	} {
		t.Run(test.name, func(t *testing.T) {
			command := Command{Kind: "create_knowledge_collection", Data: &CommandCreateKnowledgeCollectionPayload{Node: test.node, Definition: KnowledgeDefinition{Source: test.source}}}
			if got := KnowledgeRequestTarget(command, execution, controller); got != test.want {
				t.Fatalf("storage target=%v, want %v", got, test.want)
			}
		})
	}
}
