package plugins

import (
	"context"
	"errors"
	"fmt"
	"net/url"
	"slices"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type cachedSkillCatalog struct {
	page protocol.PluginSkillcatalogPage
	at   time.Time
}

func officialSkillEntry(packageName string, skill protocol.PluginSkill, bundled bool, commit string) protocol.PluginSkillcatalogEntry {
	entry := protocol.DefaultPluginSkillcatalogEntry()
	entry.ID, entry.Package, entry.Name = packageName+"/"+skill.Name, &packageName, skill.Name
	entry.DisplayName, entry.DisplayLocales, entry.Description, entry.Origin, entry.Bundled = skill.DisplayName, skill.DisplayLocales, &skill.Description, "sailry/sailry-plugins", bundled
	directory := packageName + "/skills/" + skill.Name
	source := protocol.PluginSkillsSource{Repository: OfficialRepository, GitRef: &commit, Path: &directory}
	entry.Source = &protocol.PluginSkillcatalogInstallSource{Kind: "git", Data: source}
	return entry
}

func BundledSkillEntries(ctx context.Context) ([]protocol.PluginSkillcatalogEntry, error) {
	packages, err := BundledEntries(ctx)
	if err != nil {
		return nil, err
	}
	entries := []protocol.PluginSkillcatalogEntry{}
	for _, item := range packages {
		captured, issues, err := bundledCapture(ctx, item.ID)
		if err != nil {
			return nil, err
		}
		if len(issues) != 0 {
			return nil, packageFault("invalid bundled skill resources")
		}
		files := map[string][]byte{}
		for _, entry := range captured {
			if !entry.directory {
				files[entry.name] = entry.data
			}
		}
		skills, issues := DiscoverSkills(files)
		if len(issues) != 0 {
			return nil, packageFault("invalid bundled skill documents")
		}
		for _, skill := range skills {
			entries = append(entries, officialSkillEntry(item.ID, skill, true, "main"))
		}
	}
	return entries, nil
}

func (manager *Manager) SkillCatalog(ctx context.Context, source protocol.PluginSkillcatalogSource, query string, page uint32, force bool) (protocol.PluginSkillcatalogPage, error) {
	result := protocol.DefaultPluginSkillcatalogPage()
	result.QueryScope, result.Page = protocol.PluginSkillcatalogQueryScopeCatalog, page
	if page == 0 || len(query) > 4096 || strings.ContainsRune(query, 0) {
		return result, packageFault("invalid skill catalog query")
	}
	key := fmt.Sprintf("%s\x00%s\x00%d", source, query, page)
	manager.repositoryMu.Lock()
	cached, exists := manager.skillPages[key]
	manager.repositoryMu.Unlock()
	if !force && exists && time.Since(cached.at) < 15*time.Minute {
		return cached.page, nil
	}
	var err error
	switch source {
	case protocol.PluginSkillcatalogSourceOfficial:
		var feed struct {
			Version uint32 `json:"version"`
			Commit  string `json:"source_commit"`
			Skills  []struct {
				Package     string `json:"package"`
				Path        string `json:"path"`
				Frontmatter string `json:"frontmatter"`
			} `json:"skills"`
		}
		endpoint := manager.OfficialURL
		if endpoint == "" {
			endpoint = OfficialCatalogURL
		}
		err = manager.catalogJSON(ctx, endpoint, &feed)
		if err == nil {
			if feed.Version != 1 || !commitHash(feed.Commit) || len(feed.Skills) > 4096 {
				return result, packageFault("invalid official skill catalog")
			}
			for _, item := range feed.Skills {
				skillName := strings.TrimSuffix(strings.TrimPrefix(item.Path, "skills/"), "/SKILL.md")
				if !ValidateName(item.Package) || item.Path != "skills/"+skillName+"/SKILL.md" || strings.Contains(skillName, "/") {
					return result, packageFault("invalid official skill catalog identity")
				}
				skill, parseErr := ParseSkill([]byte(item.Frontmatter), skillName)
				if parseErr != nil {
					return result, parseErr
				}
				skill.Path = item.Path
				result.Entries = append(result.Entries, officialSkillEntry(item.Package, skill, false, feed.Commit))
			}
		}
		if err != nil {
			var fault *protocol.Fault
			if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeUnavailable && fault.Code != protocol.ErrorCodeNotFound {
				return result, err
			}
			result.Entries, err = BundledSkillEntries(ctx)
			if err != nil {
				return result, err
			}
			result.Unavailable = true
		}
		result.Page = 1
		needle := strings.ToLower(strings.TrimSpace(query))
		result.Entries = slices.DeleteFunc(result.Entries, func(entry protocol.PluginSkillcatalogEntry) bool {
			text := entry.ID + " " + entry.Name
			if entry.DisplayName != nil {
				text += " " + *entry.DisplayName
			}
			if entry.Description != nil {
				text += " " + *entry.Description
			}
			for _, value := range entry.DisplayLocales {
				text += " " + value
			}
			return !strings.Contains(strings.ToLower(text), needle)
		})
		count := uint64(len(result.Entries))
		result.Total = &count
	case protocol.PluginSkillcatalogSourceSkillsSh:
		root := manager.SkillsDirectoryURL
		if root == "" {
			root = "https://skills.sh"
		}
		searching := utf8.RuneCountInString(query) >= 2
		endpoint := strings.TrimSuffix(root, "/") + "/api/skills/all-time/" + strconv.FormatUint(uint64(page-1), 10)
		if searching {
			if page != 1 {
				return result, packageFault("skill search does not provide another page")
			}
			endpoint = strings.TrimSuffix(root, "/") + "/api/search?" + url.Values{"q": {query}, "limit": {"100"}}.Encode()
		}
		var response struct {
			Skills []struct {
				Source      string  `json:"source"`
				ID          string  `json:"skillId"`
				Name        string  `json:"name"`
				Description *string `json:"description"`
			} `json:"skills"`
			Total *uint64 `json:"total"`
			More  bool    `json:"hasMore"`
		}
		err = manager.catalogJSON(ctx, endpoint, &response)
		if err == nil {
			for _, skill := range response.Skills {
				if skill.Source == "" || !registrySegment(skill.ID) || strings.TrimSpace(skill.Name) == "" {
					return result, packageFault("invalid skills.sh entry")
				}
				entry := protocol.DefaultPluginSkillcatalogEntry()
				entry.ID, entry.Name, entry.DisplayName, entry.Description, entry.Origin = skill.Source+"/"+skill.ID, skill.ID, &skill.Name, skill.Description, skill.Source
				parts := strings.Split(skill.Source, "/")
				safe := true
				for _, part := range parts {
					safe = safe && registrySegment(part)
				}
				if safe {
					endpoint := "https://skills.sh/" + skill.Source + "/" + skill.ID
					entry.Url = &endpoint
					if len(parts) == 2 {
						entry.Source = &protocol.PluginSkillcatalogInstallSource{Kind: "git", Data: protocol.PluginSkillsSource{Repository: endpoint}}
					}
				}
				if !searching && query != "" && !strings.Contains(strings.ToLower(entry.ID+" "+skill.Name), strings.ToLower(query)) {
					continue
				}
				result.Entries = append(result.Entries, entry)
			}
			if !searching && response.More && page < ^uint32(0) {
				next := page + 1
				result.Next = &next
			}
			if !searching && query == "" {
				result.Total = response.Total
			}
			if !searching && query != "" {
				result.QueryScope = protocol.PluginSkillcatalogQueryScopePage
			}
		}
	case protocol.PluginSkillcatalogSourceSkillhub:
		parameters := url.Values{"page": {strconv.FormatUint(uint64(page), 10)}, "pageSize": {"24"}}
		if query != "" {
			parameters.Set("keyword", query)
		}
		var response struct {
			Code int64 `json:"code"`
			Data struct {
				Skills []struct {
					Icon        *string `json:"iconUrl"`
					Slug        string  `json:"slug"`
					Name        string  `json:"name"`
					Description *string `json:"description"`
					Chinese     *string `json:"description_zh"`
					Namespace   struct {
						Handle string `json:"handle"`
					} `json:"namespace"`
					Version *string `json:"version"`
				} `json:"skills"`
				Total uint64 `json:"total"`
			} `json:"data"`
		}
		err = manager.catalogJSON(ctx, manager.registryEndpoint()+"/api/skills?"+parameters.Encode(), &response)
		if err == nil {
			if response.Code != 0 {
				err = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Skillhub catalog is unavailable"}
				break
			}
			for _, skill := range response.Data.Skills {
				source := protocol.PluginSkillsRegistrySource{Namespace: &skill.Namespace.Handle, Slug: skill.Slug, Version: skill.Version}
				if err := ValidateRegistrySource(source); err != nil {
					return result, err
				}
				if strings.TrimSpace(skill.Name) == "" {
					return result, packageFault("invalid Skillhub name")
				}
				entry := protocol.DefaultPluginSkillcatalogEntry()
				entry.ID, entry.Name, entry.DisplayName, entry.Description, entry.Origin = skill.Namespace.Handle+"/"+skill.Slug, skill.Slug, &skill.Name, skill.Description, skill.Namespace.Handle
				if skill.Icon != nil {
					icon, parseErr := url.Parse(*skill.Icon)
					if parseErr == nil && icon.Scheme == "https" && icon.Host != "" && icon.User == nil {
						entry.Icon = skill.Icon
					}
				}
				if skill.Chinese != nil {
					entry.DescriptionLocales["zh-CN"] = *skill.Chinese
				}
				endpoint := "https://skillhub.cn/skills/" + skill.Namespace.Handle + "/" + skill.Slug
				entry.Url, entry.Source = &endpoint, &protocol.PluginSkillcatalogInstallSource{Kind: "registry", Data: source}
				result.Entries = append(result.Entries, entry)
			}
			result.Total = &response.Data.Total
			if uint64(page)*24 < response.Data.Total && page < ^uint32(0) {
				next := page + 1
				result.Next = &next
			}
		}
	default:
		return result, packageFault("invalid skill catalog source")
	}
	if err != nil {
		if ctx.Err() != nil {
			return result, ctx.Err()
		}
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeUnavailable && fault.Code != protocol.ErrorCodeNotFound {
			return result, err
		}
		if exists {
			result = cached.page
		}
		result.Unavailable = true
		return result, nil
	}
	if !result.Unavailable {
		manager.repositoryMu.Lock()
		manager.skillPages[key] = cachedSkillCatalog{page: result, at: time.Now()}
		manager.repositoryMu.Unlock()
	}
	return result, nil
}
