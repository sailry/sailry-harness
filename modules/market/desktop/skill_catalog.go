package desktop

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

var skillSources = [...]struct {
	option int
	source protocol.PluginSkillcatalogSource
	label  string
}{
	{settings.SkillMarketSkillsSh, protocol.PluginSkillcatalogSourceSkillsSh, "skill_source_skills"},
	{settings.SkillMarketSkillhub, protocol.PluginSkillcatalogSourceSkillhub, "skill_source_hub"},
}

func (c *catalogController) searchSkills(force bool) {
	var pages [len(skillSources)]uint32
	for index, source := range skillSources {
		if c.query.source == settings.SkillMarketAll || c.query.source == source.option {
			pages[index] = 1
		}
	}
	c.publishSkills()
	c.readSkillPages(pages, false, force)
}

func (c *catalogController) nextSkillPages() (pages [len(skillSources)]uint32) {
	for index, page := range c.skillPages {
		if page.Next != nil && *page.Next > page.Page {
			pages[index] = *page.Next
		}
	}
	return pages
}

func (c *catalogController) readSkillPages(pages [len(skillSources)]uint32, paging, force bool) {
	ctx := c.start(paging)
	query, owner, m := c.query, c.owner, c.model
	if !paging {
		m.MarketLoad.Loading, m.MarketLoad.Loaded = len(m.MarketSkills) == 0, len(m.MarketSkills) > 0
	}
	remaining := 0
	available := false
	for index, number := range pages {
		if number == 0 {
			continue
		}
		source := skillSources[index]
		if owner.Run(func() {
			page, err := owner.SearchSkills(ctx, protocol.CommandSearchSkillCatalogPayload{Source: source.source, Query: query.text, Page: number, Force: force})
			if err == nil {
				page.Entries = slices.DeleteFunc(slices.Clone(page.Entries), pluginMarketSkill)
				// A page-scoped query filters a leaderboard page locally. Stop
				// on an empty page instead of scanning the leaderboard.
				if page.QueryScope == protocol.PluginSkillcatalogQueryScopePage && len(page.Entries) == 0 {
					page.Next = nil
				}
			}
			c.service.Enqueue(func(view *ui.Context) {
				if !c.current(ctx, owner, query) {
					return
				}
				remaining--
				if err == nil {
					available = available || !page.Unavailable
					// An unavailable continuation must leave its cursor on
					// the failed page even if the Node has cached cards for it.
					if !page.Unavailable || !paging && len(page.Entries) > 0 {
						if paging {
							page.Entries = append(c.skillPages[index].Entries, page.Entries...)
						}
						c.skillPages[index] = page
						c.publishSkills()
					}
				}
				if err != nil || page.Unavailable {
					if paging {
						m.MarketPagingFailed = true
					} else {
						m.MarketFailed = true
					}
					reason := i18n.T(view, "unavailable")
					if err != nil {
						reason = err.Error()
					}
					kit.Error(view, i18n.T(view, source.label), reason)
				}
				if !paging {
					m.MarketLoad.Loading = len(m.MarketSkills) == 0 && remaining > 0
					m.MarketLoad.Loaded = available || len(m.MarketSkills) > 0
				}
				if remaining == 0 {
					c.stop()
				}
			})
		}) {
			remaining++
		}
	}
	if remaining == 0 {
		c.stop()
		m.MarketLoad.Loading = false
	}
}

// The public directories can also list the official plugin repository.
func pluginMarketSkill(entry protocol.PluginSkillcatalogEntry) bool {
	repository := strings.TrimPrefix(plugins.OfficialRepository, "https://github.com/")
	return entry.Package != nil || entry.Bundled || strings.EqualFold(entry.Origin, repository)
}

func (c *catalogController) publishSkills() {
	m := c.model
	locale := i18n.Match(c.owner.Locale())
	m.MarketSkills = nil
	seen, pictures := map[string]bool{}, map[string]bool{}
	var entries []protocol.PluginSkillcatalogEntry
	for _, page := range c.skillPages {
		for _, entry := range page.Entries {
			if seen[entry.ID] {
				continue
			}
			seen[entry.ID] = true
			entries = append(entries, entry)
			row := viewmodel.Skill{ID: entry.ID, Name: entry.Name, Origin: entry.Origin,
				Installs: entry.Installs, Stars: entry.Stars, Verified: entry.Verified}
			if entry.Description != nil {
				row.Description = *entry.Description
			}
			if entry.Publisher != nil {
				row.Publisher = *entry.Publisher
			}
			if entry.Avatar != nil {
				row.Avatar = *entry.Avatar
				pictures[row.Avatar] = true
			}
			if entry.DisplayName != nil {
				row.Name = *entry.DisplayName
			}
			if title := entry.DisplayLocales[locale]; title != "" {
				row.Name = title
			}
			if text := entry.DescriptionLocales[locale]; text != "" {
				row.Description = text
			}
			m.MarketSkills = append(m.MarketSkills, row)
		}
	}
	c.pruneImages(pictures)
	m.MarketMore = c.nextSkillPages() != [len(skillSources)]uint32{}
	c.owner.RememberSkills(entries)
	c.projectRows()
}
