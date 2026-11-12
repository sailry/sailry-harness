package plugins

import (
	"context"
	"encoding/json"
	"io"
	"io/fs"
	"net/http"
	"net/url"
	"path"
	"slices"
	"strconv"
	"strings"

	sailry "github.com/sailry/sailry-harness"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const OfficialRepository = "https://github.com/sailry/sailry-plugins"
const OfficialCatalogURL = "https://sailry.dev/plugins/catalog.json"
const ThirdPartyCatalogURL = "https://pluginsmp.com"

type DefaultPackage struct {
	Name    string
	Enabled bool
}

func BundledDefaults() []DefaultPackage {
	return []DefaultPackage{
		{"reminders", true}, {"scheduled-tasks", true}, {"web-search", true}, {"progress", true}, {"goals", true},
		{"commands", true}, {"external-browser", false}, {"browser", true}, {"computer", false},
		{"brief", true}, {"memory", true}, {"files", true}, {"code", true}, {"statistics", true}, {"provider-status", true},
		{"openai-auth", true}, {"claude-auth", true}, {"copilot-auth", true}, {"workbuddy", true}, {"databases", true}, {"ssh", true}, {"worktrees", true},
		{"git", true}, {"context7", true}, {"github", true}, {"code-review", true}, {"office", true},
	}
}

// RetiredBundled names packages the application once bundled and now provides
// itself. Startup removes a bundled installation of one; the removal record
// and the captured versions stay, so earlier turns still resolve.
func RetiredBundled() []string { return []string{"media"} }

func manifestCatalogEntry(manifest Manifest) protocol.PluginCatalogEntry {
	entry := protocol.DefaultPluginCatalogEntry()
	entry.ID, entry.Name, entry.Version, entry.Description = manifest.Name, manifest.Name, manifest.Version, manifest.Description
	entry.Repository = manifest.Repository
	if extension := manifest.Extension; extension != nil {
		entry.Compatibility = Compatibility(extension.MinSailryVersion)
		entry.Category, entry.Tint, entry.Display, entry.Icon = extension.Category, extension.Tint, extension.Display, extension.Icon
		entry.Permissions = extension.Actions
		entry.ProvidesModels = len(extension.ModelProviders) != 0
		if extension.Description != nil {
			entry.DescriptionLocales = extension.Description.Locales
		}
	}
	return entry
}

func BundledEntries(ctx context.Context) ([]protocol.PluginCatalogEntry, error) {
	names, err := fs.Glob(sailry.Plugins, "plugins/*/plugin.json")
	if err != nil {
		return nil, err
	}
	entries := make([]protocol.PluginCatalogEntry, 0, len(names))
	for _, name := range names {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		data, err := fs.ReadFile(sailry.Plugins, name)
		if err != nil {
			return nil, err
		}
		manifest, err := ValidateManifest(data)
		if err != nil {
			return nil, err
		}
		entry := manifestCatalogEntry(manifest)
		if manifest.Extension != nil && manifest.Extension.Icon != nil {
			data, err := fs.ReadFile(sailry.Plugins, path.Join(path.Dir(name), *manifest.Extension.Icon))
			if err == nil {
				entry.Icon = packageIcon(data)
			} else {
				entry.Icon = nil
			}
		}
		entry.Bundled = true
		entries = append(entries, entry)
	}
	return entries, nil
}

func (manager *Manager) catalogJSON(ctx context.Context, endpoint string, output any) error {
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return packageFault("invalid plugin catalog endpoint")
	}
	request.Header.Set("User-Agent", "Sailry")
	request.Header.Set("Accept", "application/json")
	client := manager.Client
	if client == nil {
		client = http.DefaultClient
	}
	response, err := client.Do(request)
	if err != nil {
		return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "plugin catalog is unavailable"}
	}
	defer response.Body.Close()
	if response.StatusCode == http.StatusNotFound {
		return &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "plugin catalog entry is unavailable"}
	}
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "plugin catalog is unavailable"}
	}
	data, err := io.ReadAll(io.LimitReader(response.Body, 8*1024*1024+1))
	if err != nil {
		return err
	}
	if len(data) > 8*1024*1024 || json.Unmarshal(data, output) != nil {
		return packageFault("invalid plugin catalog JSON")
	}
	return nil
}

