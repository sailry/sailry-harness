package plugins

import (
	"context"
	"path"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/sync/singleflight"
)

const ThirdPartyCatalogURL = "https://agentpluginsdirectory.com"

const (
	communityCatalogIDPrefix = "community:"
	communityCatalogPageSize = 48
	communityCatalogTTL      = 5 * time.Minute
)

type communityPlugin struct {
	slug, name, description, version string
	repository, folder               string
	keywords                         []string
}

type communityCatalogCache struct {
	mu      sync.Mutex
	plugins []communityPlugin
	expires time.Time
	refresh singleflight.Group
}

func (cache *communityCatalogCache) current() []communityPlugin {
	cache.mu.Lock()
	defer cache.mu.Unlock()
	if time.Now().Before(cache.expires) {
		return cache.plugins
	}
	return nil
}

func (manager *Manager) communityPlugins(ctx context.Context, force bool) ([]communityPlugin, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	cache := &manager.communityCatalog
	if !force {
		if plugins := cache.current(); plugins != nil {
			return plugins, nil
		}
	}
	refresh := cache.refresh.DoChan("manifest", func() (any, error) {
		if !force {
			if plugins := cache.current(); plugins != nil {
				return plugins, nil
			}
		}
		// A canceled search must not discard the shared download for the
		// next search or for another controller.
		request, cancel := context.WithTimeout(context.WithoutCancel(ctx), 30*time.Second)
		defer cancel()
		plugins, err := manager.readCommunityPlugins(request)
		if err != nil {
			return nil, err
		}
		cache.mu.Lock()
		cache.plugins, cache.expires = plugins, time.Now().Add(communityCatalogTTL)
		cache.mu.Unlock()
		return plugins, nil
	})
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case result := <-refresh:
		if result.Err != nil {
			return nil, result.Err
		}
		return result.Val.([]communityPlugin), nil
	}
}

func (manager *Manager) readCommunityPlugins(ctx context.Context) ([]communityPlugin, error) {
	root := strings.TrimSuffix(manager.ThirdPartyURL, "/")
	if root == "" {
		root = ThirdPartyCatalogURL
	}
	var feed struct {
		Plugins []struct {
			Slug         string   `json:"slug"`
			Name         string   `json:"name"`
			Description  string   `json:"description"`
			Version      string   `json:"version"`
			Repository   string   `json:"repository"`
			Repo         string   `json:"repo"`
			ManifestPath string   `json:"manifestPath"`
			Keywords     []string `json:"keywords"`
		} `json:"plugins"`
	}
	if err := manager.catalogJSON(ctx, root+"/api/v1/registry-manifest.json", &feed); err != nil {
		return nil, err
	}
	if feed.Plugins == nil {
		return nil, packageFault("invalid community plugin catalog")
	}
	plugins := make([]communityPlugin, 0, len(feed.Plugins))
	seen := make(map[string]bool, len(feed.Plugins))
	const github = "https://github.com/"
	for _, item := range feed.Plugins {
		if !declarationIdentifier(item.Slug) || seen[item.Slug] || strings.TrimSpace(item.Name) == "" ||
			len(item.Name) > 256 || len(item.Description) > 8192 || len(item.Version) > 256 {
			continue
		}
		// repo is the GitHub project the directory scanned. repository is
		// optional manifest metadata and can point to a different project.
		repository := item.Repo
		if repository == "" {
			var ok bool
			repository, ok = strings.CutPrefix(item.Repository, github)
			if !ok {
				continue
			}
		}
		owner, name, separated := strings.Cut(repository, "/")
		if !separated || !registrySegment(owner) || !registrySegment(name) {
			continue
		}
		folder, file := path.Split(item.ManifestPath)
		folder = strings.TrimSuffix(folder, "/")
		if file != "plugin.json" || !resourcePath(item.ManifestPath) || len(folder) > 512 {
			continue
		}
		seen[item.Slug] = true
		plugins = append(plugins, communityPlugin{
			slug: item.Slug, name: item.Name, description: item.Description, version: item.Version,
			repository: github + repository, folder: folder, keywords: item.Keywords,
		})
	}
	if len(plugins) == 0 && len(feed.Plugins) != 0 {
		return nil, packageFault("invalid community plugin catalog entries")
	}
	return plugins, nil
}

func (manager *Manager) communityCatalogPage(ctx context.Context, query string, page uint32, force bool) (protocol.PluginCatalogPage, error) {
	result := protocol.DefaultPluginCatalogPage()
	plugins, err := manager.communityPlugins(ctx, force && page == 1)
	if err != nil {
		return result, err
	}
	query = strings.ToLower(strings.TrimSpace(query))
	match := func(text string) bool { return strings.Contains(strings.ToLower(text), query) }
	start := (uint64(page) - 1) * communityCatalogPageSize
	var total uint64
	for _, item := range plugins {
		if !match(item.slug) && !match(item.name) && !match(item.description) && !match(item.repository) && !slices.ContainsFunc(item.keywords, match) {
			continue
		}
		if total >= start && total < start+communityCatalogPageSize {
			entry := protocol.DefaultPluginCatalogEntry()
			entry.ID, entry.Name, entry.Repository = communityCatalogIDPrefix+item.slug, item.name, new(item.repository)
			if item.version != "" {
				entry.Version = new(item.version)
			}
			if item.description != "" {
				entry.Description = new(item.description)
			}
			result.Entries = append(result.Entries, entry)
		}
		total++
	}
	result.Page = page
	result.Pages = max(1, uint32((total+communityCatalogPageSize-1)/communityCatalogPageSize))
	return result, nil
}

func (manager *Manager) communityCatalogSource(ctx context.Context, id string) (protocol.PluginSkillsSource, error) {
	slug := strings.TrimPrefix(id, communityCatalogIDPrefix)
	if !declarationIdentifier(slug) {
		return protocol.PluginSkillsSource{}, packageFault("invalid catalog identifier")
	}
	plugins, err := manager.communityPlugins(ctx, false)
	if err != nil {
		return protocol.PluginSkillsSource{}, err
	}
	for _, item := range plugins {
		if item.slug == slug {
			return protocol.PluginSkillsSource{Repository: item.repository, Path: new(item.folder)}, nil
		}
	}
	return protocol.PluginSkillsSource{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "community plugin is unavailable"}
}
