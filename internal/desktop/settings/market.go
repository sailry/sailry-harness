package settings

import (
	"strconv"
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
	ui.Scroll(c.Key("market")).Fill().TrackScroll(&s.marketScroll).Children(func() {
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
				}
			})
			marketFilters(c, m)
			if m.MarketKind == 2 || m.MarketLoad.Loading {
				s.marketEnd = scrollEnd{}
			}
			switch {
			case m.MarketLoad.Loading:
				marketSkeleton(c)
			case m.MarketKind == 0:
				rows := marketPlugins(c, s)
				marketPagination(c, s, rows)
			case m.MarketKind == 1:
				rows := marketSkills(c, s)
				marketPagination(c, s, rows)
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

// marketEmpty shows the empty state or a retry when the catalog failed.
func marketEmpty(c *ui.Context, m *Model, icon *ui.SVG, none, failed string) {
	props := kit.ResultProps{Icon: icon, Title: i18n.T(c, none), Card: true}
	if m.Live {
		switch {
		case m.MarketFailed || m.MarketPagingFailed:
			props.Icon, props.Status, props.Title = nil, kit.ResultError, i18n.T(c, failed)
			props.Extra = func() { marketRetry(c, m) }
		case m.MarketMore || m.MarketPaging:
			return
		}
	}
	kit.Result(c, props)
}

func marketRetry(c *ui.Context, m *Model) {
	retry := m.retryMarket
	if m.MarketPagingFailed {
		retry = m.loadMoreMarket
	}
	ui.Row(c.Key("market-retry")).Justify(ui.Center).Children(func() {
		kit.Secondary(c.Key("retry"), i18n.T(c, "settings_retry")).Disabled(m.Busy[BusyMarket]).OnClick(retry)
	})
}

func (m *Model) loadMoreMarket() {
	if m.PackageActions != nil && m.PackageActions.MoreMarket != nil {
		m.PackageActions.MoreMarket()
	}
}

func marketPagination(c *ui.Context, s *State, rows int) {
	m := s.Model
	if !m.Live {
		return
	}
	if m.MarketMore && !m.MarketPagingFailed && !m.Busy[BusyMarket] && s.marketEnd.reached(c, &s.marketScroll, rows) {
		m.loadMoreMarket()
	}
	if m.MarketPaging || m.MarketMore && !m.MarketPagingFailed {
		ui.Row(c.Key("more")).Height(24).Center().Busy(m.MarketPaging).Children(func() {
			if m.MarketPaging {
				kit.Loader(c, 16)
			}
		})
	}
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

func marketPlugins(c *ui.Context, s *State) int {
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
		return 0
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
	if m.Live && (m.MarketFailed || m.MarketPagingFailed) {
		marketRetry(c, m)
	}
	return len(shown)
}

func marketSkills(c *ui.Context, s *State) int {
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
		return 0
	}
	kit.Grid(c, func() {
		for _, x := range shown {
			installed := x.Installed
			for _, y := range m.SkillList {
				installed = installed || y.ID == x.ID
			}
			marketSkillCard(c, s, x, MarketAction(installed, x.Update))
		}
	})
	if m.Live && (m.MarketFailed || m.MarketPagingFailed) {
		marketRetry(c, m)
	}
	return len(shown)
}

// marketSkillCard is a catalog skill as a card: the publisher's or the skill's
// picture beside the publisher and the name, the directory's verified mark,
// two lines of description, then the directory's counts with Details and the
// action. The publisher stands in for a directory without a description.
func marketSkillCard(c *ui.Context, s *State, x *preview.Skill, action string) {
	m := s.Model
	p := theme.Of(c)
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	open := func() { s.ShowDetails(DetailSkill, x.ID) }
	card := kit.Frost(c, ui.Column(c.Key(x.ID)).MinWidth(0).Padding(16).Gap(12).Radius(p.RadiusLg).Border(1, p.Border), p.Surface, kit.CardTint)
	if card.Hovered() {
		card.Background(p.Hover)
	}
	card.Children(func() {
		b := ui.ButtonBase(c.Key("open")).Label(x.Name).FillWidth().MinWidth(0).Cursor(ui.CursorPointer).OnClick(open)
		b.Children(func() {
			ui.Column(c).FillWidth().MinWidth(0).Gap(10).Children(func() {
				ui.Row(c).FillWidth().MinWidth(0).Gap(10).AlignItems(ui.Center).Children(func() {
					skillPicture(c, m, x)
					ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(2).Children(func() {
						publisher := x.Publisher
						if publisher == "" {
							publisher = x.Origin
						}
						if publisher != "" {
							ui.Text(c, publisher).FontSize(12).Font("monospace").TextColor(p.MutedForeground).SingleLine()
						}
						ui.Text(c, x.Name).FontSize(15).FontWeight(600).SingleLine()
					})
					if x.Verified {
						kit.Badge(c, t("market_verified"), kit.Positive).Shrink(0).AlignSelf(ui.Start)
					}
				})
				desc := strings.TrimSpace(x.Description)
				if desc == "" {
					desc = x.Origin
				}
				ui.Text(c, desc).FontSize(13).TextColor(p.MutedForeground).MaxLines(2).Height(36).FillWidth()
			})
		})
		ui.Row(c.Key("footer")).FillWidth().Gap(12).AlignItems(ui.Center).Children(func() {
			count := func(icon *ui.SVG, label string, value *uint64) {
				if value == nil {
					return
				}
				ui.Row(c.Key(label)).Gap(4).AlignItems(ui.Center).Label(label + " " + compactCount(*value)).Children(func() {
					ui.Icon(c, icon).FontSize(13).TextColor(p.MutedForeground)
					ui.Text(c, compactCount(*value)).FontSize(12).FontFeatures("tnum").TextColor(p.MutedForeground).SingleLine()
				})
			}
			count(icons.Download, t("market_installs"), x.Installs)
			count(icons.Star, t("market_stars"), x.Stars)
			ui.Spacer(c)
			kit.TextAction(c.Key("details"), t("market_details"), false).OnClick(open)
			label := t(action)
			switch {
			case m.Busy[BusyPackage+x.ID]:
				kit.PrimaryBusy(c.Key("action"), label, false, true).Height(28)
			case action == "market_installed":
				ui.Text(c, label).FontSize(13).TextColor(p.MutedForeground).SingleLine()
			default:
				kit.PrimaryBusy(c.Key("action"), label, false, false).Height(28).OnClick(func() {
					if m.PackageActions != nil && m.PackageActions.MarketSkill != nil {
						m.PackageActions.MarketSkill(x.ID)
						return
					}
					notice(c)
				})
			}
		})
	})
}

// skillPicture is the card's 36 DIP picture once fetched, else the tile.
func skillPicture(c *ui.Context, m *Model, x *preview.Skill) {
	var bitmap *ui.Bitmap
	if x.Avatar != "" && m.PackageActions != nil && m.PackageActions.Image != nil {
		bitmap = m.PackageActions.Image(x.Avatar)
	}
	if bitmap == nil {
		kit.Tile(c, x.Name, icons.Bot)
		return
	}
	p := theme.Of(c)
	ui.Row(c).Size(kit.TileSize, kit.TileSize).Shrink(0).Radius(kit.TileSize*11/36).Clip().Border(1, p.Border).Children(func() {
		ui.Image(c, bitmap).Size(kit.TileSize, kit.TileSize)
	})
}

// compactCount is a count in at most four characters: 950, 1.2k, 4.4M.
func compactCount(n uint64) string {
	switch {
	case n >= 1_000_000:
		return strconv.FormatFloat(float64(n)/1_000_000, 'f', 1, 64) + "M"
	case n >= 1_000:
		return strconv.FormatFloat(float64(n)/1_000, 'f', 1, 64) + "k"
	}
	return strconv.FormatUint(n, 10)
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
	ui.Grid(c.Key("themes")).Columns(3).Gap(12).FillWidth().Children(func() {
		for _, x := range shown {
			marketThemeCard(c, s, x)
		}
	})
}

// marketThemeCard is a catalog theme as its preview over its name, its
// description and its action, like the Appearance gallery. A theme the Desktop
// holds no package of yet previews as a placeholder. A theme of one mode
// previews in that mode, else in the current one.
func marketThemeCard(c *ui.Context, s *State, x *preview.MarketTheme) {
	m := s.Model
	p := theme.Of(c)
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	dark := p.Dark
	if x.Dark != x.Light {
		dark = x.Dark
	}
	card := kit.Frost(c, ui.Column(c.Key(x.ID)).MinWidth(0).Radius(p.RadiusLg).Border(1, p.Border).Clip(), p.Surface, kit.CardTint)
	if card.Hovered() {
		card.Background(p.Hover)
	}
	card.Children(func() {
		b := ui.ButtonBase(c.Key("open")).Label(x.Name).FillWidth().MinWidth(0).Cursor(ui.CursorPointer).OnClick(func() { s.ShowDetails(DetailTheme, x.ID) })
		b.Children(func() {
			if pkg := theme.Find(x.ID); pkg.ID == x.ID {
				miniature(c, pkg, dark)
			} else {
				ui.Row(c).FillWidth().Height(previewHeight).Center().Background(p.Muted.Alpha(0.5)).Children(func() {
					ui.Icon(c, icons.Palette).FontSize(24).TextColor(p.MutedForeground)
				})
			}
		})
		ui.Column(c.Key("body")).FillWidth().MinWidth(0).Padding(12).Gap(8).Children(func() {
			ui.Row(c).FillWidth().MinWidth(0).Gap(6).AlignItems(ui.Center).Children(func() {
				ui.Text(c, x.Name).FontSize(14).FontWeight(600).SingleLine().Grow(1).Shrink(1).MinWidth(0)
				if x.Builtin {
					kit.Tag(c, t("market_theme_builtin")).Shrink(0)
				}
			})
			ui.Text(c, x.Description).FontSize(12).TextColor(p.MutedForeground).MaxLines(2).Height(32).FillWidth()
			ui.Row(c.Key("footer")).FillWidth().Gap(6).AlignItems(ui.Center).Children(func() {
				if x.Dark {
					kit.Tag(c, t("market_theme_dark"))
				}
				if x.Light {
					kit.Tag(c, t("market_theme_light"))
				}
				ui.Spacer(c)
				themeAction(c, m, x)
			})
		})
	})
}
