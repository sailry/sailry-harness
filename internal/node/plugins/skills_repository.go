package plugins

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"net/url"
	"path"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func StandaloneSkillName(repository, directory string) string {
	var digest [32]byte
	blake3.DeriveKey(digest[:], "Sailry standalone skill v1", []byte(repository+"\x00"+directory))
	return "skill-" + hex.EncodeToString(digest[:16])
}

func sortPackageEntries(entries []packageEntry) {
	slices.SortFunc(entries, func(left, right packageEntry) int {
		return slices.Compare(strings.Split(left.name, "/"), strings.Split(right.name, "/"))
	})
}

var sourceLicenseNames = []string{"LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING", "NOTICE"}

type skillEntryMetadata struct {
	executable   map[string]bool
	directories  []string
	requirements *protocol.PluginRequirements
}

func standaloneSkillEntries(files map[string][]byte, skill protocol.PluginSkill, name string, version *string, repository string, metadata ...skillEntryMetadata) ([]packageEntry, error) {
	normalized := map[string]packageEntry{}
	total := 0
	for relative, data := range files {
		if !resourcePath(relative) {
			continue
		}
		name := "skills/" + skill.Name + "/" + relative
		if len(strings.Split(name, "/")) > MaxPackageDepth {
			return nil, packageFault("standalone skill exceeds its nesting limit")
		}
		entry := packageEntry{name: name, data: data}
		if len(metadata) > 0 {
			entry.executable = metadata[0].executable[relative]
		}
		normalized[name] = entry
		total += len(data)
		if total > MaxPackageBytes {
			return nil, packageFault("standalone skill exceeds its expanded size limit")
		}
		for directory := path.Dir(name); directory != "."; directory = path.Dir(directory) {
			normalized[directory] = packageEntry{name: directory, directory: true}
		}
	}
	if len(metadata) > 0 {
		for _, relative := range metadata[0].directories {
			directory := "skills/" + skill.Name + "/" + relative
			if !resourcePath(relative) || len(strings.Split(directory, "/")) > MaxPackageDepth {
				return nil, packageFault("standalone skill exceeds its nesting limit")
			}
			normalized[directory] = packageEntry{name: directory, directory: true}
		}
	}
	manifest := map[string]any{"$schema": ManifestSchema, "name": name, "description": skill.Description}
	if version != nil {
		manifest["version"] = *version
	}
	if repository != "" {
		manifest["repository"] = repository
	}
	if len(metadata) > 0 && metadata[0].requirements != nil {
		manifest["extensions"] = map[string]*protocol.PluginRequirements{"dev.sailry.platform": metadata[0].requirements}
	}
	data, err := json.Marshal(manifest)
	if err != nil {
		return nil, err
	}
	normalized["plugin.json"] = packageEntry{name: "plugin.json", data: data}
	if len(normalized) > MaxPackageEntries {
		return nil, packageFault("standalone skill exceeds its entry limit")
	}
	entries := make([]packageEntry, 0, len(normalized))
	for _, entry := range normalized {
		entries = append(entries, entry)
	}
	sortPackageEntries(entries)
	return entries, nil
}

func (manager *Manager) DiscoverRepositorySkills(ctx context.Context, source protocol.PluginSkillsSource) (protocol.PluginSkillsDiscovery, error) {
	selectedSkill := ""
	if address, err := url.Parse(source.Repository); err == nil && address.Scheme == "https" && address.Host == "skills.sh" && address.User == nil && address.RawQuery == "" && address.Fragment == "" {
		parts := strings.Split(strings.Trim(address.Path, "/"), "/")
		if len(parts) != 3 || !registrySegment(parts[0]) || !registrySegment(parts[1]) || !registrySegment(parts[2]) {
			return protocol.PluginSkillsDiscovery{}, packageFault("invalid skills.sh source")
		}
		source.Repository = "https://github.com/" + parts[0] + "/" + parts[1]
		selectedSkill = parts[2]
	}
	return manager.discoverRepositorySkills(ctx, source, selectedSkill)
}

