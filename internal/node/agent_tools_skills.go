package node

import (
	"context"
	"encoding/json"

	"github.com/Masterminds/semver/v3"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const skillInventoryLost = "skill inventory response is unavailable"

// skillManager declares manage_skills over the installed inventory, skill
// discovery and the durable standalone skill installers.
func (node *Node) skillManager(inv *store.AgentInvocation) agentTool {
	plan := inv.Turn.Config.Mode == protocol.WorkModePlan
	invalid := func(message string) error { return connectionFault(protocol.ErrorCodeInvalidRequest, message) }
	standalone := func(ctx context.Context, name string, revision uint64) (protocol.PluginInfo, error) {
		info, found, err := node.agentPlugin(ctx, inv, name, skillInventoryLost)
		if err != nil {
			return info, err
		}
		if !found {
			return info, invalid("expected a skill package")
		}
		if info.Skill == nil {
			return info, connectionFault(protocol.ErrorCodePermissionDenied, "only standalone skill packages can be managed by this tool")
		}
		if info.Summary.Revision != revision {
			return info, connectionFault(protocol.ErrorCodeRevisionConflict, "skill changed; reload its revision")
		}
		return info, nil
	}
	discover := func(arguments json.RawMessage) (protocol.Command, error) {
		var input struct {
			Repository *string                              `json:"repository"`
			Ref        *string                              `json:"ref"`
			Path       *string                              `json:"path"`
			Registry   *protocol.PluginSkillsRegistrySource `json:"registry"`
		}
		if err := agentArguments(arguments, &input); err != nil {
			return protocol.Command{}, err
		}
		switch {
		case input.Repository != nil && input.Registry == nil:
			source, err := plugins.RegistrySourceURL(*input.Repository)
			if err != nil {
				return protocol.Command{}, err
			}
			if source == nil {
				return protocol.Command{Kind: "discover_skills", Data: &protocol.CommandDiscoverSkillsPayload{Source: protocol.PluginSkillsSource{Repository: *input.Repository, GitRef: input.Ref, Path: input.Path}}}, nil
			}
			if input.Ref != nil || input.Path != nil {
				return protocol.Command{}, invalid("registry links do not take a Git ref or path")
			}
			return protocol.Command{Kind: "discover_registry_skill", Data: &protocol.CommandDiscoverRegistrySkillPayload{Source: *source}}, nil
		case input.Registry != nil && input.Repository == nil && input.Ref == nil && input.Path == nil:
			return protocol.Command{Kind: "discover_registry_skill", Data: &protocol.CommandDiscoverRegistrySkillPayload{Source: *input.Registry}}, nil
		case input.Path != nil && input.Repository == nil && input.Registry == nil && input.Ref == nil:
			return protocol.Command{Kind: "discover_local_skills", Data: &protocol.CommandDiscoverLocalSkillsPayload{Worktree: inv.Run.Worktree, Path: *input.Path}}, nil
		}
		return protocol.Command{}, invalid("invalid tool arguments: data did not match any variant of untagged enum Discover")
	}
	// install maps a discovered source onto its installer. The source kinds
	// have disjoint required fields.
	install := func(arguments json.RawMessage, update bool) (protocol.Command, error) {
		var input struct {
			Source           json.RawMessage `json:"source"`
			Stream           *string         `json:"stream"`
			Path             string          `json:"path"`
			Name             string          `json:"name"`
			ExpectedRevision uint64          `json:"expected_revision"`
		}
		if err := agentArguments(arguments, &input); err != nil {
			return protocol.Command{}, err
		}
		if update && input.ExpectedRevision == 0 {
			return protocol.Command{}, invalid("update requires an installed skill revision")
		}
		var shape map[string]json.RawMessage
		if err := agentArguments(input.Source, &shape); err != nil {
			return protocol.Command{}, err
		}
		_, git := shape["commit"]
		_, registry := shape["slug"]
		_, imported := shape["digest"]
		if input.Stream != nil && !imported {
			return protocol.Command{}, invalid("only imported skills use an upload stream")
		}
		switch {
		case git:
			var source protocol.PluginSkillsResolved
			if err := agentArguments(input.Source, &source); err != nil {
				return protocol.Command{}, err
			}
			return protocol.Command{Kind: "install_skill", Data: &protocol.CommandInstallSkillPayload{Source: source, Path: input.Path, Name: input.Name, ExpectedRevision: input.ExpectedRevision}}, nil
		case registry:
			var source protocol.PluginSkillsRegistryResolved
			if err := agentArguments(input.Source, &source); err != nil {
				return protocol.Command{}, err
			}
			if input.Path != "" {
				return protocol.Command{}, invalid("registry skills do not have a Git directory")
			}
			return protocol.Command{Kind: "install_registry_skill", Data: &protocol.CommandInstallRegistrySkillPayload{Source: source, Name: input.Name, ExpectedRevision: input.ExpectedRevision}}, nil
		case imported:
			var source protocol.PluginSkillsImported
			if err := agentArguments(input.Source, &source); err != nil {
				return protocol.Command{}, err
			}
			if input.Path != "" && input.Path != source.Path {
				return protocol.Command{}, invalid("skill path does not match the imported directory")
			}
			if input.Stream == nil {
				return protocol.Command{}, invalid("rediscover the imported skill to obtain its current upload stream")
			}
			stream, err := protocol.ParseStreamID(*input.Stream)
			if err != nil {
				return protocol.Command{}, invalid("invalid tool arguments: " + err.Error())
			}
			return protocol.Command{Kind: "install_skill_upload", Data: &protocol.CommandInstallSkillUploadPayload{Stream: stream, Source: source, Name: input.Name, ExpectedRevision: input.ExpectedRevision}}, nil
		}
		return protocol.Command{}, invalid("invalid tool arguments: data did not match any variant of untagged enum InstallSource")
	}
	prepare := func(ctx context.Context, arguments json.RawMessage) (protocol.Command, error) {
		var fields map[string]json.RawMessage
		if err := agentArguments(arguments, &fields); err != nil {
			return protocol.Command{}, err
		}
		selected, present := fields["action"]
		if !present {
			return protocol.Command{}, invalid("select a skill management action")
		}
		var action string
		if err := agentArguments(selected, &action); err != nil {
			return protocol.Command{}, err
		}
		delete(fields, "action")
		rest, err := json.Marshal(fields)
		if err != nil {
			return protocol.Command{}, err
		}
		if plan && action != "list" && action != "discover" {
			return protocol.Command{}, connectionFault(protocol.ErrorCodePermissionDenied, "skill installation changes are unavailable in Plan mode")
		}
		switch action {
		case "list":
			return protocol.Command{Kind: "list_plugins"}, nil
		case "discover":
			return discover(rest)
		case "install":
			return install(rest, false)
		case "update":
			command, err := install(rest, true)
			if err != nil {
				return protocol.Command{}, err
			}
			kind := invalid("skill update must preserve its source kind")
			switch target := command.Data.(type) {
			case *protocol.CommandInstallSkillPayload:
				info, err := standalone(ctx, target.Name, target.ExpectedRevision)
				if err != nil {
					return protocol.Command{}, err
				}
				previous, git := dispatchPayload[protocol.PluginSkillsGitProvenance](info.Skill.Data)
				if !git {
					return protocol.Command{}, kind
				}
				if previous.Source.Repository != target.Source.Repository || previous.Source.GitRef != target.Source.GitRef || previous.Path != target.Path {
					return protocol.Command{}, invalid("skill update must preserve its source identity")
				}
			case *protocol.CommandInstallRegistrySkillPayload:
				info, err := standalone(ctx, target.Name, target.ExpectedRevision)
				if err != nil {
					return protocol.Command{}, err
				}
				previous, registry := dispatchPayload[protocol.PluginSkillsProvenanceRegistryPayload](info.Skill.Data)
				if !registry {
					return protocol.Command{}, kind
				}
				next, err := semver.StrictNewVersion(target.Source.Version)
				if err != nil {
					return protocol.Command{}, invalid("invalid skill version")
				}
				current, err := semver.StrictNewVersion(previous.Registry.Version)
				if err != nil {
					return protocol.Command{}, invalid("invalid installed skill version")
				}
				if previous.Registry.Namespace != target.Source.Namespace || previous.Registry.Slug != target.Source.Slug || next.Compare(current) <= 0 {
					return protocol.Command{}, invalid("skill update requires the same coordinate and a newer version")
				}
			case *protocol.CommandInstallSkillUploadPayload:
				info, err := standalone(ctx, target.Name, target.ExpectedRevision)
				if err != nil {
					return protocol.Command{}, err
				}
				previous, imported := dispatchPayload[protocol.PluginSkillsProvenanceImportedPayload](info.Skill.Data)
				if !imported {
					return protocol.Command{}, kind
				}
				if previous.Imported.Path != target.Source.Path {
					return protocol.Command{}, invalid("skill reimport must preserve its directory identity")
				}
			}
			return command, nil
		case "uninstall":
			var input struct {
				Name             string `json:"name"`
				ExpectedRevision uint64 `json:"expected_revision"`
			}
			if err := agentArguments(rest, &input); err != nil {
				return protocol.Command{}, err
			}
			if input.ExpectedRevision == 0 {
				return protocol.Command{}, invalid("uninstall requires an installed skill revision")
			}
			if _, err := standalone(ctx, input.Name, input.ExpectedRevision); err != nil {
				return protocol.Command{}, err
			}
			return protocol.Command{Kind: "remove_plugin", Data: &protocol.CommandRemovePluginPayload{Name: input.Name, ExpectedRevision: input.ExpectedRevision}}, nil
		}
		return protocol.Command{}, invalid("invalid tool arguments: unknown skill action")
	}
	// A list and a removal both return the remaining standalone skill inventory.
	project := func(ctx context.Context, output protocol.Output) (any, error) {
		switch output.Kind {
		case "plugins":
			entries, _ := dispatchPayload[[]protocol.PluginSummary](output.Data)
			skills := []any{}
			for _, entry := range entries {
				info, found, err := node.agentPlugin(ctx, inv, entry.Name, skillInventoryLost)
				if err != nil {
					return nil, err
				}
				if !found {
					return nil, invalid("expected a skill package")
				}
				if info.Skill == nil {
					continue
				}
				value := map[string]any{"name": info.Summary.Name, "revision": info.Summary.Revision, "enabled": info.Summary.Enabled, "skills": info.Skills}
				if source, git := dispatchPayload[protocol.PluginSkillsGitProvenance](info.Skill.Data); git {
					value["source"], value["path"] = source.Source, source.Path
				} else if source, registry := dispatchPayload[protocol.PluginSkillsProvenanceRegistryPayload](info.Skill.Data); registry {
					value["source"] = source.Registry
				} else if source, imported := dispatchPayload[protocol.PluginSkillsProvenanceImportedPayload](info.Skill.Data); imported {
					value["source"] = source.Imported
				}
				skills = append(skills, value)
			}
			return map[string]any{"skills": skills}, nil
		case "skill_discovery", "skill_import_discovery", "registry_skill_discovery":
			return output.Data, nil
		case "plugin":
			if info, found := dispatchPayload[protocol.PluginInfo](output.Data); found {
				return map[string]any{"name": info.Summary.Name, "revision": info.Summary.Revision, "enabled": info.Summary.Enabled, "source": info.Skill, "skills": info.Skills}, nil
			}
		}
		return nil, invalid("unexpected skill management result")
	}
	text := map[string]any{"type": "string"}
	optional := map[string]any{"type": []any{"string", "null"}}
	described := func(kind, description string) map[string]any {
		return map[string]any{"type": kind, "description": description}
	}
	commit := described("string", "Exact resolved commit from discovery")
	commit["pattern"] = "^[0-9a-f]{40}$"
	source := map[string]any{"oneOf": []any{
		objectSchema(map[string]any{"repository": described("string", "Canonical repository from discovery, without credentials"), "git_ref": described("string", "Git ref from discovery"), "commit": commit}, "repository", "git_ref", "commit"),
		objectSchema(map[string]any{"namespace": text, "slug": text, "version": text, "version_id": text, "content_hash": text, "package_md5": text, "archive_sha256": text},
			"namespace", "slug", "version", "version_id", "content_hash", "package_md5", "archive_sha256"),
		objectSchema(map[string]any{"digest": text, "path": text, "kind": map[string]any{"enum": []any{"directory", "archive"}}}, "digest", "path", "kind"),
	}}
	stream := described("string", "Current discovery stream; required only for directory/archive imports")
	name := described("string", "Stable standalone package name from discovery or manage_skills list")
	revision := described("integer", "Current installed revision from manage_skills list")
	revision["minimum"] = 1
	required := func(names ...any) map[string]any { return map[string]any{"required": names} }
	discovery := objectSchema(map[string]any{"repository": text, "ref": optional, "path": optional,
		"registry": objectSchema(map[string]any{"namespace": optional, "slug": text, "version": optional}, "slug")})
	discovery["oneOf"] = []any{
		map[string]any{"required": []any{"repository"}, "not": required("registry")},
		map[string]any{"required": []any{"registry"}, "not": map[string]any{"anyOf": []any{required("repository"), required("ref"), required("path")}}},
		map[string]any{"required": []any{"path"}, "not": map[string]any{"anyOf": []any{required("repository"), required("registry"), required("ref")}}},
	}
	parameters := actionSchema([]string{"list", "discover", "install", "update", "uninstall"}, []map[string]any{
		objectSchema(map[string]any{}),
		discovery,
		objectSchema(map[string]any{"source": source, "stream": stream, "path": text, "name": name}, "source", "name"),
		objectSchema(map[string]any{"source": source, "stream": stream, "path": text, "name": name, "expected_revision": revision}, "source", "name", "expected_revision"),
		objectSchema(map[string]any{"name": name, "expected_revision": revision}, "name", "expected_revision"),
	})
	return agentTool{
		name:        "manage_skills",
		description: "Manage standalone skills on this execution Node. Actions: list, discover, install, update, uninstall. Discover accepts Git/skills.sh/Skillhub detail links, registry coordinates, or an execution-Node relative directory/archive path. Discovery does not install or execute source content. Use its exact source, name/path and import stream; update/uninstall require the current installed revision. Registry updates require a newer semantic version, imports require rediscovery. Changes affect new selections, never this turn's frozen resources. Never supply credentials, substitute a branch for a commit, or replay uncertain writes. Plan permits list/discover only.",
		parameters:  parameters, readOnly: plan, lost: skillInventoryLost, prepare: prepare, project: project,
	}
}
