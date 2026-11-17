package plugins

import (
	"context"
	"encoding/json"
	"errors"

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

func (manager *Manager) CheckUpdate(ctx context.Context, current protocol.PluginInfo, worktreeRoot *string, force bool) (protocol.PluginUpdatesReport, error) {
	report := protocol.DefaultPluginUpdatesReport()
	report.Current = current.Summary
	report.Current.Compatibility = InfoCompatibility(current)
	if current.Summary.Pinned {
		return report, nil
	}
	if current.Skill == nil && (current.Summary.Version == nil || !validPackageVersion(*current.Summary.Version)) {
		report.Manual = true
		return report, nil
	}
	source := current.UpdateSource
	if source == nil {
		source = current.Origin
	}
	if manager.isOfficialPackage(current) {
		entry, err := manager.officialEntry(ctx, current.Summary.Name, force)
		if err != nil {
			var fault *protocol.Fault
			if errors.As(err, &fault) && fault.Code == protocol.ErrorCodeNotFound {
				return report, nil
			}
			return report, err
		}
		entry.CompatibleVersion = SelectCompatibleVersion(entry, current.Summary.Version)
		if entry.CompatibleVersion != nil && IsNewer(current.Summary.Version, &entry.CompatibleVersion.Version) {
			report.Available = &protocol.PluginUpdatesCandidate{Kind: "official", Data: entry}
		}
		return report, nil
	}
	if source == nil {
		report.Manual = true
		return report, nil
	}
	var candidate protocol.PluginInfo
	var err error
	switch source.Kind {
	case "worktree":
		if worktreeRoot == nil {
			return report, packageFault("plugin source worktree is unavailable")
		}
		var origin protocol.PluginOriginWorktreePayload
		if err := decodePackageData(source.Data, &origin); err != nil {
			return report, err
		}
		candidate, err = manager.Inspect(ctx, origin.Path, *worktreeRoot, current.Summary.Name)
		candidate.Origin = current.Origin
		candidate.UpdateSource = source
	case "online":
		var origin protocol.PluginOriginOnlinePayload
		if err := decodePackageData(source.Data, &origin); err != nil {
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
		if err := decodePackageData(source.Data, &origin); err != nil {
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
	return AssessUpdate(current, candidate)
}

// AssessUpdate applies this Node's update policy to an already captured package,
// including packages uploaded from a controller's local directory.
func AssessUpdate(current, candidate protocol.PluginInfo) (protocol.PluginUpdatesReport, error) {
	report := protocol.DefaultPluginUpdatesReport()
	report.Current = current.Summary
	report.Current.Compatibility = InfoCompatibility(current)
	if current.Summary.Pinned {
		return report, nil
	}
	if current.Skill == nil && (current.Summary.Version == nil || !validPackageVersion(*current.Summary.Version)) {
		report.Manual = true
		return report, nil
	}
	if candidate.Summary.Name != current.Summary.Name {
		return report, packageFault("plugin update has a different identity")
	}
	if current.Skill == nil && !IsNewer(current.Summary.Version, candidate.Summary.Version) || current.Skill != nil && candidate.Summary.Digest == current.Summary.Digest {
		return report, nil
	}
	if current.Skill == nil && !packagePrerelease(current.Summary.Version) && packagePrerelease(candidate.Summary.Version) {
		return report, nil
	}
	candidate.Summary.Compatibility = InfoCompatibility(candidate)
	if candidate.Summary.Compatibility.Kind == "compatible" {
		report.Available = &protocol.PluginUpdatesCandidate{Kind: "package", Data: candidate}
	}
	return report, nil
}

func (manager *Manager) isOfficialPackage(info protocol.PluginInfo) bool {
	if info.Skill != nil {
		return false
	}
	if info.UpdateSource != nil && info.UpdateSource.Kind == "official" {
		return true
	}
	if info.Origin == nil {
		return false
	}
	if info.Origin.Kind == "bundled" || info.Origin.Kind == "official" {
		return true
	}
	if info.Origin.Kind != "online" {
		return false
	}
	var origin protocol.PluginOriginOnlinePayload
	if decodePackageData(info.Origin.Data, &origin) != nil || origin.Path != info.Summary.Name {
		return false
	}
	selected, err := ParseRepositorySource(protocol.PluginSkillsSource{Repository: origin.Source.Repository})
	return err == nil && selected.Repository == manager.officialRepositoryURL()
}

func validPackageVersion(value string) bool {
	_, err := semver.StrictNewVersion(value)
	return err == nil
}
