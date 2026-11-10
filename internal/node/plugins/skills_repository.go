package plugins

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"net/url"
	"path"
	"slices"
	"strings"

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
	executable  map[string]bool
	directories []string
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
	selected, err := ParseRepositorySource(source)
	if err != nil {
		return protocol.PluginSkillsDiscovery{}, err
	}
	resolved, entries, issues, err := acquireRepository(ctx, selected, "", repositoryCaptureOptions{maximum: 10000})
	if err != nil {
		return protocol.PluginSkillsDiscovery{}, err
	}
	candidates := []protocol.PluginSkillsCandidate{}
	selectedDirectories := []string{}
	for _, entry := range entries {
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
		for _, issue := range issues {
			if issue.Path == directory || strings.HasPrefix(directory, issue.Path+"/") {
				return protocol.PluginSkillsDiscovery{}, packageFault("selected skill directory is a symbolic link")
			}
		}
		skill, err := ParseSkill(entry.data, "")
		if err != nil {
			return protocol.PluginSkillsDiscovery{}, err
		}
		fullPath := path.Join(selected.Path, directory)
		if fullPath == "." {
			fullPath = ""
		}
		if selectedSkill == "" || selectedSkill == skill.Name {
			candidates = append(candidates, protocol.PluginSkillsCandidate{Name: StandaloneSkillName(resolved.Repository, fullPath), Path: fullPath, Skill: skill})
		}
		selectedDirectories = append(selectedDirectories, directory)
		if len(candidates) > MaxSkills {
			return protocol.PluginSkillsDiscovery{}, packageFault("skill repository has too many skills")
		}
	}
	if selectedSkill != "" && len(candidates) != 1 {
		return protocol.PluginSkillsDiscovery{}, packageFault("skills.sh source did not select exactly one skill")
	}
	return protocol.PluginSkillsDiscovery{Source: resolved, Skills: candidates}, nil
}

func (manager *Manager) InstallRepositorySkill(ctx context.Context, source protocol.PluginSkillsResolved, directory, name string) (protocol.PluginInfo, error) {
	if !commitHash(source.Commit) || name != StandaloneSkillName(source.Repository, directory) {
		return protocol.PluginInfo{}, packageFault("skill identity does not match its source")
	}
	selected, err := ParseRepositorySource(protocol.PluginSkillsSource{Repository: source.Repository, GitRef: &source.GitRef, Path: &directory})
	if err != nil || selected.Repository != source.Repository {
		return protocol.PluginInfo{}, packageFault("invalid immutable skill repository source")
	}
	entries, err := repositorySkillPackage(ctx, selected, source, name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	info, err := manager.publish(ctx, entries, nil, name, protocol.PluginOrigin{Kind: "online", Data: protocol.PluginOriginOnlinePayload{Source: source, Path: directory}})
	if err == nil {
		info.Skill = &protocol.PluginSkillsProvenance{Kind: "Git", Data: protocol.PluginSkillsGitProvenance{Source: source, Path: directory}}
	}
	return info, err
}

func repositorySkillPackage(ctx context.Context, selected RepositorySelection, source protocol.PluginSkillsResolved, name string) ([]packageEntry, error) {
	var licenses []packageEntry
	_, captured, issues, err := acquireRepository(ctx, selected, source.Commit, repositoryCaptureOptions{maximum: 10000, licenses: &licenses})
	if err != nil {
		return nil, err
	}
	return normalizeRepositorySkill(captured, issues, licenses, source, name)
}

func normalizeRepositorySkill(captured []packageEntry, issues []protocol.PluginIssue, licenses []packageEntry, source protocol.PluginSkillsResolved, name string) ([]packageEntry, error) {
	if len(issues) != 0 {
		return nil, packageFault("selected skill contains unsupported files")
	}
	files := map[string][]byte{}
	metadata := skillEntryMetadata{executable: map[string]bool{}}
	for _, entry := range captured {
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
	entries, err := standaloneSkillEntries(files, skill, name, &source.Commit, source.Repository, metadata)
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
	resolved, captured, issues, err := acquireRepository(ctx, selected, "", repositoryCaptureOptions{maximum: 10000, licenses: &licenses})
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	entries, err := normalizeRepositorySkill(captured, issues, licenses, resolved, name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	info, err := describePackage(entries, nil, name)
	if err == nil {
		info.Origin = &protocol.PluginOrigin{Kind: "online", Data: protocol.PluginOriginOnlinePayload{Source: resolved, Path: selected.Path}}
		info.Skill = &protocol.PluginSkillsProvenance{Kind: "Git", Data: protocol.PluginSkillsGitProvenance{Source: resolved, Path: selected.Path}}
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
