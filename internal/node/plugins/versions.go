package plugins

import (
	"github.com/Masterminds/semver/v3"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func CatalogVersionCompatibility(version protocol.PluginCatalogVersion, runtimes ...Runtime) protocol.PluginCompatibility {
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
		return requirementsCompatibility(&protocol.PluginRequirements{APIVersion: *version.APIVersion, MinSailryVersion: version.MinSailryVersion,
			Products: version.Products, RequiresModules: version.RequiresModules}, runtimes...)
	}
	if compatibility := Compatibility(version.MinSailryVersion); compatibility.Kind != "compatible" {
		return compatibility
	}
	return runtimeCompatibility(version.Products, version.RequiresModules, runtimes)
}

// SelectCompatibleVersion keeps catalog presentation independent from the
// installed Node's candidate. Prereleases enter automatic selection only when
// the installed package already follows a prerelease channel.
func SelectCompatibleVersion(entry protocol.PluginCatalogEntry, installed *string, runtimes ...Runtime) *protocol.PluginCatalogVersion {
	allowPrerelease := packagePrerelease(installed)
	var selected *protocol.PluginCatalogVersion
	var highest *semver.Version
	for _, candidate := range entry.Versions {
		candidate = withLegacyOfficialRequirements(entry.ID, candidate, entry.Repository)
		candidate.Compatibility = CatalogVersionCompatibility(candidate, runtimes...)
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

func packagePrerelease(version *string) bool {
	if version == nil {
		return false
	}
	parsed, err := semver.StrictNewVersion(*version)
	return err == nil && parsed.Prerelease() != ""
}

func manifestVersion(manifest Manifest, commit string, notes *protocol.PluginCatalogReleaseNotes, runtimes ...Runtime) protocol.PluginCatalogVersion {
	version := protocol.DefaultPluginCatalogVersion()
	if manifest.Version != nil {
		version.Version = *manifest.Version
	}
	version.Commit, version.ReleaseNotes = commit, notes
	if requirements := manifest.Requirements; requirements != nil {
		version.APIVersion, version.MinSailryVersion = &requirements.APIVersion, requirements.MinSailryVersion
		version.Products, version.RequiresModules = requirements.Products, requirements.RequiresModules
	}
	version.Compatibility = CatalogVersionCompatibility(version, runtimes...)
	if compatibility := requirementsCompatibility(manifest.Requirements, runtimes...); compatibility.Kind != "compatible" {
		version.Compatibility = compatibility
	}
	return version
}
