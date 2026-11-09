package providers

import (
	"context"
	"encoding/json"
	"math"
	"net/http"
	"sort"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const scoresURL = "https://modeldial.com/data/overview.json"

type ScoreEntry struct {
	Model  string
	Effort string
	Score  protocol.ConversationCatalogScoresEffortScore
}
type ScoresDataset struct {
	Batch       string
	PublishedAt string
	Entries     []ScoreEntry
	Aliases     map[string]string
}
type ScoresDownload struct {
	Unchanged bool
	Dataset   *ScoresDataset
	ETag      *string
	Fault     *protocol.Fault
}

func downloadScores(ctx context.Context, etag *string) (*ScoresDownload, error) {
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, scoresURL, nil)
	if err != nil {
		return nil, unavailable("model scores HTTP client failed")
	}
	request.Header.Set("User-Agent", "Sailry/0.1.0 model-catalog")
	if etag != nil {
		request.Header.Set("If-None-Match", *etag)
	}
	response, err := httpClient(30 * time.Second).Do(request)
	if err != nil {
		return nil, unavailable("model scores download failed")
	}
	if response.StatusCode == http.StatusNotModified {
		response.Body.Close()
		if etag == nil {
			return nil, unavailable("model scores returned an unexpected unchanged response")
		}
		return &ScoresDownload{Unchanged: true}, nil
	}
	var next *string
	if header := response.Header.Get("ETag"); header != "" {
		next = &header
	}
	raw, err := readJSON(response, 4*1024*1024)
	if err != nil {
		return nil, err
	}
	dataset, err := ParseScores(raw)
	if err != nil {
		return nil, err
	}
	return &ScoresDownload{Dataset: dataset, ETag: next}, nil
}
func ParseScores(raw []byte) (*ScoresDataset, error) {
	if len(raw) > 4*1024*1024 {
		return nil, unavailable("model scores exceed their size limit")
	}
	type metric struct {
		Elapsed *uint64  `json:"elapsed_ms"`
		Cost    *float32 `json:"estimated_api_cost_usd"`
	}
	type row struct {
		Candidate       string   `json:"candidate_id"`
		Model           string   `json:"model_id"`
		Effort          string   `json:"reasoning_effort"`
		Overall         *float32 `json:"overall_score"`
		Backend         *float64 `json:"backend_score"`
		Frontend        *float64 `json:"frontend_score"`
		Knowledge       *float64 `json:"knowledge_score"`
		Aesthetic       *float64 `json:"aesthetic_score"`
		Maintainability *float64 `json:"maintainability_score"`
	}
	var document struct {
		Snapshot struct {
			Batch     string `json:"batch_id"`
			Published string `json:"published_at"`
			Entries   []row  `json:"entries"`
		} `json:"snapshot"`
		Metrics map[string]map[string]metric `json:"metrics"`
		Names   map[string]string            `json:"displayNames"`
		Efforts map[string]map[string]string `json:"effortDisplayNames"`
	}
	if json.Unmarshal(raw, &document) != nil {
		return nil, unavailable("model scores JSON is invalid")
	}
	snapshot := document.Snapshot
	if len(snapshot.Entries) > 2000 || snapshot.Batch == "" || len(snapshot.Batch) > 256 || snapshot.Published == "" || len(snapshot.Published) > 64 {
		return nil, unavailable("model scores snapshot is invalid")
	}
	sort.SliceStable(snapshot.Entries, func(i, j int) bool { return snapshot.Entries[i].Candidate < snapshot.Entries[j].Candidate })
	result := &ScoresDataset{Batch: snapshot.Batch, PublishedAt: snapshot.Published, Aliases: map[string]string{}}
	seen := map[string]bool{}
	models := map[string]bool{}
	for _, entry := range snapshot.Entries {
		if entry.Overall == nil || !validScore(float64(*entry.Overall)) || entry.Model == "" || len(entry.Model) > 256 {
			continue
		}
		axes := []*float64{entry.Backend, entry.Frontend, entry.Knowledge, entry.Aesthetic, entry.Maintainability}
		valid := true
		var rounded [5]uint8
		for i, axis := range axes {
			if axis == nil || !validScore(*axis) {
				valid = false
				break
			}
			rounded[i] = uint8(math.Round(*axis))
		}
		if !valid {
			continue
		}
		effort := entry.Effort
		if override, ok := document.Efforts[entry.Model][effort]; ok {
			effort = override
		}
		switch effort {
		case "low", "medium", "high", "xhigh", "max":
		case "default", "unknown":
			effort = "default"
		default:
			continue
		}
		key := entry.Model + "\x00" + effort
		if seen[key] {
			continue
		}
		seen[key] = true
		models[entry.Model] = true
		score := protocol.ConversationCatalogScoresEffortScore{Effort: protocol.Effort{Kind: effort}, Overall: *entry.Overall, Axes: rounded}
		var elapsed *uint64
		var cost *float32
		badElapsed, badCost := false, false
		for _, axis := range []string{"backend", "frontend", "knowledge", "aesthetic", "maintainability"} {
			metric := document.Metrics[entry.Candidate][axis]
			if metric.Elapsed != nil {
				before := uint64(0)
				if elapsed != nil {
					before = *elapsed
				}
				sum := before + *metric.Elapsed
				if sum < before {
					badElapsed = true
				} else {
					elapsed = &sum
				}
			}
			if metric.Cost != nil {
				before := float32(0)
				if cost != nil {
					before = *cost
				}
				sum := before + *metric.Cost
				if math.IsInf(float64(sum), 0) || math.IsNaN(float64(*metric.Cost)) || math.IsInf(float64(*metric.Cost), 0) || *metric.Cost < 0 {
					badCost = true
				} else {
					cost = &sum
				}
			}
		}
		if !badElapsed {
			score.ElapsedMs = elapsed
		}
		if !badCost {
			score.CostUsd = cost
		}
		result.Entries = append(result.Entries, ScoreEntry{Model: entry.Model, Effort: effort, Score: score})
	}
	sort.Slice(result.Entries, func(i, j int) bool {
		a, b := result.Entries[i], result.Entries[j]
		if a.Model == b.Model {
			return a.Effort < b.Effort
		}
		return a.Model < b.Model
	})
	conflicts := map[string]bool{}
	for model, name := range document.Names {
		if models[model] {
			alias := NormalizeScoreName(name)
			if _, exists := result.Aliases[alias]; exists {
				conflicts[alias] = true
			}
			result.Aliases[alias] = model
		}
	}
	for alias := range conflicts {
		delete(result.Aliases, alias)
	}
	return result, nil
}
func validScore(value float64) bool {
	return !math.IsNaN(value) && !math.IsInf(value, 0) && value >= 0 && value <= 100
}
func NormalizeScoreName(name string) string {
	name = strings.ToLower(strings.TrimSpace(name))
	if at := strings.Index(name, "/"); at >= 0 {
		name = name[at+1:]
	}
	name = strings.NewReplacer(".", "-", "_", "-").Replace(name)
	name = strings.TrimSuffix(name, "-latest")
	parts := strings.Split(name, "-")
	n := len(parts)
	digits := func(value string) bool {
		if value == "" {
			return false
		}
		for _, c := range value {
			if c < '0' || c > '9' {
				return false
			}
		}
		return true
	}
	if n >= 4 && len(parts[n-3]) == 4 && len(parts[n-2]) == 2 && len(parts[n-1]) == 2 && digits(parts[n-3]) && digits(parts[n-2]) && digits(parts[n-1]) {
		return strings.Join(parts[:n-3], "-")
	}
	if n >= 2 && len(parts[n-1]) >= 4 && len(parts[n-1]) <= 8 && digits(parts[n-1]) {
		return strings.Join(parts[:n-1], "-")
	}
	return name
}
func ResolveScore(candidates []string, models []string, aliases map[string]string) *string {
	matches := map[string]bool{}
	for _, candidate := range candidates {
		normalized := NormalizeScoreName(candidate)
		direct := false
		for _, model := range models {
			if NormalizeScoreName(model) == normalized {
				matches[model] = true
				direct = true
			}
		}
		if direct {
			continue
		}
		for alias, model := range aliases {
			if NormalizeScoreName(alias) == normalized {
				matches[model] = true
			}
		}
		if normalized == "k2-8" {
			for _, model := range models {
				if NormalizeScoreName(model) == "kimi-for-coding" {
					matches[model] = true
				}
			}
		}
	}
	if len(matches) != 1 {
		return nil
	}
	for model := range matches {
		return &model
	}
	return nil
}
