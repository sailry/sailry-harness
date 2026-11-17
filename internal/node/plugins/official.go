package plugins

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func (manager *Manager) officialRepositoryURL() string {
	if manager.officialRepository != "" {
		return manager.officialRepository
	}
	return OfficialRepository + ".git"
}

func officialVersion(entry protocol.PluginCatalogEntry, requested *string) (*protocol.PluginCatalogVersion, error) {
	if requested == nil {
		if selected := SelectCompatibleVersion(entry, nil); selected != nil {
			return selected, nil
		}
		if err := EnsureCompatible(entry.Compatibility); err != nil {
			return nil, err
		}
		return nil, packageFault("no compatible official plugin version is available")
	}
	if !validPackageVersion(*requested) {
		return nil, packageFault("invalid official plugin version")
	}
	for _, version := range entry.Versions {
		if version.Version != *requested {
			continue
		}
		version.Compatibility = CatalogVersionCompatibility(version)
		if entry.Version != nil && version.Version == *entry.Version && entry.Compatibility.Kind != "compatible" {
			version.Compatibility = entry.Compatibility
		}
		if version.Withdrawn {
			return nil, packageFault("official plugin version is withdrawn")
		}
		if err := EnsureCompatible(version.Compatibility); err != nil {
			return nil, err
		}
		if !commitHash(version.Commit) && !(entry.Bundled && entry.Commit == nil && entry.Version != nil && version.Version == *entry.Version && version.Commit == "") {
			return nil, packageFault("official plugin version has no captured revision")
		}
		return &version, nil
	}
	return nil, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "official plugin version is unavailable"}
}

func (manager *Manager) officialEntry(ctx context.Context, name string, force bool) (protocol.PluginCatalogEntry, error) {
	if !ValidateName(name) {
		return protocol.PluginCatalogEntry{}, packageFault("invalid official plugin name")
	}
	entries, err := manager.OfficialEntries(ctx, force)
	if err != nil {
		return protocol.PluginCatalogEntry{}, err
	}
	for _, entry := range entries {
		if entry.ID == name {
			return entry, nil
		}
	}
	return protocol.PluginCatalogEntry{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "official plugin is unavailable"}
}

func applyOfficialMetadata(info *protocol.PluginInfo, entry protocol.PluginCatalogEntry, version protocol.PluginCatalogVersion) {
	info.UpdateSource = &protocol.PluginOrigin{Kind: "official"}
	info.Versions = entry.Versions
	if version.ReleaseNotes != nil {
		info.ReleaseNotes = version.ReleaseNotes
	}
}

func (manager *Manager) InspectOfficial(ctx context.Context, name string, version *string, force bool) (protocol.PluginCatalogSelection, error) {
	entry, err := manager.officialEntry(ctx, name, force)
	if err != nil {
		return protocol.PluginCatalogSelection{}, err
	}
	selected, err := officialVersion(entry, version)
	if err != nil {
		return protocol.PluginCatalogSelection{}, err
	}
	var selection protocol.PluginCatalogSelection
	if selected.Commit == "" {
		selection.Info, err = manager.InspectBundled(ctx, name)
	} else {
		selection, err = manager.InspectRepository(ctx, protocol.PluginSkillsSource{Repository: manager.officialRepositoryURL(), GitRef: &selected.Commit, Path: &name})
	}
	if err != nil {
		return protocol.PluginCatalogSelection{}, err
	}
	if selection.Info.Summary.Name != name || selection.Info.Summary.Version == nil || *selection.Info.Summary.Version != selected.Version || selection.Source.Commit != selected.Commit {
		return protocol.PluginCatalogSelection{}, packageFault("official plugin identity differs from its catalog version")
	}
	if err := EnsureCompatible(InfoCompatibility(selection.Info)); err != nil {
		return protocol.PluginCatalogSelection{}, err
	}
	applyOfficialMetadata(&selection.Info, entry, *selected)
	return selection, nil
}

func officialSelectionChanged() error {
	return &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "official plugin selection changed"}
}

// InstallOfficial rechecks the catalog at the installation boundary. A reviewed
// selection cannot install a withdrawn or reassigned version from cached bytes.
func (manager *Manager) InstallOfficial(ctx context.Context, selection protocol.PluginCatalogSelection) (protocol.PluginInfo, error) {
	name, version := selection.Info.Summary.Name, selection.Info.Summary.Version
	if !ValidateName(name) || version == nil || !validPackageVersion(*version) || selection.Info.UpdateSource == nil || selection.Info.UpdateSource.Kind != "official" || selection.Info.Origin == nil {
		return protocol.PluginInfo{}, packageFault("invalid official plugin selection")
	}
	if selection.Info.Origin.Kind == "bundled" {
		if selection.Source != (protocol.PluginSkillsResolved{}) || selection.Path != "" {
			return protocol.PluginInfo{}, packageFault("invalid bundled plugin selection")
		}
		entries, issues, err := bundledCapture(ctx, name)
		if err != nil {
			return protocol.PluginInfo{}, err
		}
		inspected, err := describePackage(entries, issues, name)
		if err != nil {
			return protocol.PluginInfo{}, err
		}
		if !sameOptionalString(inspected.Summary.Version, version) || inspected.Summary.Digest != selection.Info.Summary.Digest {
			return protocol.PluginInfo{}, officialSelectionChanged()
		}
		return manager.publish(ctx, entries, issues, name, protocol.PluginOrigin{Kind: "bundled"})
	}
	if selection.Info.Origin.Kind != "online" || selection.Source.Repository != manager.officialRepositoryURL() || selection.Source.GitRef != selection.Source.Commit || !commitHash(selection.Source.Commit) || selection.Path != name {
		return protocol.PluginInfo{}, packageFault("invalid official plugin repository selection")
	}
	var origin protocol.PluginOriginOnlinePayload
	if decodePackageData(selection.Info.Origin.Data, &origin) != nil || origin.Source != selection.Source || origin.Path != name {
		return protocol.PluginInfo{}, packageFault("official plugin origin differs from its selection")
	}
	entry, err := manager.officialEntry(ctx, name, true)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	selected, err := officialVersion(entry, version)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	if selected.Commit != selection.Source.Commit {
		return protocol.PluginInfo{}, officialSelectionChanged()
	}
	info, err := manager.InstallRepository(ctx, selection)
	if err != nil {
		return info, err
	}
	applyOfficialMetadata(&info, entry, *selected)
	return info, nil
}
