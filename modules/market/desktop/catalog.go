package desktop

import (
	"context"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	pluginCatalog = iota
	skillCatalog
	themeCatalog
)

func bindCatalog(app *shell.App, visible func() bool) func() {
	var catalog *catalogController
	return func() {
		if catalog == nil {
			model := app.Settings.Model
			if actions := model.PackageActions; actions != nil && actions.Catalog != nil {
				catalog = &catalogController{model: model, visible: visible, service: actions.Catalog}
				catalog.bind(actions)
			} else {
				return
			}
		}
		if catalog.images != nil {
			catalog.images.beginFrame()
		}
		catalog.sync(false)
	}
}

type catalogQuery struct {
	kind, source int
	text         string
}

type catalogController struct {
	model   *settings.Model
	visible func() bool
	service *settings.PackageCatalogService
	owner   *settings.PackageCatalogNode

	query      catalogQuery
	searched   bool
	cancel     context.CancelFunc
	pluginNext uint32
	skillPages [len(skillSources)]protocol.PluginSkillcatalogPage
	projection uint64
	locale     string
	images     *marketImages
}

func (c *catalogController) bind(actions *settings.PackageActions) {
	actions.Image = c.image
	actions.RetryMarket = func() {
		if c.visible() {
			c.sync(true)
		}
	}
	actions.MoreMarket = c.more
	actions.MarketPlugin = func(id string) {
		if c.visible() && c.owner != nil && c.owner == c.service.Node() {
			c.owner.InstallPlugin(id)
		}
	}
	actions.MarketSkill = func(id string) {
		if c.visible() && c.owner != nil && c.owner == c.service.Node() {
			c.owner.InstallSkill(id)
		}
	}
	actions.Theme = func(id string) {
		if c.visible() {
			c.service.InstallTheme(id)
		}
	}
}

func (c *catalogController) currentQuery() catalogQuery {
	m := c.model
	source := m.MarketSource
	if m.MarketKind == skillCatalog {
		source = m.MarketSkillSource
	}
	return catalogQuery{m.MarketKind, source, m.MarketQuery}
}

func (c *catalogController) stop() {
	if c.cancel == nil {
		return
	}
	c.cancel()
	c.cancel = nil
	c.model.MarketPaging = false
	c.model.SetBusy(settings.BusyMarket, false)
}

func (c *catalogController) sync(force bool) {
	m := c.model
	if owner := c.service.Node(); c.owner != owner {
		c.stop()
		c.owner, c.searched = owner, false
		m.MarketPlugins, m.MarketSkills = nil, nil
		m.MarketPagingFailed = false
		c.pruneImages(nil)
	}
	if !c.visible() {
		if c.cancel != nil && !m.MarketPaging {
			c.searched, m.MarketLoad.Loading = false, false
		}
		c.stop()
		if c.images != nil && len(c.images.entries) > 0 {
			c.pruneImages(nil)
			c.images.client.CloseIdleConnections()
		}
		return
	}
	query := c.currentQuery()
	if query.kind == themeCatalog {
		c.stop()
		c.searched = false
		c.pruneImages(nil)
		m.MarketMore, m.MarketFailed, m.MarketPagingFailed = false, false, false
		m.Themes, m.MarketLoad.Loading, m.MarketLoad.Loaded = c.service.ThemeCatalog(force)
		return
	}
	if c.owner == nil {
		m.MarketLoad.Loading, m.MarketLoad.Loaded = false, true
		m.MarketMore, m.MarketFailed = false, false
		return
	}
	sameQuery := c.searched && query == c.query
	if sameQuery && !force {
		locale, revision := c.owner.Locale(), c.owner.Revision()
		if query.kind == skillCatalog && locale != c.locale {
			c.publishSkills()
		} else if locale != c.locale || revision != c.projection {
			c.projectRows()
		}
		return
	}
	c.stop()
	if !sameQuery {
		clear(c.skillPages[:])
		c.pluginNext = 0
		m.MarketPlugins, m.MarketSkills = nil, nil
		c.pruneImages(nil)
	}
	c.query, c.searched = query, true
	m.MarketLoad.Loading, m.MarketLoad.Loaded = true, false
	m.MarketMore, m.MarketFailed, m.MarketPagingFailed = false, false, false
	if query.kind == skillCatalog {
		c.searchSkills(force)
	} else {
		c.searchPlugins(1, force)
	}
}

