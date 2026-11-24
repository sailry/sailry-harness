package live

import (
	"reflect"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	nodeproviders "github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type modelSource struct {
	providers, catalog, scores uint64
}

type modelScoreState struct {
	source         modelSource
	ready, reading bool
	revision       uint64
	values         map[string][]protocol.ConversationCatalogScoresEffortScore
}

type modelProjection struct {
	source    modelSource
	scores    uint64
	ready     bool
	private   []protocol.ConversationProvider
	providers []viewmodel.Provider
}

func (r *Runtime) modelInputs() (protocol.Snapshot, modelSource) {
	// Read the provider revision before the snapshot so a concurrent change
	// cannot mark an older snapshot with a newer revision.
	providers := r.controller.ProviderRevision()
	snapshot := r.snapshot()
	return snapshot, modelSource{providers, snapshot.ModelCatalog.Revision, snapshot.ModelCatalog.Scores.Revision}
}

func (r *Runtime) projectModels() {
	snapshot, source := r.modelInputs()
	r.readModelScores(source, snapshot.Providers)
	private := r.privateComposerProviders(snapshot)
	projected := &r.modelProjection
	if !projected.ready || projected.source != source || projected.scores != r.modelScores.revision || !reflect.DeepEqual(projected.private, private) {
		var models []composer.Model
		var providers []viewmodel.Provider
		for i, p := range slices.Concat(snapshot.Providers, private) {
			if i < len(snapshot.Providers) {
				providers = append(providers, viewmodel.Provider{ID: p.ID.String(), Name: p.Name})
			}
			if !p.Enabled || !r.hasLocalProviderCredential(p) {
				continue
			}
			for _, m := range p.Models {
				models = append(models, r.projectModel(p, m))
			}
		}
		r.nodeModels = models
		*projected = modelProjection{source: source, scores: r.modelScores.revision, ready: true, private: private, providers: providers}
	}
	if r.parent == nil {
		// Shell data can be rebuilt even when the model catalog is unchanged.
		r.App.Data.Models = r.nodeModels
		r.App.Data.Providers = projected.providers
	}
}

func (r *Runtime) projectModel(p protocol.ConversationProvider, m protocol.ConversationModel) composer.Model {
	name := m.ID
	if m.DisplayName != nil {
		name = *m.DisplayName
	}
	id := modelKey(p.ID, m.ID)
	scoreEffort := m.DefaultEffort
	if scoreEffort.Kind == "" || scoreEffort.Kind == "default" {
		if hinted := nodeproviders.ScoreEffortHint(m.ID); hinted != "" {
			scoreEffort = protocol.Effort{Kind: hinted}
		}
	}
	v := composer.Model{ID: id, Name: name, Provider: p.Name, Reasoning: m.Reasoning, Family: modelFamily(p.Api), DefaultEffort: effortName(m.DefaultEffort), Score: r.measuredScore(id, scoreEffort)}
	for _, e := range m.Efforts {
		v.Efforts = append(v.Efforts, effortName(e))
	}
	if m.Reasoning && !slices.Contains(v.Efforts, v.DefaultEffort) {
		v.Efforts = append(v.Efforts, v.DefaultEffort)
	}
	v.Scores = map[string]*composer.Score{}
	for _, effort := range v.Efforts {
		if score := r.measuredScore(id, parseEffort(effort)); score != nil {
			v.Scores[effort] = score
		}
	}
	if m.Capabilities != nil {
		for _, feature := range m.Capabilities.Features {
			f := composer.Feature{ID: feature.ID, Name: feature.ID}
			if feature.Name != nil {
				f.Name = *feature.Name
			}
			for _, option := range feature.Options {
				o := composer.FeatureOption{ID: option.ID, Name: option.ID}
				if option.Name != nil {
					o.Name = *option.Name
				}
				f.Options = append(f.Options, o)
			}
			v.Features = append(v.Features, f)
		}
	}
	return v
}

func modelFamily(api protocol.ConversationModelApi) string {
	switch api.Family() {
	case "anthropic":
		return "anthropic"
	case "responses", "chat_completions", "azure_open_ai":
		return "openai"
	case "gemini":
		return "gemini"
	default:
		return "other"
	}
}

func (r *Runtime) readModelScores(source modelSource, providers []protocol.ConversationProvider) {
	cache := &r.modelScores
	if cache.reading || cache.ready && cache.source == source {
		return
	}
	var refs []protocol.ConversationCatalogScoresModelRef
	for _, p := range providers {
		if p.Enabled {
			for _, model := range p.Models {
				refs = append(refs, protocol.ConversationCatalogScoresModelRef{Provider: p.ID, Model: model.ID})
			}
		}
	}
	if len(refs) == 0 {
		cache.source, cache.ready, cache.values = source, true, nil
		cache.revision++
		return
	}
	cache.reading = true
	r.execute(protocol.Command{Kind: "read_model_scores", Data: refs}, func(_ *ui.Context, out protocol.Output, err error) {
		cache.reading = false
		if _, current := r.modelInputs(); current != source {
			r.projectModels()
			return
		}
		if err != nil {
			// The next projection reads again.
			return
		}
		result, ok := detail[protocol.ConversationCatalogScores](out.Data)
		if !ok {
			return
		}
		scores := map[string][]protocol.ConversationCatalogScoresEffortScore{}
		for _, model := range result.Scores {
			scores[modelKey(model.Provider, model.Model)] = model.Efforts
		}
		cache.source, cache.ready, cache.values = source, true, scores
		cache.revision++
		r.projectModels()
	})
}

func (r *Runtime) measuredScore(key string, effort protocol.Effort) *composer.Score {
	scores := r.modelScores.values[key]
	if len(scores) == 0 {
		return nil
	}
	rank := func(e protocol.Effort) int {
		switch e.Kind {
		case "disabled":
			return 0
		case "minimal":
			return 1
		case "low":
			return 2
		case "medium":
			return 3
		case "high":
			return 4
		case "xhigh":
			return 5
		case "max":
			return 6
		default:
			return -1
		}
	}
	selected := -1
	for i, s := range scores {
		if effortName(s.Effort) == effortName(effort) {
			selected = i
			break
		}
	}
	if selected < 0 {
		requested := rank(effort)
		best := -1
		for i, s := range scores {
			if n := rank(s.Effort); n >= 0 && requested >= n && n > best {
				selected, best = i, n
			}
		}
	}
	if selected < 0 {
		best := int(^uint(0) >> 1)
		for i, s := range scores {
			if n := rank(s.Effort); n >= 0 && n < best {
				selected, best = i, n
			}
		}
	}
	if selected < 0 {
		for i, s := range scores {
			if s.Effort.Kind == "default" {
				selected = i
				break
			}
		}
	}
	if selected < 0 {
		return nil
	}
	s := scores[selected]
	value := &composer.Score{Overall: float64(s.Overall), Tested: effortName(s.Effort)}
	for i, v := range s.Axes {
		value.Axes[i] = float64(v) / 10
	}
	return value
}
