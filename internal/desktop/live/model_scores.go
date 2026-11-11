package live

import (
	"encoding/json"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func modelFamily(api protocol.ConversationModelApi) string {
	switch api {
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

func (r *Runtime) readModelScores() {
	snapshot := r.snapshot()
	var refs []protocol.ConversationCatalogScoresModelRef
	for _, p := range snapshot.Providers {
		if p.Enabled {
			for _, model := range p.Models {
				refs = append(refs, protocol.ConversationCatalogScoresModelRef{Provider: p.ID, Model: model.ID})
			}
		}
	}
	encoded, _ := json.Marshal(struct {
		Revision  uint64
		Providers []protocol.ConversationProvider
	}{snapshot.ModelCatalog.Scores.Revision, snapshot.Providers})
	key := string(encoded)
	if r.modelScoreKey == key {
		return
	}
	r.modelScoreKey = key
	if len(refs) == 0 {
		r.modelScores = nil
		return
	}
	r.execute(protocol.Command{Kind: "read_model_scores", Data: refs}, func(_ *ui.Context, out protocol.Output, err error) {
		if err != nil || r.modelScoreKey != key {
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
		r.modelScores = scores
		r.projectModels()
	})
}

func (r *Runtime) measuredScore(key string, effort protocol.Effort) *composer.Score {
	scores := r.modelScores[key]
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
