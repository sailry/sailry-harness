package live

import (
	"context"
	"errors"
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

var skillMarketSources = [...]struct {
	option int
	source protocol.PluginSkillcatalogSource
	label  string
}{
	{settings.SkillMarketSkillsSh, protocol.PluginSkillcatalogSourceSkillsSh, "skill_source_skills"},
	{settings.SkillMarketSkillhub, protocol.PluginSkillcatalogSourceSkillhub, "skill_source_hub"},
}

func (r *Runtime) searchSkillMarket(selection int, query string, force bool, generation uint64) {
	var pages [len(skillMarketSources)]uint32
	for index, source := range skillMarketSources {
		if selection == settings.SkillMarketAll || selection == source.option {
			pages[index] = 1
		}
	}
	r.publishSkillMarket()
	r.readSkillMarketPages(query, force, generation, pages)
}

func (r *Runtime) skillMarketNextPages() (pages [len(skillMarketSources)]uint32) {
	if r.App.Settings.MarketKind == 1 && r.packages.marketCancel == nil {
		for index, page := range r.packages.skillMarketPages {
			if page.Next != nil && *page.Next > page.Page {
				pages[index] = *page.Next
			}
		}
	}
	return pages
}

// loadMoreSkillMarket reads the next page of each source with one, for the
// search the cards show; a query the Node has not searched yet pages nothing.
func (r *Runtime) loadMoreSkillMarket() {
	m := r.App.Settings.Model
	if packageMarketKey(m.MarketKind, marketSource(m), m.MarketQuery) != r.packages.marketKey {
		return
	}
	if pages := r.skillMarketNextPages(); pages != [len(skillMarketSources)]uint32{} {
		r.readSkillMarketPages(m.MarketQuery, false, r.packages.marketGeneration, pages)
	}
}

func (r *Runtime) readSkillMarketPages(query string, force bool, generation uint64, pages [len(skillMarketSources)]uint32) {
	ctx, cancel := context.WithCancel(r.ctx)
	state := r.packages
	state.marketCancel = cancel
	m := r.App.Settings.Model
	m.MarketLoad.Loading, m.MarketLoad.Loaded = len(m.MarketSkills) == 0, len(m.MarketSkills) > 0
	m.SetBusy(settings.BusyMarket, true)
	remaining := 0
	available := false
	for index, number := range pages {
		if number == 0 {
			continue
		}
		remaining++
		source := skillMarketSources[index]
		r.async(func() {
			out, err := r.controller.Do(ctx, protocol.Command{Kind: "search_skill_catalog", Data: protocol.CommandSearchSkillCatalogPayload{Source: source.source, Query: query, Page: number, Force: force}})
			page, valid := detail[protocol.PluginSkillcatalogPage](out.Data)
			if err == nil && (!valid || out.Kind != "skill_catalog" || page.Page != number || page.Next != nil && *page.Next <= number) {
				err = errors.New("unexpected skill catalog response")
			}
			if err == nil {
				page.Entries = slices.DeleteFunc(slices.Clone(page.Entries), pluginMarketSkill)
			}
			r.enqueue(func(c *ui.Context) {
				if ctx.Err() != nil || state.marketGeneration != generation || r.localRuntime().packageOwner() != r || m.MarketKind != 1 {
					return
				}
				remaining--
				if err == nil {
					available = available || !page.Unavailable
					if !page.Unavailable || len(page.Entries) > 0 {
						if number > 1 {
							page.Entries = append(state.skillMarketPages[index].Entries, page.Entries...)
						}
						state.skillMarketPages[index] = page
						r.publishSkillMarket()
					}
				}
				if err != nil || page.Unavailable {
					// A first page that failed is offered again by Retry; a
					// later one stays the source's next page, for Load more.
					m.MarketFailed = m.MarketFailed || number == 1
					reason := i18n.T(c, "unavailable")
					if err != nil {
						reason = faultDetail(err)
					}
					kit.Error(c, i18n.T(c, source.label), reason)
				}
				m.MarketLoad.Loading = len(m.MarketSkills) == 0 && remaining > 0
				m.MarketLoad.Loaded = available || len(m.MarketSkills) > 0
				if remaining == 0 {
					r.cancelMarketSearch()
				}
			})
		})
	}
}

// The public directories can also list the official plugin repository.
func pluginMarketSkill(entry protocol.PluginSkillcatalogEntry) bool {
	repository := strings.TrimPrefix(plugins.OfficialRepository, "https://github.com/")
	return entry.Package != nil || entry.Bundled || strings.EqualFold(entry.Origin, repository)
}

func (r *Runtime) publishSkillMarket() {
	m, state := r.App.Settings.Model, r.packages
	locale := i18n.Match(r.packageLocale())
	m.MarketSkills = nil
	seen := map[string]bool{}
	for _, page := range state.skillMarketPages {
		for _, entry := range page.Entries {
			if seen[entry.ID] {
				continue
			}
			seen[entry.ID] = true
			state.skillCatalog[entry.ID] = entry
			row := viewmodel.Skill{ID: entry.ID, Name: entry.Name, Description: stringValue(entry.Description), Origin: entry.Origin, Content: state.documents[entry.ID]}
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
	m.MarketMore = false
	for _, page := range state.skillMarketPages {
		m.MarketMore = m.MarketMore || page.Next != nil && *page.Next > page.Page
	}
	r.projectSkillMarket()
	r.readMarketSkillUpdates()
}