func (manager *Manager) discoverRepositorySkills(ctx context.Context, source protocol.PluginSkillsSource, selectedSkill string) (protocol.PluginSkillsDiscovery, error) {
	selected, err := ParseRepositorySource(source)
	if err != nil {
		return protocol.PluginSkillsDiscovery{}, err
	}
	capture, err := manager.captureRepositorySkill(ctx, selected, "", repositoryCaptureOptions{maximum: 10000})
	if err != nil {
		return protocol.PluginSkillsDiscovery{}, err
	}
	candidates := []protocol.PluginSkillsCandidate{}
	selectedDirectories := []string{}
	for _, entry := range capture.entries {
		if entry.directory || path.Base(entry.name) != "SKILL.md" {
			continue
		}
		directory := path.Dir(entry.name)
		if directory == "." {
			directory = ""
		}
		skip := false
		for _, parent := range selectedDirectories {
			skip = skip || parent == "" || strings.HasPrefix(directory, parent+"/")
		}
		for _, component := range strings.Split(directory, "/") {
			skip = skip || slices.Contains([]string{".git", "node_modules", "target", ".venv"}, component)
		}
		if skip {
			continue
		}
		for _, issue := range capture.issues {
			if issue.Path == directory || strings.HasPrefix(directory, issue.Path+"/") {
				return protocol.PluginSkillsDiscovery{}, packageFault("selected skill directory is a symbolic link")
			}
		}
		fullPath := path.Join(selected.Path, directory)
		if fullPath == "." {
			fullPath = ""
		}
		skill, err := ParseSkill(entry.data, "")
		if err != nil {
			// A catalog slug selects one skill. Keep validation errors for
			// its named directory while continuing past unrelated documents.
			if selectedSkill != "" && path.Base(fullPath) != selectedSkill {
				continue
			}
			return protocol.PluginSkillsDiscovery{}, err
		}
		if selectedSkill == "" || selectedSkill == skill.Name {
			candidates = append(candidates, protocol.PluginSkillsCandidate{Name: StandaloneSkillName(capture.source.Repository, fullPath), Path: fullPath, Skill: skill})
		}
		selectedDirectories = append(selectedDirectories, directory)
		if len(candidates) > MaxSkills {
			return protocol.PluginSkillsDiscovery{}, packageFault("skill repository has too many skills")
		}
	}
	if selectedSkill != "" && len(candidates) != 1 {
		return protocol.PluginSkillsDiscovery{}, packageFault("skills.sh source did not select exactly one skill")
	}
	return protocol.PluginSkillsDiscovery{Source: capture.source, Skills: candidates}, nil
}

func (manager *Manager) InstallRepositorySkill(ctx context.Context, source protocol.PluginSkillsResolved, directory, name string) (protocol.PluginInfo, error) {
	if !commitHash(source.Commit) || name != StandaloneSkillName(source.Repository, directory) {
		return protocol.PluginInfo{}, packageFault("skill identity does not match its source")
	}
	selected, err := ParseRepositorySource(protocol.PluginSkillsSource{Repository: source.Repository, GitRef: &source.GitRef, Path: &directory})
	if err != nil || selected.Repository != source.Repository {
		return protocol.PluginInfo{}, packageFault("invalid immutable skill repository source")
	}
	entries, err := manager.repositorySkillPackage(ctx, selected, source.Commit, name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	immutable := source
	immutable.GitRef = immutable.Commit
	info, err := manager.publish(ctx, entries, nil, name, protocol.PluginOrigin{Kind: "online", Data: protocol.PluginOriginOnlinePayload{Source: immutable, Path: directory}})
	if err == nil {
		setRepositoryOrigins(&info, source, directory)
		info.Skill = &protocol.PluginSkillsProvenance{Kind: "Git", Data: protocol.PluginSkillsGitProvenance{Source: source, Path: directory}}
	}
	return info, err
}

func (manager *Manager) repositorySkillPackage(ctx context.Context, selected RepositorySelection, commit, name string) ([]packageEntry, error) {
	var licenses []packageEntry
	capture, err := manager.captureRepositorySkill(ctx, selected, commit, repositoryCaptureOptions{maximum: 10000, licenses: &licenses})
	if err != nil {
		return nil, err
	}
	return normalizeRepositorySkill(capture, licenses, name)
}

func (manager *Manager) officialSkillPackage(selected RepositorySelection) string {
	if selected.Repository != manager.officialRepositoryURL() {
		return ""
	}
	parts := strings.Split(selected.Path, "/")
	if len(parts) != 3 || parts[1] != "skills" || !ValidateName(parts[0]) || !skillName(parts[2]) {
		return ""
	}
	return parts[0]
}

type repositorySkillCapture struct {
	source       protocol.PluginSkillsResolved
	entries      []packageEntry
	issues       []protocol.PluginIssue
	requirements *protocol.PluginRequirements
}

// The v1 Git selection has no product or package metadata. Recover the release
// from the canonical repository, skill path, and immutable commit even when the
// controller replays a previously captured selection without discovery.
func (manager *Manager) captureRepositorySkill(ctx context.Context, selected RepositorySelection, commit string, options repositoryCaptureOptions) (repositorySkillCapture, error) {
	var capture repositorySkillCapture
	name := manager.officialSkillPackage(selected)
	if name == "" {
		var err error
		capture.source, capture.entries, capture.issues, err = acquireRepository(ctx, selected, commit, options)
		return capture, err
	}
	if slices.Contains(RetiredBundled(), name) {
		return capture, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "official skill has been retired"}
	}
	catalogContext, cancel := context.WithTimeout(ctx, 30*time.Second)
	feed, err := manager.readOfficialCatalog(catalogContext)
	cancel()
	if err != nil {
		return capture, err
	}
	runtime := RuntimeFromContext(ctx)
	entry, err := manager.officialSkillPackageEntry(feed, name, runtime)
	if err != nil {
		return capture, err
	}
	var release *protocol.PluginCatalogVersion
	requested := commit
	if requested == "" {
		requested = selected.GitRef
	}
	if commit == "" && (requested == "HEAD" || requested == "main") {
		release, err = currentSkillRelease(entry, runtime)
	} else if len(entry.Versions) == 0 && requested == feed.Commit {
		release, err = currentSkillRelease(entry, runtime)
	} else if index := slices.IndexFunc(entry.Versions, func(version protocol.PluginCatalogVersion) bool { return version.Commit == requested }); index >= 0 {
		release, err = officialVersion(entry, &entry.Versions[index].Version, runtime)
		if err == nil && release.Commit != requested {
			err = officialSelectionChanged()
		}
	} else {
		err = officialSelectionChanged()
	}
	if err != nil {
		return capture, err
	}
	commit = feed.Commit
	if release != nil {
		commit = release.Commit
	}
	selected.GitRef = commit
	options.manifestPath = name + "/plugin.json"
	options.checkManifest = func(data []byte) error {
		manifest, err := ValidateManifest(data)
		if err != nil {
			return err
		}
		if manifest.Name != name || release != nil && (manifest.Version == nil || *manifest.Version != release.Version) {
			return packageFault("official skill manifest differs from its catalog release")
		}
		if err := EnsureCompatible(requirementsCompatibility(manifest.Requirements, runtime)); err != nil {
			return err
		}
		capture.requirements, err = officialSkillRequirements(release, manifest.Requirements)
		return err
	}
	capture.source, capture.entries, capture.issues, err = acquireRepository(ctx, selected, commit, options)
	return capture, err
}

