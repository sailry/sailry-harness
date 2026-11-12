package live

import (
	"sort"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (p *workspaceProject) pendingGitOperations() []panel.PendingOperation {
	keys := []string{}
	for key, command := range p.uncertainOperations {
		if !strings.Contains(command.Kind, "git") || command.Kind == "switch_git_branch" {
			continue
		}
		if command.Kind == "run_git_action" {
			if value, ok := detail[protocol.CommandRunGitActionPayload](command.Data); ok && strings.HasPrefix(value.Action.Kind, "switch_") {
				continue
			}
		}
		keys = append(keys, key)
	}
	sort.Strings(keys)
	operations := []panel.PendingOperation{}
	for _, key := range keys {
		command, settled := p.uncertainOperations[key], p.uncertainCallbacks[key]
		operations = append(operations, panel.PendingOperation{Label: "git_check_result", Run: func(c *ui.Context) {
			p.mutateNamed(workspaceAction, command, settled)
		}})
	}
	return operations
}