func (c *catalogController) more() {
	if !c.visible() || c.owner == nil || c.owner != c.service.Node() || !c.searched || c.cancel != nil || c.currentQuery() != c.query {
		return
	}
	switch c.query.kind {
	case pluginCatalog:
		if c.pluginNext != 0 {
			c.searchPlugins(c.pluginNext, false)
		}
	case skillCatalog:
		if pages := c.nextSkillPages(); pages != [len(skillSources)]uint32{} {
			c.readSkillPages(pages, true, false)
		}
	}
}

func (c *catalogController) start(paging bool) context.Context {
	ctx, cancel := context.WithCancel(c.owner.Context)
	c.cancel = cancel
	c.model.MarketPaging, c.model.MarketPagingFailed = paging, false
	c.model.SetBusy(settings.BusyMarket, true)
	return ctx
}

func (c *catalogController) current(ctx context.Context, owner *settings.PackageCatalogNode, query catalogQuery) bool {
	return ctx.Err() == nil && c.visible() && c.service.Node() == owner && c.currentQuery() == query
}

func (c *catalogController) projectRows() {
	c.projection, c.locale = c.owner.Revision(), c.owner.Locale()
	if c.query.kind == skillCatalog {
		c.owner.ProjectSkills(c.model.MarketSkills)
	} else {
		c.owner.ProjectPlugins(c.model.MarketPlugins)
	}
}

func (c *catalogController) searchPlugins(number uint32, force bool) {
	paging := number > 1
	ctx := c.start(paging)
	query, owner := c.query, c.owner
	source := protocol.PluginCatalogSourceOfficial
	if query.source == 1 {
		source = protocol.PluginCatalogSourceThirdParty
	}
	if !owner.Run(func() {
		page, err := owner.SearchPlugins(ctx, protocol.CommandSearchPluginCatalogPayload{Source: source, Query: query.text, Page: number, Force: force})
		c.service.Enqueue(func(view *ui.Context) {
			if !c.current(ctx, owner, query) {
				return
			}
			c.stop()
			m := c.model
			failed := err != nil || page.Unavailable
			if paging {
				m.MarketPagingFailed = failed
			} else {
				m.MarketLoad.Loading, m.MarketLoad.Loaded = false, true
				m.MarketFailed = failed
			}
			if err != nil {
				kit.Error(view, i18n.T(view, "market_plugins"), err.Error())
			} else if page.Unavailable {
				kit.Fail(view, kit.Failed, "unavailable", nil)
			}
			if failed && (paging || len(page.Entries) == 0) {
				return
			}
			c.pluginNext = 0
			if !failed && number < page.Pages {
				c.pluginNext = number + 1
			}
			m.MarketMore = c.pluginNext != 0
			if !paging {
				m.MarketPlugins = nil
			}
			seen := make(map[string]bool, len(m.MarketPlugins)+len(page.Entries))
			for _, row := range m.MarketPlugins {
				seen[row.ID] = true
			}
			var entries []protocol.PluginCatalogEntry
			for _, entry := range page.Entries {
				if seen[entry.ID] {
					continue
				}
				seen[entry.ID] = true
				entries = append(entries, entry)
				m.MarketPlugins = append(m.MarketPlugins, viewmodel.Package{ID: entry.ID})
			}
			owner.RememberPlugins(source, entries)
			c.projectRows()
		})
	}) {
		c.stop()
		c.searched = false
		c.model.MarketLoad.Loading = false
	}
}
