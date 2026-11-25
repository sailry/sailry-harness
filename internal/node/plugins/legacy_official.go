package plugins

import (
	"context"
	"io/fs"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type legacyOfficialRelease struct {
	name, version, commit, digest, bundledDigest string
}

var legacyOfficialDigests = func() map[string]*legacyOfficialRelease {
	result := make(map[string]*legacyOfficialRelease, len(legacyOfficialReleases))
	for index := range legacyOfficialReleases {
		release := &legacyOfficialReleases[index]
		result[release.digest], result[release.bundledDigest] = release, release
	}
	return result
}()

var legacyOfficialCommits = func() map[[3]string]string {
	result := make(map[[3]string]string, len(legacyOfficialReleases))
	for _, release := range legacyOfficialReleases {
		result[[3]string{release.name, release.version, release.commit}] = release.name
	}
	return result
}()

var legacyOfficialVersions = func() map[[2]string]string {
	result := make(map[[2]string]string, len(legacyOfficialReleases))
	for _, release := range legacyOfficialReleases {
		result[[2]string{release.name, release.version}] = release.name
	}
	return result
}()

// IsLegacyOfficialPackage checks the immutable content identity of a known
// official release, independent of its installation origin and compatibility.
func IsLegacyOfficialPackage(name, digest string) bool {
	release := legacyOfficialDigests[digest]
	return release != nil && release.name == name
}

func legacyOfficialInfoModules(info protocol.PluginInfo) []string {
	if !IsLegacyOfficialPackage(info.Summary.Name, info.Summary.Digest) {
		return nil
	}
	return []string{info.Summary.Name}
}

func withLegacyOfficialRequirements(name string, version protocol.PluginCatalogVersion, repository *string) protocol.PluginCatalogVersion {
	module := legacyOfficialCommits[[3]string{name, version.Version, version.Commit}]
	if repository != nil && (*repository == OfficialRepository || *repository == OfficialRepository+".git") {
		module = legacyOfficialVersions[[2]string{name, version.Version}]
	}
	if module != "" && !slices.Contains(version.RequiresModules, module) {
		version.RequiresModules = append(slices.Clone(version.RequiresModules), module)
	}
	return version
}

func legacyBundledModules(ctx context.Context, files fs.FS, manifest Manifest) ([]string, error) {
	if manifest.Version == nil || legacyOfficialVersions[[2]string{manifest.Name, *manifest.Version}] == "" {
		return nil, nil
	}
	entries, _, err := captureBundled(ctx, files, manifest.Name)
	if err != nil {
		return nil, err
	}
	return legacyOfficialInfoModules(protocol.PluginInfo{Summary: protocol.PluginSummary{
		Name: manifest.Name, Version: manifest.Version, Digest: digestPackage(entries),
	}}), nil
}
