package plugins

import (
	"github.com/Masterminds/semver/v3"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func CatalogVersionCompatibility(version protocol.PluginCatalogVersion) protocol.PluginCompatibility {
	if version.Withdrawn {
		return unsupportedCompatibility("plugin version is withdrawn")
	}
	if !validPackageVersion(version.Version) {
		return unsupportedCompatibility("invalid plugin version")
	}
	if version.Commit != "" && !commitHash(version.Commit) {
		return unsupportedCompatibility("invalid plugin source revision")
	}
	if version.APIVersion != nil {
		return requirementsCompatibility(&protocol.PluginRequirements{APIVersion: *version.APIVersion, MinSailryVersion: version.MinSailryVersion})
	}
	return Compatibility(version.MinSailryVersion)
}

// SelectCompatibleVersion keeps catalog presentation independent from the
// installed Node's candidate. Prereleases enter automatic selection only when
// the installed package already follows a prerelease channel.
func SelectCompatibleVersion(entry protocol.PluginCatalogEntry, installed *string) *protocol.PluginCatalogVersion {
	allowPrerelease := false
	if installed != nil {
		if parsed, err := semver.StrictNewVersion(*installed); err == nil {
			allowPrerelease = parsed.Prerelease() != ""
		}
	}
	var selected *protocol.PluginCatalogVersion
	var highest *semver.Version
	for _, candidate := range entry.Versions {
		candidate.Compatibility = CatalogVersionCompatibility(candidate)
		if candidate.Compatibility.Kind != "compatible" ||
			!commitHash(candidate.Commit) && !(entry.Bundled && entry.Commit == nil && entry.Version != nil && candidate.Version == *entry.Version && candidate.Commit == "") ||
			entry.Version != nil && candidate.Version == *entry.Version && entry.Compatibility.Kind != "compatible" {
			continue
		}
		parsed, err := semver.StrictNewVersion(candidate.Version)
		if err != nil || !allowPrerelease && parsed.Prerelease() != "" {
			continue
		}
		if highest == nil || parsed.Compare(highest) > 0 {
			highest, selected = parsed, &candidate
		}
	}
	return selected
}

func manifestVersion(manifest Manifest, commit string, notes *protocol.PluginCatalogReleaseNotes) protocol.PluginCatalogVersion {
	version := protocol.DefaultPluginCatalogVersion()
	if manifest.Version != nil {
		version.Version = *manifest.Version
	}
	version.Commit, version.ReleaseNotes = commit, notes
	if requirements := manifest.Requirements; requirements != nil {
		version.APIVersion, version.MinSailryVersion = &requirements.APIVersion, requirements.MinSailryVersion
	}
	version.Compatibility = CatalogVersionCompatibility(version)
	if compatibility := requirementsCompatibility(manifest.Requirements); compatibility.Kind != "compatible" {
		version.Compatibility = compatibility
	}
	return version
}
