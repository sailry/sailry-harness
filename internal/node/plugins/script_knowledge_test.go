package plugins

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestScriptKnowledgeUsesExecutionNodeAndLocalReferences(t *testing.T) {
	environment := scriptTestEnvironment()
	storage := environment.Target
	encoded, err := json.Marshal(storage)
	if err != nil {
		t.Fatal(err)
	}
	ref := fmt.Sprintf(`{node:%s,id:"00000000-0000-4000-8000-000000000001"}`, encoded)
	citation := fmt.Sprintf(`{node:%s,collection:"00000000-0000-4000-8000-000000000001",chunk:"%s",revision:"%s"}`, encoded, strings.Repeat("a", 64), strings.Repeat("b", 64))
	for _, fixture := range []struct {
		name   string
		call   string
		kind   string
		target protocol.NodeID
	}{
		{"query", `await queryKnowledge({query:"notes"})`, "query_knowledge", environment.Target},
		{"list", `await listKnowledgeCollections()`, "list_knowledge_collections", environment.Target},
		{"citation", `await readKnowledgeCitation(` + citation + `)`, "read_knowledge_citation", storage},
		{"create", `await completeRequest(prepareKnowledgeCollection({name:"Notes",visibility:"shared",source:{kind:"storage",scope:"node",prefix:"note/"}}))`, "create_knowledge_collection", environment.Target},
		{"rebuild", `await completeRequest(prepareKnowledgeRebuild(` + ref + `,"9007199254740993"))`, "rebuild_knowledge_collection", storage},
		{"remove", `await completeRequest(prepareKnowledgeRemoval(` + ref + `,"9007199254740993"))`, "delete_knowledge_collection", storage},
		{"pause", `await completeRequest(prepareKnowledgePause(` + ref + `,true,"9007199254740993"))`, "pause_knowledge_collection", storage},
		{"update", `await completeRequest(prepareKnowledgeUpdate(` + ref + `,{name:"Notes",source:{kind:"storage",scope:"node",prefix:"note/"}},"9007199254740993"))`, "update_knowledge_collection", storage},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			source := `import {queryKnowledge,listKnowledgeCollections,readKnowledgeCitation,prepareKnowledgeCollection,prepareKnowledgeRebuild,prepareKnowledgeRemoval,prepareKnowledgePause,prepareKnowledgeUpdate,completeRequest} from "sailry/sdk";export async function run(){return ` + fixture.call + `}`
			calls := 0
			_, err := RunCallback(t.Context(), scriptTestBundle(source), "run", nil, environment, false, func(_ context.Context, request protocol.Request) (protocol.Output, error) {
				calls++
				if request.Target != fixture.target || request.Command.Kind != fixture.kind || request.Plugin == nil || request.Plugin.Package != environment.Context.Package {
					t.Fatalf("SDK lost its captured target or package: %+v", request)
				}
				switch payload := request.Command.Data.(type) {
				case *protocol.CommandCreateKnowledgeCollectionPayload:
					if payload.Definition.Visibility != protocol.KnowledgeVisibilityShared {
						t.Fatal("SDK discarded the selected collection visibility")
					}
				case *protocol.CommandRebuildKnowledgeCollectionPayload:
					if payload.ExpectedRevision != 9_007_199_254_740_993 {
						t.Fatal("SDK rounded the collection revision")
					}
				case *protocol.CommandUpdateKnowledgeCollectionPayload:
					if payload.ExpectedRevision != 9_007_199_254_740_993 {
						t.Fatal("SDK rounded the collection revision")
					}
				}
				return protocol.Output{Kind: "knowledge_collections", Data: []protocol.KnowledgeCollection{}}, nil
			})
			if err != nil || calls != 1 {
				t.Fatalf("SDK route: %d calls: %v", calls, err)
			}
		})
	}
}

func TestScriptKnowledgeLocalSelectionWorksWithoutController(t *testing.T) {
	environment := scriptTestEnvironment()
	node, err := json.Marshal(environment.Target)
	if err != nil {
		t.Fatal(err)
	}
	source := fmt.Sprintf(`import {queryKnowledge} from "sailry/sdk";export async function run(){return await queryKnowledge({query:"local",refs:[{node:%s,id:"00000000-0000-4000-8000-000000000001"}]})}`, node)
	_, err = RunCallback(t.Context(), scriptTestBundle(source), "run", nil, environment, false, func(_ context.Context, request protocol.Request) (protocol.Output, error) {
		if request.Target != environment.Target {
			t.Fatal("local collection selection depended on its controller")
		}
		return protocol.Output{Kind: "knowledge_search", Data: protocol.DefaultKnowledgePage()}, nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestScriptKnowledgeRejectsForeignSelectionBeforeDispatch(t *testing.T) {
	environment := scriptTestEnvironment()
	foreign := protocol.NodeID{2}
	node, err := json.Marshal(foreign)
	if err != nil {
		t.Fatal(err)
	}
	query := fmt.Sprintf(`import {queryKnowledge} from "sailry/sdk";export async function run(){return await queryKnowledge({query:"notes",refs:[{node:%s,id:"00000000-0000-4000-8000-000000000001"}]})}`, node)
	_, err = RunCallback(t.Context(), scriptTestBundle(query), "run", nil, environment, false, func(_ context.Context, request protocol.Request) (protocol.Output, error) {
		t.Fatalf("script dispatched a foreign collection selection: %+v", request)
		return protocol.Output{}, nil
	})
	if err == nil || scriptFault(err).Code != protocol.ErrorCodeWrongTarget {
		t.Fatalf("script accepted a foreign collection selection: %v", err)
	}
}
