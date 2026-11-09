package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"reflect"
	"sort"
	"strings"
	"time"
	"unicode"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func ReadProviderCatalogStatus(ctx context.Context, db *sql.Conn) (protocol.ConversationCatalogStatus, error) {
	status := protocol.DefaultConversationCatalogStatus()
	saved, err := readBody[protocol.ConversationCatalogStatus](ctx, db, "SELECT body FROM model_catalog_status WHERE id=1")
	if err != nil {
		return status, err
	}
	if saved != nil {
		status = *saved
	}
	scores, err := providerScoreState(ctx, db)
	status.Scores = scores.Status
	return status, err
}
func writeProviderCatalogStatus(ctx context.Context, db *sql.Conn, status protocol.ConversationCatalogStatus) error {
	raw, err := encode(status)
	if err != nil {
		return err
	}
	var body map[string]json.RawMessage
	if err := json.Unmarshal(raw, &body); err != nil {
		return err
	}
	delete(body, "scores")
	raw, err = encode(body)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO model_catalog_status(id,body) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET body=excluded.body", raw)
	return err
}
func applyProviderCatalog(ctx context.Context, db *sql.Conn, prepared *providers.Prepared) (protocol.Output, []protocol.Event, error) {
	if prepared == nil || prepared.Catalog == nil {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeInternal, "model catalog download is missing")
	}
	status, err := ReadProviderCatalogStatus(ctx, db)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	download := prepared.Catalog
	var events []protocol.Event
	if download.Fault != nil {
		status.Fault = download.Fault
	} else {
		status.Revision, err = nextRevision(status.Revision)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		now := uint64(time.Now().UnixMilli())
		status.UpdatedAtMs = &now
		status.Providers = download.Providers
		status.Models = uint32(len(download.Entries))
		status.Bytes = download.Bytes
		status.Fault = nil
		previous := map[string]bool{}
		rows, err := db.QueryContext(ctx, "SELECT provider,id FROM model_catalog")
		if err != nil {
			return protocol.Output{}, nil, err
		}
		for rows.Next() {
			var provider, id string
			if err := rows.Scan(&provider, &id); err != nil {
				rows.Close()
				return protocol.Output{}, nil, err
			}
			previous[provider+"\x00"+id] = true
		}
		err = rows.Err()
		rows.Close()
		if err != nil {
			return protocol.Output{}, nil, err
		}
		for _, entry := range download.Entries {
			if _, err := db.ExecContext(ctx, "INSERT INTO model_catalog(provider,id,body) VALUES(?1,?2,?3) ON CONFLICT(provider,id) DO UPDATE SET body=excluded.body WHERE body<>excluded.body", entry.Provider, entry.ID, []byte(entry.Body)); err != nil {
				return protocol.Output{}, nil, err
			}
			delete(previous, entry.Provider+"\x00"+entry.ID)
		}
		for key := range previous {
			parts := strings.SplitN(key, "\x00", 2)
			if _, err := db.ExecContext(ctx, "DELETE FROM model_catalog WHERE provider=?1 AND id=?2", parts[0], parts[1]); err != nil {
				return protocol.Output{}, nil, err
			}
		}
	}
	if err := writeProviderCatalogStatus(ctx, db, status); err != nil {
		return protocol.Output{}, nil, err
	}
	if prepared.Scores != nil {
		if err := applyProviderScores(ctx, db, prepared.Scores, prepared.ScoreRevision); err != nil {
			return protocol.Output{}, nil, err
		}
	}
	if download.Fault == nil {
		events, err = refreshProviderMetadata(ctx, db)
		if err != nil {
			return protocol.Output{}, nil, err
		}
	}
	status, err = ReadProviderCatalogStatus(ctx, db)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	return protocol.Output{Kind: "catalog_status", Data: status}, append(events, protocol.Event{Kind: "model_catalog_changed", Data: status}), nil
}
func readProviderCatalog(ctx context.Context, db *sql.Conn, query protocol.ConversationCatalogQuery) (protocol.ConversationCatalogPage, error) {
	result := protocol.DefaultConversationCatalogPage()
	if query.Provider != nil && (!boundedCatalogID(*query.Provider, 128)) {
		return result, invalid("catalog identifier is invalid")
	}
	if len(query.IDs) > 100 || query.Provider == nil && len(query.IDs) == 0 {
		return result, invalid("catalog lookup requires bounded model identifiers")
	}
	for _, id := range query.IDs {
		if !boundedCatalogID(id, 256) {
			return result, invalid("catalog identifier is invalid")
		}
	}
	if query.Limit == 0 || query.Limit > 100 || query.After != nil && query.Revision == nil {
		return result, invalid("catalog page requires bounded size and revision")
	}
	if query.After != nil && !boundedCatalogID(*query.After, 256) {
		return result, invalid("catalog identifier is invalid")
	}
	status, err := ReadProviderCatalogStatus(ctx, db)
	if err != nil {
		return result, err
	}
	if status.Revision == 0 {
		return result, fault(protocol.ErrorCodeNotConfigured, "model catalog has not been refreshed")
	}
	if query.Revision != nil && *query.Revision != status.Revision {
		return result, fault(protocol.ErrorCodeRevisionConflict, "model catalog revision changed")
	}
	result.Revision = status.Revision
	var models []protocol.ConversationCatalogModel
	if len(query.IDs) == 0 {
		after := ""
		if query.After != nil {
			after = *query.After
		}
		rows, err := db.QueryContext(ctx, "SELECT body FROM model_catalog WHERE provider=?1 AND id>?2 ORDER BY id LIMIT ?3", *query.Provider, after, int(query.Limit)+1)
		if err != nil {
			return result, err
		}
		for rows.Next() {
			var body []byte
			if err := rows.Scan(&body); err != nil {
				rows.Close()
				return result, err
			}
			model, err := providers.CatalogModel(body)
			if err != nil {
				rows.Close()
				return result, err
			}
			models = append(models, model)
		}
		err = rows.Err()
		rows.Close()
		if err != nil {
			return result, err
		}
	} else {
		ids := append([]string(nil), query.IDs...)
		sort.Strings(ids)
		seen := map[string]bool{}
		for _, id := range ids {
			if seen[id] || query.After != nil && id <= *query.After {
				continue
			}
			seen[id] = true
			rows, err := db.QueryContext(ctx, "SELECT body FROM model_catalog WHERE id=?1 AND (?2 IS NULL OR provider=?2) ORDER BY provider", id, query.Provider)
			if err != nil {
				return result, err
			}
			var combined *protocol.ConversationCatalogModel
			presence := [3]bool{}
			for rows.Next() {
				var body []byte
				if err := rows.Scan(&body); err != nil {
					rows.Close()
					return result, err
				}
				model, err := providers.CatalogModel(body)
				if err != nil {
					rows.Close()
					return result, err
				}
				if combined == nil {
					combined = &model
					presence = [3]bool{len(model.Inputs) > 0, len(model.Outputs) > 0, len(model.Options) > 0}
				} else {
					mergeCatalogModel(combined, model, &presence)
				}
			}
			err = rows.Err()
			rows.Close()
			if err != nil {
				return result, err
			}
			if combined != nil {
				models = append(models, *combined)
			}
		}
	}
	bytes := 0
	for _, model := range models {
		body, err := encode(model)
		if err != nil {
			return result, err
		}
		if len(result.Models) == int(query.Limit) || bytes+len(body)+1 > 512*1024 {
			if len(result.Models) > 0 {
				last := result.Models[len(result.Models)-1].ID
				result.Next = &last
			}
			break
		}
		result.Models = append(result.Models, model)
		bytes += len(body) + 1
	}
	return result, nil
}
func boundedCatalogID(value string, limit int) bool {
	if strings.TrimSpace(value) == "" || len(value) > limit {
		return false
	}
	for _, r := range value {
		if unicode.IsControl(r) {
			return false
		}
	}
	return true
}
func catalogMinimum[T ~uint32 | ~int32](left, right *T) *T {
	if left == nil {
		return right
	}
	if right == nil {
		return left
	}
	if *left < *right {
		return left
	}
	return right
}
func catalogBoolean(left, right *bool) *bool {
	if left == nil {
		return right
	}
	if right == nil {
		return left
	}
	value := *left && *right
	return &value
}
func intersectCatalogStrings(left *[]string, right []string, seen *bool) {
	if len(right) == 0 {
		return
	}
	if !*seen {
		*left = right
		*seen = true
		return
	}
	result := []string{}
	for _, a := range *left {
		for _, b := range right {
			if a == b {
				result = append(result, a)
				break
			}
		}
	}
	*left = result
}
func mergeCatalogModel(left *protocol.ConversationCatalogModel, right protocol.ConversationCatalogModel, seen *[3]bool) {
	left.Context = catalogMinimum(left.Context, right.Context)
	left.Output = catalogMinimum(left.Output, right.Output)
	left.Tools = catalogBoolean(left.Tools, right.Tools)
	left.Reasoning = catalogBoolean(left.Reasoning, right.Reasoning)
	intersectCatalogStrings(&left.Inputs, right.Inputs, &seen[0])
	intersectCatalogStrings(&left.Outputs, right.Outputs, &seen[1])
	if len(right.Options) == 0 {
		return
	}
	if !seen[2] {
		left.Options = right.Options
		seen[2] = true
		return
	}
	result := []protocol.ConversationCatalogReasoning{}
	for _, a := range left.Options {
		for _, b := range right.Options {
			if a.Kind != b.Kind {
				continue
			}
			switch a.Kind {
			case "toggle":
				result = append(result, a)
			case "effort":
				var av, bv protocol.ConversationCatalogReasoningEffortPayload
				decodeProviderData(a.Data, &av)
				decodeProviderData(b.Data, &bv)
				values := []*string{}
				for _, x := range av.Values {
					for _, y := range bv.Values {
						if reflect.DeepEqual(x, y) {
							values = append(values, x)
							break
						}
					}
				}
				if len(values) > 0 {
					result = append(result, protocol.ConversationCatalogReasoning{Kind: "effort", Data: protocol.ConversationCatalogReasoningEffortPayload{Values: values}})
				}
			case "budget_tokens":
				var av, bv protocol.ConversationCatalogReasoningBudgetTokensPayload
				decodeProviderData(a.Data, &av)
				decodeProviderData(b.Data, &bv)
				minimum := av.Min
				if minimum == nil || bv.Min != nil && *bv.Min > *minimum {
					minimum = bv.Min
				}
				maximum := catalogMinimum(av.Max, bv.Max)
				if minimum == nil || maximum == nil || *minimum < 0 || uint32(*minimum) <= *maximum {
					result = append(result, protocol.ConversationCatalogReasoning{Kind: "budget_tokens", Data: protocol.ConversationCatalogReasoningBudgetTokensPayload{Min: minimum, Max: maximum}})
				}
			}
			break
		}
	}
	left.Options = result
}
func decodeProviderData(source, target any) error {
	raw, err := json.Marshal(source)
	if err != nil {
		return err
	}
	return json.Unmarshal(raw, target)
}
