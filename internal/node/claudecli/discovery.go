package claudecli

import (
	"errors"
	"fmt"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/program"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Discover lists the models the package declares. Claude Code lists none
// without a request, and discovery sends none: it starts no process and only
// confirms that the executable is on the search path.
func Discover(endpoint string, capture Capture) (protocol.ConversationDiscoveryCatalog, error) {
	catalog := protocol.ConversationDiscoveryCatalog{Endpoint: endpoint, Models: []protocol.ConversationDiscoveryModel{}}
	route := capture.Adapter.ClaudeCode
	if route == nil {
		return catalog, &model.Failure{Kind: model.NotConfigured, Message: "captured model adapter is unavailable"}
	}
	environment := map[string]string{}
	for _, entry := range capture.Host {
		if name, value, ok := strings.Cut(entry, "="); ok && (name == "PATH" || name == "PATHEXT") {
			environment[name] = value
		}
	}
	if _, err := program.Resolve(capture.Program.Command, environment); errors.Is(err, program.ErrNotInstalled) {
		return catalog, &model.Failure{Kind: model.NotConfigured, Message: fmt.Sprintf("%s is not installed or not on the search path", capture.Program.Command)}
	} else if err != nil {
		return catalog, &model.Failure{Kind: model.NotConfigured, Message: err.Error()}
	}
	for _, declared := range route.Models {
		capabilities := protocol.DefaultConversationDiscoveryCapabilities()
		vision, tools := capture.Adapter.Capabilities.Vision, capture.Adapter.Capabilities.Tools
		reasoning := capture.Adapter.Capabilities.Reasoning && len(declared.Efforts) > 0
		capabilities.Vision, capabilities.Tools, capabilities.Reasoning = &vision, &tools, &reasoning
		if reasoning {
			efforts := make([]protocol.Effort, len(declared.Efforts))
			selected := protocol.Effort{Kind: declared.Efforts[0]}
			for index, level := range declared.Efforts {
				efforts[index] = protocol.Effort{Kind: level}
				if level == "medium" {
					selected = efforts[index]
				}
			}
			capabilities.Efforts, capabilities.DefaultEffort = &efforts, &selected
		}
		catalog.Models = append(catalog.Models, protocol.ConversationDiscoveryModel{
			ID: declared.ID, DisplayName: declared.Name, Context: declared.Context, Output: declared.Output, Capabilities: &capabilities,
		})
	}
	return catalog, nil
}
