package node

import (
	"bytes"
	"context"
	"encoding/json"

	"github.com/Masterminds/semver/v3"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const pluginInventoryLost = "plugin inventory response is unavailable"

// maxInstalledPlugins bounds one update check to the installed package limit.
const maxInstalledPlugins = 64

// featurePlugin rejects standalone skill and MCP packages, which have their
// own managers.
func featurePlugin(info protocol.PluginInfo) error {
	if info.Skill != nil || info.McpSource != nil {
		return connectionFault(protocol.ErrorCodeInvalidRequest, "use manage_skills or manage_mcp for standalone packages")
	}
	return nil
}

func pluginManagerSummary(info protocol.PluginInfo) map[string]any {
	return map[string]any{"summary": info.Summary, "origin": info.Origin, "skills": info.Skills, "mcp": info.Mcp, "issues": info.Issues}
}

type pluginManagerAction struct {
	Action string  `json:"action"`
	Source string  `json:"source"`
	Query  string  `json:"query"`
	Page   *uint32 `json:"page"`
	Force  bool    `json:"force"`
	Target *struct {
		Kind    string          `json:"kind"`
		Name    string          `json:"name"`
		Source  json.RawMessage `json:"source"`
		ID      string          `json:"id"`
		Bundled bool            `json:"bundled"`
	} `json:"target"`
	Packages  []protocol.PluginUpdatesRequest `json:"packages"`
	Name      string                          `json:"name"`
	Selection *struct {
		Kind   string                        `json:"kind"`
		Source protocol.PluginSkillsResolved `json:"source"`
		Path   string                        `json:"path"`
	} `json:"selection"`
	ExpectedRevision uint64 `json:"expected_revision"`
}

func (action pluginManagerAction) readOnly() bool {
	switch action.Action {
	case "list", "search", "inspect", "check_updates":
		return true
	}
	return false
}

// pluginManager declares manage_plugins over the installed inventory, the
// plugin catalogs and the durable package installer.
func (node *Node) pluginManager(inv *store.AgentInvocation) agentTool {
	plan := inv.Turn.Config.Mode == protocol.WorkModePlan
	installed := func(ctx context.Context, name string, revision uint64) (protocol.PluginInfo, error) {
		info, found, err := node.agentPlugin(ctx, inv, name, pluginInventoryLost)
		if err != nil {
			return info, err
		}
		if !found {
			return info, connectionFault(protocol.ErrorCodeInvalidRequest, "expected an installed plugin")
		}
		if err := featurePlugin(info); err != nil {
			return info, err
		}
		if revision == 0 || info.Summary.Revision != revision {
			return info, connectionFault(protocol.ErrorCodeRevisionConflict, "plugin changed; reload its revision")
		}
		return info, nil
	}
	// candidate turns a newer version of the same package into the command
	// that installs it from its recorded origin.
	candidate := func(info, current protocol.PluginInfo) (protocol.Command, error) {
		if err := featurePlugin(info); err != nil {
			return protocol.Command{}, err
		}
		newer := false
		if info.Summary.Version != nil && current.Summary.Version != nil {
			previous, previousErr := semver.StrictNewVersion(*current.Summary.Version)
			next, nextErr := semver.StrictNewVersion(*info.Summary.Version)
			newer = previousErr == nil && nextErr == nil && next.Compare(previous) > 0
		}
		if info.Summary.Name != current.Summary.Name || !newer {
			return protocol.Command{}, connectionFault(protocol.ErrorCodeInvalidRequest, "plugin update requires a newer version of the same package")
		}
		name, expected := current.Summary.Name, current.Summary.Revision
		if info.Origin != nil {
			switch info.Origin.Kind {
			case "bundled":
				return protocol.Command{Kind: "install_bundled_plugin", Data: &protocol.CommandInstallBundledPluginPayload{Name: name, ExpectedRevision: expected}}, nil
			case "online":
				if origin, valid := dispatchPayload[protocol.PluginOriginOnlinePayload](info.Origin.Data); valid {
					return protocol.Command{Kind: "install_plugin_source", Data: &protocol.CommandInstallPluginSourcePayload{Source: origin.Source, Path: origin.Path, Name: name, ExpectedRevision: expected}}, nil
				}
			case "worktree":
				if origin, valid := dispatchPayload[protocol.PluginOriginWorktreePayload](info.Origin.Data); valid && origin.Worktree == inv.Run.Worktree {
					digest := info.Summary.Digest
					return protocol.Command{Kind: "install_plugin", Data: &protocol.CommandInstallPluginPayload{Worktree: origin.Worktree, Path: origin.Path, Name: name, ExpectedRevision: expected, ExpectedDigest: &digest}}, nil
				}
			}
		}
		return protocol.Command{}, connectionFault(protocol.ErrorCodeInvalidRequest, "this plugin requires another explicit source selection")
	}
	decode := func(arguments json.RawMessage) (pluginManagerAction, error) {
		var action pluginManagerAction
		if err := agentArguments(arguments, &action); err != nil {
			return action, err
		}
		if plan && !action.readOnly() {
			return action, connectionFault(protocol.ErrorCodePermissionDenied, "planning turns cannot change plugins")
		}
		return action, nil
	}
	revisionRequired := connectionFault(protocol.ErrorCodeInvalidRequest, "plugin mutation requires a current installed revision")
	command := func(action pluginManagerAction) (protocol.Command, error) {
		invalid := connectionFault(protocol.ErrorCodeInvalidRequest, "invalid tool arguments: unknown plugin action")
		switch action.Action {
		case "list":
			return protocol.Command{Kind: "list_plugins"}, nil
		case "search":
			page := uint32(1)
			if action.Page != nil {
				page = *action.Page
			}
			return protocol.Command{Kind: "search_plugin_catalog", Data: &protocol.CommandSearchPluginCatalogPayload{Source: protocol.PluginCatalogSource(action.Source), Query: action.Query, Page: page, Force: action.Force}}, nil
		case "inspect":
			if action.Target == nil {
				return protocol.Command{}, invalid
			}
			switch action.Target.Kind {
			case "installed":
				return protocol.Command{Kind: "read_plugin", Data: &protocol.CommandReadPluginPayload{Name: action.Target.Name}}, nil
			case "catalog":
				var source protocol.PluginCatalogSource
				if err := agentArguments(action.Target.Source, &source); err != nil {
					return protocol.Command{}, err
				}
				return protocol.Command{Kind: "read_catalog_plugin_info", Data: &protocol.CommandReadCatalogPluginInfoPayload{Source: source, ID: action.Target.ID, Bundled: action.Target.Bundled}}, nil
			case "git":
				var source protocol.PluginSkillsSource
				if err := agentArguments(action.Target.Source, &source); err != nil {
					return protocol.Command{}, err
				}
				return protocol.Command{Kind: "inspect_plugin_source", Data: &protocol.CommandInspectPluginSourcePayload{Source: source}}, nil
			}
			return protocol.Command{}, invalid
		case "check_updates":
			return protocol.Command{Kind: "check_plugin_updates", Data: &protocol.CommandCheckPluginUpdatesPayload{Packages: action.Packages, Force: action.Force}}, nil
		case "install":
			if action.Selection == nil {
				return protocol.Command{}, invalid
			}
			switch action.Selection.Kind {
			case "bundled":
				return protocol.Command{Kind: "install_bundled_plugin", Data: &protocol.CommandInstallBundledPluginPayload{Name: action.Name}}, nil
			case "git":
				return protocol.Command{Kind: "install_plugin_source", Data: &protocol.CommandInstallPluginSourcePayload{Source: action.Selection.Source, Path: action.Selection.Path, Name: action.Name}}, nil
			}
			return protocol.Command{}, invalid
		case "uninstall":
			if action.ExpectedRevision == 0 {
				return protocol.Command{}, revisionRequired
			}
			return protocol.Command{Kind: "remove_plugin", Data: &protocol.CommandRemovePluginPayload{Name: action.Name, ExpectedRevision: action.ExpectedRevision}}, nil
		}
		return protocol.Command{}, invalid
	}
	prepare := func(ctx context.Context, arguments json.RawMessage) (protocol.Command, error) {
		action, err := decode(arguments)
		if err != nil {
			return protocol.Command{}, err
		}
		switch action.Action {
		case "update":
			current, err := installed(ctx, action.Name, action.ExpectedRevision)
			if err != nil {
				return protocol.Command{}, err
			}
			output, err := node.agentCommand(ctx, inv, protocol.Command{Kind: "check_plugin_update", Data: &protocol.CommandCheckPluginUpdatePayload{Name: action.Name, ExpectedRevision: action.ExpectedRevision, Force: action.Force}}, pluginInventoryLost)
			if err != nil {
				return protocol.Command{}, err
			}
			report, reported := dispatchPayload[protocol.PluginUpdatesReport](output.Data)
			if !reported {
				return protocol.Command{}, connectionFault(protocol.ErrorCodeInvalidRequest, "expected a plugin update report")
			}
			before, err := json.Marshal(current.Summary)
			if err != nil {
				return protocol.Command{}, err
			}
			after, err := json.Marshal(report.Current)
			if err != nil {
				return protocol.Command{}, err
			}
			if !bytes.Equal(before, after) {
				return protocol.Command{}, connectionFault(protocol.ErrorCodeRevisionConflict, "plugin changed during inspection")
			}
			if report.Available == nil {
				return protocol.Command{}, connectionFault(protocol.ErrorCodeNotFound, "no newer plugin version is available")
			}
			if info, packaged := dispatchPayload[protocol.PluginInfo](report.Available.Data); packaged {
				return candidate(info, current)
			}
			entry, official := dispatchPayload[protocol.PluginCatalogEntry](report.Available.Data)
			if !official {
				return protocol.Command{}, connectionFault(protocol.ErrorCodeInvalidRequest, "expected an official plugin selection")
			}
			output, err = node.agentCommand(ctx, inv, protocol.Command{Kind: "read_catalog_plugin_info", Data: &protocol.CommandReadCatalogPluginInfoPayload{Source: protocol.PluginCatalogSourceOfficial, ID: entry.ID}}, pluginInventoryLost)
			if err != nil {
				return protocol.Command{}, err
			}
			details, detailed := dispatchPayload[protocol.PluginCatalogDetails](output.Data)
			info, selected := dispatchPayload[protocol.PluginInfo](details.Data)
			if !detailed || details.Kind != "plugin" || !selected {
				return protocol.Command{}, connectionFault(protocol.ErrorCodeInvalidRequest, "expected an official plugin selection")
			}
			if (info.Summary.Version == nil) != (entry.Version == nil) || info.Summary.Version != nil && *info.Summary.Version != *entry.Version {
				return protocol.Command{}, connectionFault(protocol.ErrorCodeRevisionConflict, "plugin catalog changed during inspection")
			}
			return candidate(info, current)
		case "uninstall":
			if _, err := installed(ctx, action.Name, action.ExpectedRevision); err != nil {
				return protocol.Command{}, err
			}
		}
		return command(action)
	}
	project := func(ctx context.Context, output protocol.Output) (any, error) {
		switch output.Kind {
		case "plugins":
			entries, _ := dispatchPayload[[]protocol.PluginSummary](output.Data)
			packages := []any{}
			for _, entry := range entries {
				info, found, err := node.agentPlugin(ctx, inv, entry.Name, pluginInventoryLost)
				if err != nil {
					return nil, err
				}
				if found && featurePlugin(info) == nil {
					packages = append(packages, pluginManagerSummary(info))
				}
			}
			return map[string]any{"plugins": packages}, nil
		case "plugin":
			if info, found := dispatchPayload[protocol.PluginInfo](output.Data); found {
				if err := featurePlugin(info); err != nil {
					return nil, err
				}
				return pluginManagerSummary(info), nil
			}
		case "catalog_details":
			if details, found := dispatchPayload[protocol.PluginCatalogDetails](output.Data); found {
				info, selected := dispatchPayload[protocol.PluginInfo](details.Data)
				if details.Kind != "plugin" || !selected {
					return map[string]any{"catalog_details": details}, nil
				}
				if err := featurePlugin(info); err != nil {
					return nil, err
				}
				return pluginManagerSummary(info), nil
			}
		case "plugin_source":
			if selection, found := dispatchPayload[protocol.PluginCatalogSelection](output.Data); found {
				if err := featurePlugin(selection.Info); err != nil {
					return nil, err
				}
				return map[string]any{"source": selection.Source, "path": selection.Path, "plugin": pluginManagerSummary(selection.Info)}, nil
			}
		case "plugin_catalog":
			if page, found := dispatchPayload[protocol.PluginCatalogPage](output.Data); found {
				return map[string]any{"catalog": page}, nil
			}
		case "plugin_updates":
			if batch, found := dispatchPayload[protocol.PluginUpdatesBatch](output.Data); found {
				reports := []any{}
				for _, report := range batch.Reports {
					var available any
					if report.Available != nil {
						if info, packaged := dispatchPayload[protocol.PluginInfo](report.Available.Data); packaged {
							available = pluginManagerSummary(info)
						} else if entry, official := dispatchPayload[protocol.PluginCatalogEntry](report.Available.Data); official {
							available = map[string]any{"catalog": entry, "source": "official"}
						}
					}
					reports = append(reports, map[string]any{"current": report.Current, "available": available, "manual": report.Manual})
				}
				failures := batch.Failures
				if failures == nil {
					failures = []protocol.PluginUpdatesFailure{}
				}
				return map[string]any{"updates": reports, "failures": failures}, nil
			}
		}
		return nil, connectionFault(protocol.ErrorCodeInvalidRequest, "unexpected plugin management result")
	}
	name := map[string]any{"type": "string", "minLength": 1}
	revision := map[string]any{"type": "integer", "minimum": 1}
	catalog := map[string]any{"enum": []any{"official", "third_party"}}
	resolved := objectSchema(map[string]any{"repository": map[string]any{"type": "string"}, "git_ref": map[string]any{"type": "string"}, "commit": map[string]any{"type": "string", "pattern": "^[0-9a-f]{40}$"}}, "repository", "git_ref", "commit")
	git := objectSchema(map[string]any{"repository": map[string]any{"type": "string"}, "git_ref": map[string]any{"type": []any{"string", "null"}}, "path": map[string]any{"type": []any{"string", "null"}}}, "repository")
	flag := map[string]any{"type": "boolean"}
	kind := func(value string) map[string]any { return map[string]any{"const": value} }
	parameters := actionSchema(
		[]string{"list", "search", "inspect", "check_updates", "install", "update", "uninstall"},
		[]map[string]any{
			objectSchema(map[string]any{}),
			objectSchema(map[string]any{"source": catalog, "query": map[string]any{"type": "string"}, "page": map[string]any{"type": "integer", "minimum": 1, "default": 1}, "force": flag}, "source"),
			objectSchema(map[string]any{"target": map[string]any{"oneOf": []any{
				objectSchema(map[string]any{"kind": kind("installed"), "name": name}, "kind", "name"),
				objectSchema(map[string]any{"kind": kind("catalog"), "source": catalog, "id": name, "bundled": flag}, "kind", "source", "id"),
				objectSchema(map[string]any{"kind": kind("git"), "source": git}, "kind", "source"),
			}}}, "target"),
			objectSchema(map[string]any{"packages": map[string]any{"type": "array", "maxItems": maxInstalledPlugins, "items": objectSchema(map[string]any{"name": name, "expected_revision": revision}, "name", "expected_revision")}, "force": flag}, "packages"),
			objectSchema(map[string]any{"name": name, "selection": map[string]any{"oneOf": []any{
				objectSchema(map[string]any{"kind": kind("bundled")}, "kind"),
				objectSchema(map[string]any{"kind": kind("git"), "source": resolved, "path": map[string]any{"type": "string"}}, "kind", "source"),
			}}}, "name", "selection"),
			objectSchema(map[string]any{"name": name, "expected_revision": revision, "force": flag}, "name", "expected_revision"),
			objectSchema(map[string]any{"name": name, "expected_revision": revision}, "name", "expected_revision"),
		})
	return agentTool{
		name:        "manage_plugins",
		description: "Manage feature plugins on this execution Node. Use action=list, search, inspect, check_updates, install, update or uninstall. Search explicitly selects the real official or third_party catalog, not installed-name filtering. Inspect Git sources before installation and use their exact resolved commit/path/name; bundled installation uses the embedded package. Update checks do not install; update explicitly applies only a newer SemVer candidate and preserves existing configuration and enabled choices. Uploaded/directory packages without reusable provenance require a new explicit source selection in the existing package UI. Standalone skills and MCP servers use their own managers. Never provide credentials. Existing turns retain captured resources. Do not replay uncertain mutations. In Code mode this mixed-action tool uses the ordinary confirmation policy for all calls.",
		parameters:  parameters, readOnly: plan, lost: pluginInventoryLost, prepare: prepare, project: project,
	}
}
