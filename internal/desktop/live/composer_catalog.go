package live

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// composerCatalog is what a conversation's composer offers: its model lists
// and its reference and command pages. Building them on every frame
// allocated them again while an idle window kept drawing, so each part is
// kept until the key it was derived from changes. The slices are shared by
// frames and clipped, so a frame's additions reallocate instead of writing
// into them.
type composerCatalog struct {
	seen                 bool
	modelsKey            composerCatalogKey
	models, members      []composer.Model
	families             []composer.Family
	referencesKey        composerCatalogKey
	references, commands []composer.Reference
}

// composerCatalogKey is what a catalog part is derived from: the Client
// revision, the results delivered to frames since (packages, skills, scores
// and admitted sessions arrive that way), the model list, the locales and
// the composer's own choices.
type composerCatalogKey struct {
	valid                             bool
	delivered, revision, session      uint64
	sessionExists                     bool
	models                            *composer.Model
	modelCount                        int
	locale, packageLocale, teamLocale string
	model, project, worktree          string
	effort                            float64
}

func (r *Runtime) composerCatalogKey(c *ui.Context, cv *viewmodel.Conversation, comp *composer.State) composerCatalogKey {
	models := r.models()
	key := composerCatalogKey{
		valid: true, delivered: r.localRuntime().delivered, revision: r.controller.Revision(),
		modelCount: len(models), locale: c.Locale(), packageLocale: r.packageLocale(),
		model: comp.Model, project: comp.Project, worktree: comp.Worktree, effort: comp.Effort,
	}
	if len(models) > 0 {
		key.models = &models[0]
	}
	if r.teams != nil {
		key.teamLocale = r.teams.locale
	}
	if session, ok := r.session(cv.ID); ok {
		key.session, key.sessionExists = session.Revision, true
	}
	return key
}

func (r *Runtime) composerCatalog(id string) *composerCatalog {
	if r.composerCatalogs == nil {
		r.composerCatalogs = map[string]*composerCatalog{}
	}
	catalog := r.composerCatalogs[id]
	if catalog == nil {
		catalog = &composerCatalog{}
		r.composerCatalogs[id] = catalog
	}
	catalog.seen = true
	return catalog
}

func (r *Runtime) releaseComposerCatalogs() {
	for id, catalog := range r.composerCatalogs {
		if !catalog.seen {
			delete(r.composerCatalogs, id)
		} else {
			catalog.seen = false
		}
	}
}

// projectComposerModels offers the conversation's models, named by their
// configuration's source, with the chosen model's measured score.
func (r *Runtime) projectComposerModels(c *ui.Context, cv *viewmodel.Conversation, comp *composer.State, ctx *composer.Context) {
	catalog := r.composerCatalog(cv.ID)
	if key := r.composerCatalogKey(c, cv, comp); catalog.modelsKey != key {
		catalog.modelsKey = key
		models := r.offeredModels(cv.ID, ctx.Models)
		r.nameModelSources(c, cv.ID, models)
		members := r.memberModels(models)
		for i := range models {
			model := &models[i]
			if model.ID == comp.Model && model.Reasoning {
				model.Score = r.measuredScore(model.ID, parseEffort(comp.EffortName(nil, *model)))
			}
		}
		var families []composer.Family
		for _, family := range composerFamilies {
			if slices.ContainsFunc(models, func(m composer.Model) bool { return m.Family == family.ID }) {
				families = append(families, family)
			}
		}
		catalog.models, catalog.members, catalog.families = slices.Clip(models), slices.Clip(members), slices.Clip(families)
	}
	ctx.Models, ctx.MemberModels, ctx.Families = catalog.models, catalog.members, catalog.families
}

// catalogReferences are the conversation's reference and command pages.
func (r *Runtime) catalogReferences(c *ui.Context, cv *viewmodel.Conversation, comp *composer.State) ([]composer.Reference, []composer.Reference) {
	catalog := r.composerCatalog(cv.ID)
	if key := r.composerCatalogKey(c, cv, comp); catalog.referencesKey != key {
		catalog.referencesKey = key
		references, commands := r.references(cv, comp)
		catalog.references, catalog.commands = slices.Clip(references), slices.Clip(commands)
	}
	return catalog.references, catalog.commands
}
