package plugins

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"net/http"
	"path"
	"slices"
	"strings"
	"sync"

	sailry "github.com/sailry/sailry-harness"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const OfficialRepository = "https://github.com/sailry/sailry-plugins"
const OfficialCatalogURL = "https://sailry.dev/plugins/catalog.json"

type DefaultPackage struct {
	Name    string
	Enabled bool
}

func BundledDefaults() []DefaultPackage {
	return []DefaultPackage{}
}

// RetiredBundled names packages the application once bundled and now provides
// itself. Startup removes a bundled installation of one; the removal record
// and the captured versions stay, so earlier turns still resolve.
func RetiredBundled() []string {
	return []string{"media", "progress", "code", "web-search", "external-browser", "browser", "computer", "memory"}
}

func manifestCatalogEntry(manifest Manifest, runtimes ...Runtime) protocol.PluginCatalogEntry {
	entry := protocol.DefaultPluginCatalogEntry()
	entry.ID, entry.Name, entry.Version, entry.Description = manifest.Name, manifest.Name, manifest.Version, manifest.Description
	entry.Repository = manifest.Repository
	entry.Compatibility = requirementsCompatibility(manifest.Requirements, runtimes...)
	if extension := manifest.Extension; extension != nil {
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
	return bundledEntries(ctx, nil)
}

var embeddedWithdrawals = sync.OnceValues(func() (map[string][]string, error) {
	return readBundledWithdrawals(sailry.Plugins)
})

func bundledWithdrawals(files fs.FS) (map[string][]string, error) {
	// Embedded assets cannot change during this process. Startup inspects each
	// default package, so decode their shared release index only once.
	if files == nil {
		return embeddedWithdrawals()
	}
	return readBundledWithdrawals(files)
}

func readBundledWithdrawals(files fs.FS) (map[string][]string, error) {
	data, err := fs.ReadFile(files, "plugins/catalog.json")
	if err != nil {
		if errors.Is(err, fs.ErrNotExist) {
			entries, directoryErr := fs.ReadDir(files, "plugins")
			if errors.Is(directoryErr, fs.ErrNotExist) || directoryErr == nil && len(entries) == 0 {
				return map[string][]string{}, nil
			}
			if directoryErr != nil {
				return nil, directoryErr
			}
			return nil, packageFault("bundled plugin catalog is missing")
		}
		return nil, err
	}
	var catalog struct {
		Version  uint32 `json:"version"`
		Packages []struct {
			ID       string `json:"id"`
			Versions []struct {
				Version   string `json:"version"`
				Withdrawn bool   `json:"withdrawn"`
			} `json:"versions"`
		} `json:"packages"`
	}
	if json.Unmarshal(data, &catalog) != nil || catalog.Version != 1 {
		return nil, packageFault("invalid bundled plugin catalog")
	}
	withdrawals := map[string][]string{}
	for _, entry := range catalog.Packages {
		for _, version := range entry.Versions {
			if version.Withdrawn {
				withdrawals[entry.ID] = append(withdrawals[entry.ID], version.Version)
			}
		}
	}
	return withdrawals, nil
}

func bundledEntries(ctx context.Context, files fs.FS) ([]protocol.PluginCatalogEntry, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	withdrawals, err := bundledWithdrawals(files)
	if err != nil {
		return nil, err
	}
	if files == nil {
		files = sailry.Plugins
	}
	names, err := fs.Glob(files, "plugins/*/plugin.json")
	if err != nil {
		return nil, err
	}
	entries := make([]protocol.PluginCatalogEntry, 0, len(names))
	retired := RetiredBundled()
	for _, name := range names {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		data, err := fs.ReadFile(files, name)
		if err != nil {
			return nil, err
		}
		manifest, err := ValidateManifest(data)
		if err != nil {
			return nil, err
		}
		if slices.Contains(retired, manifest.Name) {
			continue
		}
		entry := manifestCatalogEntry(manifest, RuntimeFromContext(ctx))
		if manifest.Extension != nil && manifest.Extension.Icon != nil {
			data, err := fs.ReadFile(files, path.Join(path.Dir(name), *manifest.Extension.Icon))
			if err == nil {
				entry.Icon = packageIcon(data)
			} else {
				entry.Icon = nil
			}
		}
		entry.Bundled = true
		entry.Versions = []protocol.PluginCatalogVersion{manifestVersion(manifest, "", nil, RuntimeFromContext(ctx))}
		modules, err := legacyBundledModules(ctx, files, manifest)
		if err != nil {
			return nil, err
		}
		entry.Versions[0].RequiresModules = append(entry.Versions[0].RequiresModules, modules...)
		if compatibility := runtimeCompatibility(nil, modules, []Runtime{RuntimeFromContext(ctx)}); compatibility.Kind != "compatible" {
			entry.Compatibility = compatibility
			entry.Versions[0].Compatibility = compatibility
		}
		if manifest.Version != nil && slices.Contains(withdrawals[manifest.Name], *manifest.Version) {
			entry.Versions[0].Withdrawn = true
			entry.Versions[0].Compatibility = CatalogVersionCompatibility(entry.Versions[0], RuntimeFromContext(ctx))
		}
		entry.CompatibleVersion = SelectCompatibleVersion(entry, nil, RuntimeFromContext(ctx))
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
			ID                  string                              `json:"id"`
			Manifest            json.RawMessage                     `json:"manifest"`
			ReleaseNotes        *protocol.PluginCatalogReleaseNotes `json:"release_notes"`
			Versions            []protocol.PluginCatalogVersion     `json:"versions"`
			VersionRequirements json.RawMessage                     `json:"version_requirements"`
		} `json:"packages"`
	}
	endpoint := manager.OfficialURL
	if endpoint == "" {
		endpoint = OfficialCatalogURL
	}
	err := manager.catalogJSON(ctx, endpoint, &feed)
	bundled, bundledErr := bundledEntries(ctx, manager.bundled)
	if bundledErr != nil {
		return nil, bundledErr
	}
	if err == nil && (feed.Version != 1 || !commitHash(feed.Commit) || len(feed.Packages) > 512) {
		err = packageFault("unsupported official plugin catalog")
	}
	if err != nil {
		if !force && ctx.Err() == nil && len(bundled) != 0 {
			return bundled, nil
		}
		return nil, err
	}
	entries := []protocol.PluginCatalogEntry{}
	seen := map[string]bool{}
	retired := RetiredBundled()
	for _, item := range feed.Packages {
		manifest, err := ValidateManifest(item.Manifest)
		if err != nil || item.ID != manifest.Name || seen[item.ID] || slices.Contains(retired, item.ID) {
			continue
		}
		requirements, err := readCatalogRequirements(item.VersionRequirements)
		if err != nil {
			return nil, err
		}
		entry := manifestCatalogEntry(manifest, RuntimeFromContext(ctx))
		repository, commit := manager.officialRepositoryURL(), feed.Commit
		entry.Repository, entry.Commit, entry.ReleaseNotes, entry.Versions = &repository, &commit, item.ReleaseNotes, item.Versions
		latest := -1
		for index := range entry.Versions {
			version := &entry.Versions[index]
			if manifest.Version != nil && version.Version == *manifest.Version {
				latest = index
				if requirements := manifest.Requirements; requirements != nil {
					if version.APIVersion == nil {
						version.APIVersion = &requirements.APIVersion
					}
					if version.MinSailryVersion == nil || IsNewer(version.MinSailryVersion, requirements.MinSailryVersion) {
						version.MinSailryVersion = requirements.MinSailryVersion
					}
					if err := mergeCatalogRequirements(version, catalogRuntimeRequirements{Products: requirements.Products, RequiresModules: requirements.RequiresModules}); err != nil {
						return nil, err
					}
				}
				if version.ReleaseNotes == nil {
					version.ReleaseNotes = item.ReleaseNotes
				}
			}
			if err := applyCatalogRequirements(version, requirements); err != nil {
				return nil, err
			}
			*version = withLegacyOfficialRequirements(entry.ID, *version, entry.Repository)
			version.Compatibility = CatalogVersionCompatibility(*version, RuntimeFromContext(ctx))
			if manifest.Version != nil && version.Version == *manifest.Version && !version.Withdrawn && entry.Compatibility.Kind != "compatible" {
				version.Compatibility = entry.Compatibility
			}
			if latest == index && !version.Withdrawn {
				entry.Compatibility = version.Compatibility
			}
		}
		if latest < 0 && manifest.Version != nil {
			version := manifestVersion(manifest, feed.Commit, item.ReleaseNotes, RuntimeFromContext(ctx))
			if err := applyCatalogRequirements(&version, requirements); err != nil {
				return nil, err
			}
			version = withLegacyOfficialRequirements(entry.ID, version, entry.Repository)
			version.Compatibility = CatalogVersionCompatibility(version, RuntimeFromContext(ctx))
			if entry.Compatibility.Kind == "compatible" {
				entry.Compatibility = version.Compatibility
			}
			entry.Versions = append(entry.Versions, version)
		} else if latest >= 0 {
			entry.Commit = &entry.Versions[latest].Commit
		}
		entry.CompatibleVersion = SelectCompatibleVersion(entry, nil, RuntimeFromContext(ctx))
		seen[item.ID] = true
		entries = append(entries, entry)
	}
	for _, candidate := range bundled {
		index := slices.IndexFunc(entries, func(entry protocol.PluginCatalogEntry) bool { return entry.ID == candidate.ID })
		if index < 0 {
			entries = append(entries, candidate)
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
	return manager.communityCatalogPage(ctx, query, page, force)
}

func (manager *Manager) CatalogSource(ctx context.Context, source protocol.PluginCatalogSource, id string) (protocol.PluginSkillsSource, error) {
	if source == protocol.PluginCatalogSourceOfficial {
		if !declarationIdentifier(id) {
			return protocol.PluginSkillsSource{}, packageFault("invalid catalog identifier")
		}
		entries, err := manager.OfficialEntries(ctx, true)
		if err != nil {
			return protocol.PluginSkillsSource{}, err
		}
		for _, entry := range entries {
			if entry.ID == id {
				selected := SelectCompatibleVersion(entry, nil, RuntimeFromContext(ctx))
				if selected == nil {
					return protocol.PluginSkillsSource{}, packageFault("no compatible official plugin version is available")
				}
				if !commitHash(selected.Commit) {
					return protocol.PluginSkillsSource{}, packageFault("official plugin has no captured online revision")
				}
				return protocol.PluginSkillsSource{Repository: manager.officialRepositoryURL(), GitRef: &selected.Commit, Path: &id}, nil
			}
		}
		return protocol.PluginSkillsSource{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "official plugin is unavailable"}
	}
	if source != protocol.PluginCatalogSourceThirdParty {
		return protocol.PluginSkillsSource{}, packageFault("invalid catalog source")
	}
	return manager.communityCatalogSource(ctx, id)
}
