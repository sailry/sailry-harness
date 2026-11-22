package settings

import (
	"context"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Skill market source choices, in the order shown in the selector.
const (
	SkillMarketAll = iota
	SkillMarketSkillsSh
	SkillMarketSkillhub
)

// PackageCatalogService exposes shared package operations to the optional page.
// Node returns a stable scope for the selected package target, or nil.
type PackageCatalogService struct {
	Node         func() *PackageCatalogNode
	ThemeCatalog func(refresh bool) (rows []viewmodel.MarketTheme, loading, loaded bool)
	InstallTheme func(string)
	Context      context.Context
	Run          func(func()) bool
	Enqueue      func(func(*ui.Context))
}

type PackageCatalogNode struct {
	Context         context.Context
	Run             func(func()) bool
	SearchPlugins   func(context.Context, protocol.CommandSearchPluginCatalogPayload) (protocol.PluginCatalogPage, error)
	SearchSkills    func(context.Context, protocol.CommandSearchSkillCatalogPayload) (protocol.PluginSkillcatalogPage, error)
	RememberPlugins func(protocol.PluginCatalogSource, []protocol.PluginCatalogEntry)
	RememberSkills  func([]protocol.PluginSkillcatalogEntry)
	ProjectPlugins  func([]viewmodel.Package)
	ProjectSkills   func([]viewmodel.Skill)
	Revision        func() uint64
	Locale          func() string
	InstallPlugin   func(string)
	InstallSkill    func(string)
}

// MarketCatalog is the search and result state shared with the scoped package service.
type MarketCatalog struct {
	MarketKind        int // 0 plugins, 1 skills, 2 themes
	MarketQuery       string
	MarketSource      int // 0 official, 1 community
	MarketSkillSource int
	MarketPlugins     []viewmodel.Package
	MarketSkills      []viewmodel.Skill
	// The catalog shows a skeleton until its first page is ready. A failed
	// continuation leaves the existing cards in place until a retry.
	MarketLoad         preview.Fetch
	MarketMore         bool
	MarketPaging       bool
	MarketPagingFailed bool
	MarketFailed       bool
}

// MarketScope binds the catalog and package operations to the selected Node.
// Themes use the local Desktop profile.
type MarketScope struct {
	*MarketCatalog
	Live     bool
	Node     string
	Offline  bool
	Packages []viewmodel.Package
	Skills   []viewmodel.Skill
	Themes   []viewmodel.MarketTheme

	SelectNode   func(*ui.Context) bool
	ShowDetails  func(DetailKind, string)
	CloseDetails func()
	Busy         func(string) bool
	Retry        func()
	More         func()
	Plugin       func(string)
	Skill        func(string)
	Theme        func(string)
	Image        func(string) *ui.Bitmap
}

// MarketScope projects the same selected Node and package services used by settings.
func (s *State) MarketScope() MarketScope {
	scope := MarketScope{
		MarketCatalog: &s.MarketCatalog,
		Live:          s.Live,
		Node:          s.managedNodeID(),
		Offline:       s.packageTargetOffline(),
		Packages:      s.Packages,
		Skills:        s.SkillList,
		Themes:        s.Themes,
		SelectNode:    func(c *ui.Context) bool { return ManagedNodeSelect(c, s.Model) },
		ShowDetails:   s.ShowDetails,
		CloseDetails:  s.CloseDetails,
		Busy:          func(key string) bool { return s.Busy[key] },
	}
	if a := s.PackageActions; a != nil {
		scope.Retry, scope.More = a.RetryMarket, a.MoreMarket
		scope.Plugin, scope.Skill, scope.Theme, scope.Image = a.MarketPlugin, a.MarketSkill, a.Theme, a.Image
	}
	return scope
}
