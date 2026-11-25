package live

import (
	"context"
	"errors"
	"os"
	"strings"

	hostassembly "github.com/sailry/sailry-harness/internal/assembly/host"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/terminal"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Projection fixtures exercise the shared Node services and live adapters, not
// a product's agent tools. Select those services without changing Harness defaults.
func panelProjectionHost() moduleapi.Assembly {
	assembly := hostassembly.Harness()
	assembly.Modules = append(assembly.Modules,
		projectionRepositoryModule{projectionServiceModule("projection-repository")},
		projectionTerminalModule{projectionServiceModule("terminal")},
	)
	return assembly
}

type projectionRepositoryModule struct{ projectionServiceModule }

func (projectionRepositoryModule) WorkspaceServices() moduleapi.WorkspaceServices {
	return moduleapi.WorkspaceServices{Git: true, Bind: func(*moduleapi.WorkspaceRuntime) (moduleapi.WorkspaceBindings, error) {
		return moduleapi.WorkspaceBindings{
			RepositoryWatch: &moduleapi.RepositoryWatch{
				Open: func(context.Context, string) (moduleapi.RepositoryWatchFilter, []string, error) {
					return projectionWatchFilter{}, nil, nil
				},
				Invalidate: func(string) {},
			},
			Commands: map[string]moduleapi.WorkspaceCommand{
				"inspect_git": {Run: func(_ context.Context, scope moduleapi.WorkspaceCommandScope, _ protocol.Command) (protocol.Output, error) {
					status, err := panelFixtureChanges(scope.Root())
					return protocol.Output{Kind: "git_status", Data: status}, err
				}},
			},
		}, nil
	}}
}

type projectionWatchFilter struct{}

func (projectionWatchFilter) Changed([]string) bool           { return true }
func (projectionWatchFilter) ReferencesChanged([]string) bool { return false }

func panelFixtureChanges(root string) (protocol.GitStatus, error) {
	status := protocol.GitStatus{Kind: protocol.RepositoryKindDirectory, Entries: []protocol.GitEntry{}}
	anchor, err := os.OpenRoot(root)
	if err != nil {
		return status, err
	}
	defer anchor.Close()
	body, err := anchor.ReadFile("README.md")
	if errors.Is(err, os.ErrNotExist) {
		return status, nil
	}
	if err != nil {
		return status, err
	}
	added, fixture := strings.CutPrefix(string(body), "# Panel\n")
	if !fixture {
		return status, nil
	}
	branch, head := "main", strings.Repeat("0", 40)
	status.Kind, status.Branch, status.Head = protocol.RepositoryKindReady, &branch, &head
	if added != "" {
		modified := protocol.GitChangeKindModified
		status.Entries = append(status.Entries, protocol.GitEntry{Path: "README.md", Unstaged: &modified, Diff: protocol.GitLineStats{Additions: uint64(strings.Count(added, "\n"))}})
	}
	return status, nil
}

type projectionTerminalModule struct{ projectionServiceModule }

func (projectionTerminalModule) NewTerminalService(ctx context.Context, id protocol.NodeID, publish func(context.Context, protocol.TerminalInfo)) *terminal.Service {
	return terminal.New(ctx, id, publish)
}

type projectionServiceModule string

func (module projectionServiceModule) ID() string         { return string(module) }
func (projectionServiceModule) ReplacesPlugins() []string { return nil }
func (projectionServiceModule) Catalog() []moduleapi.ToolMeta {
	return nil
}
func (projectionServiceModule) ToolsForTurn(context.Context, moduleapi.Host, *store.AgentInvocation) ([]engine.Tool, error) {
	return nil, nil
}
