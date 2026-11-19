package settings

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The Market page (settings/market*, market/*): Plugins, Skills and
// Themes as segments, a search with the plugin or skill catalog source,
// filters (categories, theme modes, installed only) and cards with install
// or update actions. A card opens its details.

// Skill market source choices, in the order shown in the selector.
const (
	SkillMarketAll = iota
	SkillMarketSkillsSh
	SkillMarketSkillhub
)

// marketCategories are the plugins' category filters.
var marketCategories = []string{"", "market_category_development", "market_category_automation", "market_category_data",
	"market_category_documents", "market_category_entertainment"}

// MarketAction is a card's action: install, installed or update.
func MarketAction(installed, update bool) string {
	switch {
	case !installed:
		return "market_install"
	case update:
		return "market_update"
	}
	return "market_installed"
}

func (m *Model) theme(id string) *preview.MarketTheme {
	for i := range m.Themes {
		if m.Themes[i].ID == id {
			return &m.Themes[i]
		}
	}
	return nil
}

// installedPackage is the installed package of a catalog entry.
func (m *Model) installedPackage(id string) *preview.Package {
	for _, entry := range m.MarketPlugins {
		if entry.ID == id && entry.InstallName != "" {
			id = entry.InstallName
			break
		}
	}
	for i := range m.Packages {
		if m.Packages[i].ID == id && m.Packages[i].Installed {
			return &m.Packages[i]
		}
	}
	return nil
}

// MarketPage is the market, 800 wide at most.
func MarketPage(c *ui.Context, s *State) {
	m := s.Model
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	if !m.Live && m.MarketLoad.Advance(c.Now()) {
		c.After(preview.Tick)
	}
	ui.Scroll(c.Key("market")).Fill().Children(func() {
		ui.Column(c).FillWidth().MaxWidth(kit.PageWidth).Margin(0, ui.Auto).Padding(24).Gap(20).Children(func() {
			changedHost := false
			ui.Row(c.Key("heading")).Gap(12).AlignItems(ui.Center).Children(func() {
				ui.Text(c, t("market_title")).FontSize(20).FontWeight(600).Grow(1)
				if m.MarketKind != 2 {
					ui.Row(c.Key("host")).Shrink(0).MaxWidthPercent(50).Children(func() {
						changedHost = managedNodeSelect(c, m, &m.ManagedNode)
					})
				}
			})
			if changedHost {
				return
			}
			kit.Segmented(c, "kind", &m.MarketKind, []string{t("market_plugins"), t("market_skills"), t("market_themes")}, kit.SegmentedProps{}).Label(t("market_kind")).AlignSelf(ui.Start)
			if m.MarketKind != 2 && m.packageTargetOffline() {
				settingsTargetStatus(c, "connections_offline")
				return
			}
			search := []string{"market_search_plugins", "market_search_skills", "market_search_themes"}[m.MarketKind]
			ui.Row(c.Key("search")).Gap(12).Children(func() {
				ui.SearchField(c.Key("query"), &m.MarketQuery).Placeholder(t(search)).Label(t(search)).Grow(1).OnSubmit(m.retryMarket)
				switch m.MarketKind {
				case 0:
					pick(c, "source", t("market_details_source"), &m.MarketSource, []string{t("market_source_official"), t("market_source_community")}, false).Width(140)
				case 1:
					pick(c, "skill-source", t("market_details_source"), &m.MarketSkillSource, []string{t("market_all"), t("skill_source_skills"), t("skill_source_hub")}, false).Width(140)
				case 2:
					ui.Text(c, t("market_local")).FontSize(14).TextColor(theme.Of(c).MutedForeground)
				}
			})
			marketFilters(c, m)
			switch {
			case m.MarketLoad.Loading:
				marketSkeleton(c)
			case m.MarketKind == 0:
				marketPlugins(c, s)
			case m.MarketKind == 1:
				marketSkills(c, s)
			default:
				marketThemes(c, s)
			}
		})
	})
}

// marketFilters are chips of the categories or theme modes, and Installed
// only.
func marketFilters(c *ui.Context, m *Model) {
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	p := theme.Of(c)
	chip := func(key, label string, on bool, pick func()) {
		b := kit.Ghost(c.Key(key), label, on).Height(28).PaddingX(10).Radius(999).Border(1, p.Border)
		b.Children(func() { ui.Text(c, label).FontSize(14) }).OnClick(pick)
	}
	ui.Row(c.Key("filters")).Wrap().Gap(6).Children(func() {
		switch m.MarketKind {
		case 0:
			for i, k := range marketCategories {
				label := t("market_all")
				if k != "" {
					label = t(k)
				}
				chip(k+"-chip", label, m.MarketCategory == i, func() { m.MarketCategory = i })
			}
		case 2:
			for i, k := range []string{"market_all", "market_theme_dark", "market_theme_light"} {
				chip(k, t(k), m.MarketMode == i, func() { m.MarketMode = i })
			}
		}
		ui.Spacer(c)
		if m.MarketKind != 1 {
			ui.Checkbox(c.Key("installed"), &m.MarketInstalled, t("market_installed_only"))
		}
	})
}

// marketMatch is whether a card is of the query. A live catalog is the
// Node's search for it, in the Node's order, which the page shows as is.
func (m *Model) marketMatch(text string) bool {
	return m.Live || marketQueryMatch(m.MarketQuery, text)
}

func marketQueryMatch(q, text string) bool {
	q = strings.ToLower(strings.TrimSpace(q))
	return q == "" || strings.Contains(strings.ToLower(text), q)
}

// retryMarket searches the catalog again.
func (m *Model) retryMarket() {
	if m.PackageActions != nil && m.PackageActions.RetryMarket != nil {
		m.PackageActions.RetryMarket()
	}
}

// marketEmpty keeps paging available when a source has later pages but
// the pages read so far have no matches.
func marketEmpty(c *ui.Context, m *Model, icon *ui.SVG, none, failed string) {
	props := kit.ResultProps{Icon: icon, Title: i18n.T(c, none), Card: true}
	if m.Live && m.MarketFailed {
		props.Icon, props.Status, props.Title = nil, kit.ResultError, i18n.T(c, failed)
	}
	if m.Live && (m.MarketFailed || m.MarketMore) {
		props.Extra = func() { marketActions(c, m) }
	}
	kit.Result(c, props)
}

// marketMore follows a live search's cards: Retry once a source failed, the
// cards read kept, and Load more while a source has a further page; a page
// that failed stays next, for Load more again.
func marketMore(c *ui.Context, m *Model) {
	if !m.Live || !m.MarketFailed && !m.MarketMore {
		return
	}
	marketActions(c, m)
}

func marketActions(c *ui.Context, m *Model) {
	ui.Row(c.Key("more")).Justify(ui.Center).Gap(12).Children(func() {
		if m.MarketFailed {
			kit.Secondary(c.Key("retry"), i18n.T(c, "settings_retry")).OnClick(m.retryMarket)
		}
		if m.MarketMore {
			kit.SecondaryBusy(c.Key("load-more"), i18n.T(c, "market_load_more"), m.Busy[BusyMarket]).OnClick(func() {
				if m.PackageActions != nil && m.PackageActions.MoreMarket != nil {
					m.PackageActions.MoreMarket()
				}
			})
		}
	})
}

// marketCard is a 76 DIP bordered card of the item's summary and its action.
func marketCard(c *ui.Context, key, title, desc string, tile func(), action string, busy bool, open, act func()) {
	p := theme.Of(c)
	card := kit.Frost(c, ui.Row(c.Key(key)).Height(76).MinWidth(0).PaddingX(16).Gap(12).Radius(p.RadiusLg).Border(1, p.Border), p.Surface, kit.CardTint)
	if card.Hovered() {
		card.Background(p.Hover)
	}
	card.Children(func() {
		b := ui.ButtonBase(c.Key("open")).Label(title).Grow(1).Shrink(1).MinWidth(0).Gap(12).Justify(ui.Start).Cursor(ui.CursorPointer).OnClick(open)
		b.Children(func() {
			ui.Row(c).Shrink(0).Children(tile)
			ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(4).Children(func() {
				ui.Text(c, title).FontSize(14).FontWeight(500).SingleLine()
				ui.Text(c, desc).FontSize(14).SingleLine().TextColor(p.MutedForeground)
			})
		})
		// The action is an icon: install or update, the spinner while it
		// runs, and a check that takes no press once installed.
		label := i18n.T(c, action)
		switch {
		case busy:
			kit.BusyAction(c.Key("action"), label)
		case action == "market_installed" || action == "market_theme_current":
			kit.Action(c.Key("action"), icons.Check, label, true)
		case action == "market_install":
			kit.Action(c.Key("action"), icons.Download, label, act == nil).OnClick(act)
		case action == "market_update":
			kit.Action(c.Key("action"), icons.ArrowUp, label, act == nil).OnClick(act)
		default:
			kit.SecondaryBusy(c.Key("action"), label, busy).Height(28).Shrink(0).Disabled(act == nil).OnClick(act)
		}
	})
}

// marketSkeleton holds the cards' place while the catalog loads: six bordered
// 76 DIP cards of a 40 DIP tile, a half-width title and a full-width line.
func marketSkeleton(c *ui.Context) {
	p := theme.Of(c)
	kit.Grid(c, func() {
		for i := range 6 {
			ui.Row(c.Key(i)).Height(76).MinWidth(0).PaddingX(16).Gap(12).Radius(p.RadiusLg).Border(1, p.Border).Children(func() {
				kit.Skeleton(c, kit.SkeletonProps{Width: 40, Height: 40})
				ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(8).Children(func() {
					kit.Skeleton(c, kit.SkeletonProps{Height: 16}).WidthPercent(50)
					kit.Skeleton(c, kit.SkeletonProps{Height: 12})
				})
			})
		}
	}).Busy(true)
}

func marketPlugins(c *ui.Context, s *State) {
	m := s.Model
	source := []string{"market_details_source_official", "market_details_source_community"}[m.MarketSource]
	var shown []*preview.Package
	for i := range m.MarketPlugins {
		x := &m.MarketPlugins[i]
		installed := m.installedPackage(x.ID) != nil
		if x.Source != source || !m.marketMatch(x.Name+" "+x.Description) ||
			m.MarketCategory > 0 && x.Category != marketCategories[m.MarketCategory] || m.MarketInstalled && !installed {
			continue
		}
		shown = append(shown, x)
	}
	if len(shown) == 0 {
		marketEmpty(c, m, icons.Puzzle, "plugins_none", "unavailable")
		return
	}
	kit.Grid(c, func() {
		for _, x := range shown {
			inst := m.installedPackage(x.ID)
			action := MarketAction(inst != nil, inst != nil && Update(inst))
			// The source is the filter above the cards, not repeated on each.
			desc := x.Description
			var install func()
			if inst == nil && x.Installable || inst != nil && Update(inst) {
				install = func() {
					if m.PackageActions != nil && m.PackageActions.MarketPlugin != nil {
						m.PackageActions.MarketPlugin(x.ID)
						return
					}
					notice(c)
				}
			}
			marketCard(c, x.ID, x.Name, desc, func() { iconTile(c, x.Name, x.Icon, x.Tint, icons.Puzzle, kit.TileSize) }, action, m.Busy[BusyPackage+x.ID],
				func() { s.ShowDetails(DetailPlugin, x.ID) }, install)
		}
	})
	marketMore(c, m)
}

func marketSkills(c *ui.Context, s *State) {
	m := s.Model
	// Preview fixtures use catalog URLs as origins. Live sources are selected
	// by the request, since a live origin names the repository or publisher.
	var previewOrigin string
	if !m.Live {
		switch m.MarketSkillSource {
		case SkillMarketSkillsSh:
			previewOrigin = "skills.sh/"
		case SkillMarketSkillhub:
			previewOrigin = "skillhub.cn/"
		}
	}
	var shown []*preview.Skill
	for i := range m.MarketSkills {
		x := &m.MarketSkills[i]
		if previewOrigin != "" && !strings.HasPrefix(x.Origin, previewOrigin) {
			continue
		}
		if m.marketMatch(x.Name + " " + x.Description) {
			shown = append(shown, x)
		}
	}
	if len(shown) == 0 {
		marketEmpty(c, m, icons.Bot, "skills_no_matches", "skill_catalog_load_failed")
		return
	}
	kit.Grid(c, func() {
		for _, x := range shown {
			installed := x.Installed
			for _, y := range m.SkillList {
				installed = installed || y.ID == x.ID
			}
			marketCard(c, x.ID, x.Name, x.Origin+" · "+x.Description, func() { kit.Tile(c, x.Name, icons.Bot) },
				MarketAction(installed, x.Update), m.Busy[BusyPackage+x.ID], func() { s.ShowDetails(DetailSkill, x.ID) }, func() {
					if m.PackageActions != nil && m.PackageActions.MarketSkill != nil {
						m.PackageActions.MarketSkill(x.ID)
						return
					}
					notice(c)
				})
		}
	})
	marketMore(c, m)
}

// themeAction is a theme's action: Apply an installed theme, Install or
// Update one, or In use.
func themeAction(c *ui.Context, m *Model, th *preview.MarketTheme) {
	action := MarketAction(th.Installed, th.Update)
	switch {
	case th.Current:
		action = "market_theme_current"
	case th.Installed && !th.Update:
		action = "market_theme_apply"
	}
	label := i18n.T(c, action)
	if action == "market_theme_current" {
		ui.Text(c, label).FontSize(14).TextColor(theme.Of(c).MutedForeground)
		return
	}
	kit.SecondaryBusy(c.Key("theme-action"), label, m.Busy[BusyPackage+th.ID]).Height(28).OnClick(func() {
		if m.PackageActions != nil && m.PackageActions.Theme != nil {
			m.PackageActions.Theme(th.ID)
			return
		}
		notice(c)
	})
}

func marketThemes(c *ui.Context, s *State) {
	m := s.Model
	var shown []*preview.MarketTheme
	for i := range m.Themes {
		x := &m.Themes[i]
		if !marketQueryMatch(m.MarketQuery, x.Name+" "+x.Description) || m.MarketInstalled && !x.Installed ||
			m.MarketMode == 1 && !x.Dark || m.MarketMode == 2 && !x.Light {
			continue
		}
		shown = append(shown, x)
	}
	if len(shown) == 0 {
		kit.Result(c, kit.ResultProps{Icon: icons.Palette, Title: i18n.T(c, "market_no_themes"), Card: true})
		return
	}
	kit.Grid(c, func() {
		for _, x := range shown {
			action := MarketAction(x.Installed, x.Update)
			switch {
			case x.Current:
				action = "market_theme_current"
			case x.Installed && !x.Update:
				action = "market_theme_apply"
			}
			desc := x.Description
			if x.Builtin {
				desc = i18n.T(c, "market_theme_builtin") + " · " + desc
			}
			marketCard(c, x.ID, x.Name, desc, func() { kit.Tile(c, x.Name, icons.Palette) }, action, m.Busy[BusyPackage+x.ID],
				func() { s.ShowDetails(DetailTheme, x.ID) }, func() {
					if m.PackageActions != nil && m.PackageActions.Theme != nil {
						m.PackageActions.Theme(x.ID)
						return
					}
					notice(c)
				})
		}
	})
}