// Persist only compatibility, not the parent package's executable extension.
// The selected release already includes the version-specific catalog sidecar.
func officialSkillRequirements(release *protocol.PluginCatalogVersion, parent *protocol.PluginRequirements) (*protocol.PluginRequirements, error) {
	if release == nil {
		return parent, nil
	}
	version := *release
	if parent != nil {
		version.APIVersion = &parent.APIVersion
		if version.MinSailryVersion == nil || IsNewer(version.MinSailryVersion, parent.MinSailryVersion) {
			version.MinSailryVersion = parent.MinSailryVersion
		}
		version.RequiresModules = slices.Clone(version.RequiresModules)
		if err := mergeCatalogRequirements(&version, catalogRuntimeRequirements{Products: parent.Products, RequiresModules: parent.RequiresModules}); err != nil {
			return nil, err
		}
	}
	apiVersion := "v1"
	if version.APIVersion != nil {
		apiVersion = *version.APIVersion
	} else if version.MinSailryVersion == nil && len(version.Products) == 0 && len(version.RequiresModules) == 0 {
		return nil, nil
	}
	return &protocol.PluginRequirements{APIVersion: apiVersion, MinSailryVersion: version.MinSailryVersion, Products: version.Products, RequiresModules: version.RequiresModules}, nil
}

func normalizeRepositorySkill(capture repositorySkillCapture, licenses []packageEntry, name string) ([]packageEntry, error) {
	if len(capture.issues) != 0 {
		return nil, packageFault("selected skill contains unsupported files")
	}
	files := map[string][]byte{}
	metadata := skillEntryMetadata{executable: map[string]bool{}, requirements: capture.requirements}
	for _, entry := range capture.entries {
		if !entry.directory {
			files[entry.name] = entry.data
			metadata.executable[entry.name] = entry.executable
		} else {
			metadata.directories = append(metadata.directories, entry.name)
		}
	}
	skill, err := ParseSkill(files["SKILL.md"], "")
	if err != nil {
		return nil, err
	}
	entries, err := standaloneSkillEntries(files, skill, name, &capture.source.Commit, capture.source.Repository, metadata)
	if err != nil {
		return nil, err
	}
	entries = append(entries, licenses...)
	if err := validateSkillPackage(entries); err != nil {
		return nil, err
	}
	sortPackageEntries(entries)
	return entries, nil
}

func (manager *Manager) inspectRepositorySkill(ctx context.Context, source protocol.PluginSkillsSource, name string) (protocol.PluginInfo, error) {
	selected, err := ParseRepositorySource(source)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	if name != StandaloneSkillName(selected.Repository, selected.Path) {
		return protocol.PluginInfo{}, packageFault("skill identity does not match its source")
	}
	var licenses []packageEntry
	capture, err := manager.captureRepositorySkill(ctx, selected, "", repositoryCaptureOptions{maximum: 10000, licenses: &licenses})
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	entries, err := normalizeRepositorySkill(capture, licenses, name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	info, err := describePackage(entries, nil, name)
	if err == nil {
		setRepositoryOrigins(&info, capture.source, selected.Path)
		info.Skill = &protocol.PluginSkillsProvenance{Kind: "Git", Data: protocol.PluginSkillsGitProvenance{Source: capture.source, Path: selected.Path}}
	}
	return info, err
}

func validateSkillPackage(entries []packageEntry) error {
	total := 0
	for _, entry := range entries {
		total += len(entry.data)
	}
	if len(entries) > MaxPackageEntries || total > MaxPackageBytes {
		return packageFault("standalone skill exceeds its package limit")
	}
	return nil
}
