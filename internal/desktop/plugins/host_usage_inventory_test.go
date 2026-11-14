package plugins

import (
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestUsageInventorySurvivesQuerySerialization(t *testing.T) {
	project := protocol.DefaultProject()
	project.ID, project.Name = protocol.NewProjectID(), "Example project"
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Name = protocol.NewProviderID(), "Example provider"
	provider.Api, provider.Authentication = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey
	model := protocol.DefaultConversationModel()
	model.ID = "example-model"
	provider.Models = []protocol.ConversationModel{model}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	snapshot.Projects, snapshot.Providers = []protocol.Project{project}, []protocol.ConversationProvider{provider}
	owner, transport := newHostRequestFixture(t, func(request protocol.Request) (link.Admission, error) {
		switch request.Command.Kind {
		case "read_usage":
			// A Node receives normalized JSON arrays, including empty filters.
			encoded, err := json.Marshal(request.Command.Data)
			if err != nil {
				return link.Admission{}, err
			}
			var query protocol.UsageQuery
			if err := json.Unmarshal(encoded, &query); err != nil {
				return link.Admission{}, err
			}
			report := protocol.DefaultUsageReport()
			report.Node, report.Query = snapshot.Node, query
			return hostRequestResponse(request, protocol.Output{Kind: "usage", Data: report}), nil
		case "snapshot":
			return hostRequestResponse(request, protocol.Output{Kind: "snapshot", Data: snapshot}), nil
		default:
			return link.Admission{}, fmt.Errorf("unexpected command %s", request.Command.Kind)
		}
	})
	var inventory json.RawMessage
	for _, export := range owner.sdkModule().Exports {
		if export.Name == "readUsageInventory" {
			var err error
			inventory, err = export.Async(t.Context(), nil)
			if err != nil {
				t.Fatal(err)
			}
			break
		}
	}
	var value struct {
		Projects []struct {
			ID   string `json:"id"`
			Name string `json:"name"`
		} `json:"projects"`
		Providers []struct {
			ID     string   `json:"id"`
			Name   string   `json:"name"`
			Models []string `json:"models"`
		} `json:"providers"`
	}
	if err := json.Unmarshal(inventory, &value); err != nil {
		t.Fatal(err)
	}
	if len(value.Projects) != 1 || value.Projects[0].ID != project.ID.String() || value.Projects[0].Name != project.Name ||
		len(value.Providers) != 1 || value.Providers[0].ID != provider.ID.String() || value.Providers[0].Name != provider.Name ||
		len(value.Providers[0].Models) != 1 || value.Providers[0].Models[0] != model.ID {
		t.Fatalf("usage choices lost Node inventory: %s", inventory)
	}
	requests := transport.snapshot()
	if len(requests) != 2 || requests[0].Command.Kind != "read_usage" || requests[0].Plugin == nil || requests[1].Command.Kind != "snapshot" {
		t.Fatalf("usage inventory skipped its scoped authorization: %+v", requests)
	}
}