func (manager *Manager) OfficialEntries(ctx context.Context, force bool) ([]protocol.PluginCatalogEntry, error) {
	var feed struct {
		Version  uint32 `json:"version"`
		Commit   string `json:"source_commit"`
		Packages []struct {
			ID           string                              `json:"id"`
			Manifest     json.RawMessage                     `json:"manifest"`
			ReleaseNotes *protocol.PluginCatalogReleaseNotes `json:"release_notes"`
			Versions     []protocol.PluginCatalogVersion     `json:"versions"`
		} `json:"packages"`
	}
	endpoint := manager.OfficialURL
	if endpoint == "" {
		endpoint = OfficialCatalogURL
	}
	err := manager.catalogJSON(ctx, endpoint, &feed)
	bundled, bundledErr := BundledEntries(ctx)
	if bundledErr != nil {
		return nil, bundledErr
	}
	if err == nil && (feed.Version != 1 || !commitHash(feed.Commit) || len(feed.Packages) > 512) {
		err = packageFault("unsupported official plugin catalog")
	}
	if err != nil {
		if !force && ctx.Err() == nil {
			return bundled, nil
		}
		return nil, err
	}
	entries := []protocol.PluginCatalogEntry{}
	seen := map[string]bool{}
	for _, item := range feed.Packages {
		manifest, err := ValidateManifest(item.Manifest)
		if err != nil || item.ID != manifest.Name || seen[item.ID] {
			continue
		}
		validVersions := true
		for _, version := range item.Versions {
			if !commitHash(version.Commit) {
				validVersions = false
			}
		}
		if !validVersions {
			continue
		}
		entry := manifestCatalogEntry(manifest)
		repository, commit := OfficialRepository+".git", feed.Commit
		entry.Repository, entry.Commit, entry.ReleaseNotes, entry.Versions = &repository, &commit, item.ReleaseNotes, item.Versions
		for index := range entry.Versions {
			entry.Versions[index].Compatibility = Compatibility(entry.Versions[index].MinSailryVersion)
		}
		seen[item.ID] = true
		entries = append(entries, entry)
	}
	for _, candidate := range bundled {
		index := slices.IndexFunc(entries, func(entry protocol.PluginCatalogEntry) bool { return entry.ID == candidate.ID })
		if index < 0 {
			entries = append(entries, candidate)
		} else if IsNewer(entries[index].Version, candidate.Version) {
			entries[index] = candidate
		}
	}
	return entries, nil
}

