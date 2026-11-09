package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"reflect"
	"sort"
	"time"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type providerScoreStored struct {
	Status  protocol.ConversationCatalogScoresScoreStatus
	ETag    *string
	Entries uint32
}

func (value providerScoreStored) MarshalJSON() ([]byte, error) {
	raw, err := json.Marshal(value.Status)
	if err != nil {
		return nil, err
	}
	var body map[string]any
	if err := json.Unmarshal(raw, &body); err != nil {
		return nil, err
	}
	body["etag"] = value.ETag
	body["entries"] = value.Entries
	return json.Marshal(body)
}
func (value *providerScoreStored) UnmarshalJSON(data []byte) error {
	value.Status = protocol.DefaultConversationCatalogScoresScoreStatus()
	if err := json.Unmarshal(data, &value.Status); err != nil {
		return err
	}
	var extra struct {
		ETag    *string `json:"etag"`
		Entries uint32  `json:"entries"`
	}
	if err := json.Unmarshal(data, &extra); err != nil {
		return err
	}
	value.ETag, value.Entries = extra.ETag, extra.Entries
	return nil
}
func providerScoreState(ctx context.Context, db *sql.Conn) (providerScoreStored, error) {
	value, err := readBody[providerScoreStored](ctx, db, "SELECT body FROM model_score_status WHERE id=1")
	if err != nil {
		return providerScoreStored{}, err
	}
	if value != nil {
		return *value, nil
	}
	return providerScoreStored{Status: protocol.DefaultConversationCatalogScoresScoreStatus()}, nil
}
func providerScoreRows(ctx context.Context, db *sql.Conn) (map[string]protocol.ConversationCatalogScoresEffortScore, map[string]string, error) {
	scores := map[string]protocol.ConversationCatalogScoresEffortScore{}
	aliases := map[string]string{}
	rows, err := db.QueryContext(ctx, "SELECT model,effort,body FROM model_scores")
	if err != nil {
		return nil, nil, err
	}
	for rows.Next() {
		var model, effort string
		var body []byte
		if err := rows.Scan(&model, &effort, &body); err != nil {
			rows.Close()
			return nil, nil, err
		}
		var value protocol.ConversationCatalogScoresEffortScore
		if err := json.Unmarshal(body, &value); err != nil {
			rows.Close()
			return nil, nil, err
		}
		scores[model+"\x00"+effort] = value
	}
	err = rows.Err()
	rows.Close()
	if err != nil {
		return nil, nil, err
	}
	rows, err = db.QueryContext(ctx, "SELECT alias,model FROM model_score_aliases")
	if err != nil {
		return nil, nil, err
	}
	for rows.Next() {
		var alias, model string
		if err := rows.Scan(&alias, &model); err != nil {
			rows.Close()
			return nil, nil, err
		}
		aliases[alias] = model
	}
	err = rows.Err()
	rows.Close()
	return scores, aliases, err
}
func applyProviderScores(ctx context.Context, db *sql.Conn, download *providers.ScoresDownload, revision uint64) error {
	state, err := providerScoreState(ctx, db)
	if err != nil {
		return err
	}
	if state.Status.Revision != revision {
		return nil
	}
	now := uint64(time.Now().UnixMilli())
	state.Status.CheckedAtMs = &now
	if download.Fault != nil {
		state.Status.Fault = download.Fault
	} else if download.Unchanged {
		state.Status.Fault = nil
	} else if dataset := download.Dataset; dataset != nil {
		current, aliases, err := providerScoreRows(ctx, db)
		if err != nil {
			return err
		}
		next := map[string]protocol.ConversationCatalogScoresEffortScore{}
		models := map[string]bool{}
		for _, entry := range dataset.Entries {
			next[entry.Model+"\x00"+entry.Effort] = entry.Score
			models[entry.Model] = true
		}
		changed := !reflect.DeepEqual(current, next) || !reflect.DeepEqual(aliases, dataset.Aliases) || state.Status.Batch == nil || *state.Status.Batch != dataset.Batch || state.Status.PublishedAt == nil || *state.Status.PublishedAt != dataset.PublishedAt
		if changed {
			if _, err := db.ExecContext(ctx, "DELETE FROM model_scores"); err != nil {
				return err
			}
			if _, err := db.ExecContext(ctx, "DELETE FROM model_score_aliases"); err != nil {
				return err
			}
			for _, entry := range dataset.Entries {
				body, err := encode(entry.Score)
				if err != nil {
					return err
				}
				if _, err := db.ExecContext(ctx, "INSERT INTO model_scores(model,effort,body) VALUES(?1,?2,?3)", entry.Model, entry.Effort, body); err != nil {
					return err
				}
			}
			for alias, model := range dataset.Aliases {
				if _, err := db.ExecContext(ctx, "INSERT INTO model_score_aliases(alias,model) VALUES(?1,?2)", alias, model); err != nil {
					return err
				}
			}
			state.Status.Revision, err = nextRevision(state.Status.Revision)
			if err != nil {
				return err
			}
			state.Status.UpdatedAtMs = &now
		}
		state.ETag = download.ETag
		state.Status.Batch = &dataset.Batch
		state.Status.PublishedAt = &dataset.PublishedAt
		state.Status.Models = uint32(len(models))
		state.Entries = uint32(len(dataset.Entries))
		state.Status.Fault = nil
	}
	body, err := encode(state)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO model_score_status(id,body) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET body=excluded.body", body)
	return err
}
func readProviderScores(ctx context.Context, db *sql.Conn, references []protocol.ConversationCatalogScoresModelRef) (protocol.ConversationCatalogScores, error) {
	result := protocol.DefaultConversationCatalogScores()
	if len(references) > 512 {
		return result, invalid("too many model score references")
	}
	state, err := providerScoreState(ctx, db)
	if err != nil {
		return result, err
	}
	result.Revision = state.Status.Revision
	result.Batch = state.Status.Batch
	result.PublishedAt = state.Status.PublishedAt
	if state.Entries == 0 || len(references) == 0 {
		return result, nil
	}
	rows, aliases, err := providerScoreRows(ctx, db)
	if err != nil {
		return result, err
	}
	models := []string{}
	unique := map[string]bool{}
	for key := range rows {
		for i, c := range key {
			if c == 0 {
				model := key[:i]
				if !unique[model] {
					unique[model] = true
					models = append(models, model)
				}
				break
			}
		}
	}
	sort.Strings(models)
	seen := map[string]bool{}
	for _, reference := range references {
		key := reference.Provider.String() + "\x00" + reference.Model
		if seen[key] {
			continue
		}
		seen[key] = true
		saved, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", reference.Provider.String())
		if err != nil {
			return result, err
		}
		if saved == nil || !saved.Listed || saved.Provider == nil {
			continue
		}
		provider := saved.Provider
		var model *protocol.ConversationModel
		for i := range provider.Models {
			if provider.Models[i].ID == reference.Model {
				model = &provider.Models[i]
				break
			}
		}
		if model == nil {
			continue
		}
		candidates := []string{model.ID}
		if model.DisplayName != nil {
			candidates = append(candidates, *model.DisplayName)
		}
		if provider.Preset != nil {
			entries, err := db.QueryContext(ctx, "SELECT id,body FROM model_catalog WHERE provider=?1", provider.Preset.ID)
			if err != nil {
				return result, err
			}
			for entries.Next() {
				var id string
				var body []byte
				if err := entries.Scan(&id, &body); err != nil {
					entries.Close()
					return result, err
				}
				var raw map[string]json.RawMessage
				if err := json.Unmarshal(body, &raw); err != nil {
					entries.Close()
					return result, err
				}
				var name *string
				json.Unmarshal(raw["name"], &name)
				if id == model.ID || model.DisplayName != nil && name != nil && *model.DisplayName == *name {
					candidates = append(candidates, id)
				}
			}
			err = entries.Err()
			entries.Close()
			if err != nil {
				return result, err
			}
		}
		matched := providers.ResolveScore(candidates, models, aliases)
		if matched == nil {
			continue
		}
		score := protocol.ConversationCatalogScoresModelScore{Provider: reference.Provider, Model: reference.Model, Efforts: []protocol.ConversationCatalogScoresEffortScore{}}
		keys := []string{}
		for key := range rows {
			keys = append(keys, key)
		}
		sort.Strings(keys)
		for _, key := range keys {
			if len(key) > len(*matched) && key[:len(*matched)+1] == *matched+"\x00" {
				score.Efforts = append(score.Efforts, rows[key])
			}
		}
		result.Scores = append(result.Scores, score)
	}
	return result, nil
}
