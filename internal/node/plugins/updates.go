package plugins

import (
	"context"
	"encoding/json"

	"github.com/Masterminds/semver/v3"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func decodePackageData(input any, target any) error {
	data, err := json.Marshal(input)
	if err != nil {
		return err
	}
	return json.Unmarshal(data, target)
}

func blockedUpdate(info protocol.PluginInfo) *protocol.PluginUpdatesBlocked {
	if info.Summary.Version == nil || info.Summary.Compatibility.Kind != "requires_newer" {
		return nil
	}
	var requirement protocol.PluginCompatibilityRequiresNewerPayload
	if decodePackageData(info.Summary.Compatibility.Data, &requirement) != nil {
		return nil
	}
	return &protocol.PluginUpdatesBlocked{Version: *info.Summary.Version, Required: requirement.Required, ReleaseNotes: info.ReleaseNotes}
}

func (manager *Manager) CheckUpdate(ctx context.Context, current protocol.PluginInfo, worktreeRoot *string, force bool) (protocol.PluginUpdatesReport, error) {
	report := protocol.DefaultPluginUpdatesReport()
	report.Current = current.Summary
	if current.Skill == nil && (current.Summary.Version == nil || !validPackageVersion(*current.Summary.Version)) {
		report.Manual = true
		return report, nil
	}
	if current.Origin == nil {
		report.Manual = true
		return report, nil
	}
	var candidate protocol.PluginInfo
	var err error
	switch current.Origin.Kind {
	case "bundled":
		entries, readErr := manager.OfficialEntries(ctx, force)
		if readErr != nil {
			return report, readErr
		}
		for _, entry := range entries {
			if entry.ID != current.Summary.Name {
				continue
			}
			if IsNewer(current.Summary.Version, entry.Version) {
				if entry.Compatibility.Kind != "compatible" && entry.Version != nil {
					var requirement protocol.PluginCompatibilityRequiresNewerPayload
					if decodePackageData(entry.Compatibility.Data, &requirement) == nil {
						report.Blocked = &protocol.PluginUpdatesBlocked{Version: *entry.Version, Required: requirement.Required, ReleaseNotes: entry.ReleaseNotes}
					}
				} else {
					report.Available = &protocol.PluginUpdatesCandidate{Kind: "official", Data: entry}
				}
			}
			return report, nil
		}
		return report, nil
	case "worktree":
		if worktreeRoot == nil {
			return report, packageFault("plugin source worktree is unavailable")
		}
		var origin protocol.PluginOriginWorktreePayload
		if err := decodePackageData(current.Origin.Data, &origin); err != nil {
			return report, err
		}
		candidate, err = manager.Inspect(ctx, origin.Path, *worktreeRoot, current.Summary.Name)
		candidate.Origin = current.Origin
	case "online":
		var origin protocol.PluginOriginOnlinePayload
		if err := decodePackageData(current.Origin.Data, &origin); err != nil {
			return report, err
		}
		source := protocol.PluginSkillsSource{Repository: origin.Source.Repository, GitRef: &origin.Source.GitRef, Path: &origin.Path}
		if current.Skill != nil {
			candidate, err = manager.inspectRepositorySkill(ctx, source, current.Summary.Name)
		} else {
			selection, inspectErr := manager.InspectRepository(ctx, source)
			err, candidate = inspectErr, selection.Info
		}
	case "registry":
		var origin protocol.PluginOriginRegistryPayload
		if err := decodePackageData(current.Origin.Data, &origin); err != nil {
			return report, err
		}
		source := origin.Source
		resolved, files, acquireErr := manager.acquireRegistry(ctx, protocol.PluginSkillsRegistrySource{Namespace: &source.Namespace, Slug: source.Slug}, nil)
		if acquireErr != nil {
			return report, acquireErr
		}
		if !IsNewer(&source.Version, &resolved.Version) {
			before, beforeErr := semver.StrictNewVersion(source.Version)
			after, afterErr := semver.StrictNewVersion(resolved.Version)
			if beforeErr == nil && afterErr == nil && before.Compare(after) == 0 && source != resolved {
				return report, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "published skill version changed"}
			}
			return report, nil
		}
		document, normalizeErr := normalizeRegistrySkill(files["SKILL.md"], resolved)
		if normalizeErr != nil {
			return report, normalizeErr
		}
		skill, parseErr := ParseSkill(document, resolved.Slug)
		if parseErr != nil {
			return report, parseErr
		}
		original := files["SKILL.md"]
		files["SKILL.md"] = document
		entries, entriesErr := standaloneSkillEntries(files, skill, current.Summary.Name, &resolved.Version, "https://skillhub.cn/skills/"+resolved.Namespace+"/"+resolved.Slug)
		if entriesErr != nil {
			return report, entriesErr
		}
		entries = append(entries, packageEntry{name: "SOURCE-SKILL.md", data: original})
		sortPackageEntries(entries)
		candidate, err = describePackage(entries, nil, current.Summary.Name)
		candidate.Origin = &protocol.PluginOrigin{Kind: "registry", Data: protocol.PluginOriginRegistryPayload{Source: resolved}}
		candidate.Skill = &protocol.PluginSkillsProvenance{Kind: "Registry", Data: protocol.PluginSkillsProvenanceRegistryPayload{Registry: resolved}}
	default:
		report.Manual = true
		return report, nil
	}
	if err != nil {
		return report, err
	}
	if candidate.Summary.Name != current.Summary.Name {
		return report, packageFault("plugin update has a different identity")
	}
	if current.Skill == nil && !IsNewer(current.Summary.Version, candidate.Summary.Version) || current.Skill != nil && candidate.Summary.Digest == current.Summary.Digest {
		return report, nil
	}
	report.Blocked = blockedUpdate(candidate)
	if report.Blocked == nil {
		report.Available = &protocol.PluginUpdatesCandidate{Kind: "package", Data: candidate}
	}
	return report, nil
}

func validPackageVersion(value string) bool {
	_, err := semver.StrictNewVersion(value)
	return err == nil
}