func (manager *Manager) Catalog(ctx context.Context, source protocol.PluginCatalogSource, query string, page uint32, force bool) (protocol.PluginCatalogPage, error) {
	result := protocol.DefaultPluginCatalogPage()
	if page == 0 || len(query) > 4096 || strings.ContainsRune(query, 0) {
		return result, packageFault("invalid catalog query")
	}
	if source == protocol.PluginCatalogSourceOfficial {
		entries, err := manager.OfficialEntries(ctx, force)
		if err != nil {
			return result, err
		}
		query = strings.ToLower(query)
		for _, entry := range entries {
			match := strings.Contains(strings.ToLower(entry.Name), query)
			if entry.Description != nil {
				match = match || strings.Contains(strings.ToLower(*entry.Description), query)
			}
			for _, text := range entry.DescriptionLocales {
				match = match || strings.Contains(strings.ToLower(text), query)
			}
			if entry.Display != nil {
				match = match || strings.Contains(strings.ToLower(entry.Display.Label), query)
				for _, text := range entry.Display.Locales {
					match = match || strings.Contains(strings.ToLower(text), query)
				}
			}
			if match {
				result.Entries = append(result.Entries, entry)
			}
		}
		result.Page, result.Pages = 1, 1
		return result, nil
	}
	if source != protocol.PluginCatalogSourceThirdParty {
		return result, packageFault("invalid catalog source")
	}
	root := strings.TrimSuffix(manager.ThirdPartyURL, "/")
	if root == "" {
		root = ThirdPartyCatalogURL
	}
	parameters := url.Values{"protocol": {"agent-plugins"}, "q": {query}, "page": {strconv.FormatUint(uint64(page), 10)}, "per_page": {"12"}}
	var response struct {
		Data []struct {
			Slug        string   `json:"slug"`
			Name        string   `json:"name"`
			Description *string  `json:"description"`
			Repository  string   `json:"repoUrl"`
			Protocols   []string `json:"protocols"`
		} `json:"data"`
		Meta *struct {
			Page  uint32 `json:"page"`
			Pages uint32 `json:"total_pages"`
		} `json:"meta"`
	}
	if err := manager.catalogJSON(ctx, root+"/api/v1/plugins?"+parameters.Encode(), &response); err != nil {
		return result, err
	}
	if response.Meta == nil || response.Meta.Page == 0 {
		return result, packageFault("catalog pagination is missing")
	}
	result.Page, result.Pages = response.Meta.Page, response.Meta.Pages
	for _, item := range response.Data {
		if !slices.Contains(item.Protocols, "agent-plugins") {
			continue
		}
		if !declarationIdentifier(item.Slug) || len(item.Name) > 256 || item.Description != nil && len(*item.Description) > 8192 {
			return result, packageFault("invalid catalog entry")
		}
		if _, err := ParseRepositorySource(protocol.PluginSkillsSource{Repository: item.Repository}); err != nil {
			return result, err
		}
		entry := protocol.DefaultPluginCatalogEntry()
		entry.ID, entry.Name, entry.Description, entry.Repository = item.Slug, item.Name, item.Description, &item.Repository
		result.Entries = append(result.Entries, entry)
	}
	return result, nil
}

func (manager *Manager) CatalogSource(ctx context.Context, source protocol.PluginCatalogSource, id string) (protocol.PluginSkillsSource, error) {
	if !declarationIdentifier(id) {
		return protocol.PluginSkillsSource{}, packageFault("invalid catalog identifier")
	}
	if source == protocol.PluginCatalogSourceOfficial {
		entries, err := manager.OfficialEntries(ctx, true)
		if err != nil {
			return protocol.PluginSkillsSource{}, err
		}
		for _, entry := range entries {
			if entry.ID == id {
				if entry.Commit == nil {
					return protocol.PluginSkillsSource{}, packageFault("official plugin has no captured online revision")
				}
				return protocol.PluginSkillsSource{Repository: OfficialRepository + ".git", GitRef: entry.Commit, Path: &id}, nil
			}
		}
		return protocol.PluginSkillsSource{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "official plugin is unavailable"}
	}
	root := strings.TrimSuffix(manager.ThirdPartyURL, "/")
	if root == "" {
		root = ThirdPartyCatalogURL
	}
	var response struct {
		Data struct {
			Repository string `json:"repoUrl"`
			Path       string `json:"plugin_path"`
			Manifests  map[string]struct {
				Path string `json:"path"`
			} `json:"manifests"`
		} `json:"data"`
	}
	if err := manager.catalogJSON(ctx, root+"/api/v1/plugins/"+url.PathEscape(id), &response); err != nil {
		return protocol.PluginSkillsSource{}, err
	}
	selectedPath := response.Data.Path
	if manifest, exists := response.Data.Manifests["agent-plugins"]; exists && strings.HasSuffix(manifest.Path, "/plugin.json") {
		selectedPath = strings.TrimSuffix(manifest.Path, "/plugin.json")
	}
	selected := protocol.PluginSkillsSource{Repository: response.Data.Repository, Path: &selectedPath}
	_, err := ParseRepositorySource(selected)
	return selected, err
}
