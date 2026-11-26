package statisticsdesktop

import (
	"cmp"
	"encoding/json"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	breakdownVisible   = 6
	breakdownNameWidth = 280
	breakdownNameRoom  = breakdownNameWidth - 20
)

type scopedUsageGroup struct {
	Node  string
	Group protocol.UsageGroup
}

type usageCatalogNames struct {
	providers map[string]string
	projects  map[string]string
	hosts     map[string]string
}

type breakdownItem struct {
	id        string
	name      string
	note      string
	provider  string
	responses uint64
	reported  uint64
	tokens    *protocol.ConversationUsage
	hasTotal  bool
	total     uint64
	hasHit    bool
	hit       float64
	hasShare  bool
	share     float64
	hasCost   bool
	cost      string
	costData  *protocol.UsageCost
}

func estimateTextWidth(text string) int {
	total := 0
	for _, r := range text {
		if r > 0x2e7f {
			total += 15
		} else {
			total += 8
		}
	}
	return total
}

// breakdownAnnotated reports whether every row's name and note fit untruncated
// in the name column: notes show for all rows or none.
func breakdownAnnotated(rows []breakdownItem) bool {
	for _, r := range rows {
		if r.note != "" && estimateTextWidth(r.name)+6+estimateTextWidth(r.note) > breakdownNameRoom {
			return false
		}
	}
	return true
}

func extractModelKey(data any) (protocol.UsageKeyModelPayload, bool) {
	switch v := data.(type) {
	case protocol.UsageKeyModelPayload:
		return v, true
	case *protocol.UsageKeyModelPayload:
		if v != nil {
			return *v, true
		}
	}
	return protocol.UsageKeyModelPayload{}, false
}

func extractProjectID(data any) (protocol.ProjectID, bool) {
	switch v := data.(type) {
	case protocol.ProjectID:
		return v, true
	case *protocol.ProjectID:
		if v != nil {
			return *v, true
		}
	}
	return protocol.ProjectID{}, false
}

func extractProviderID(data any) (protocol.ProviderID, bool) {
	switch v := data.(type) {
	case protocol.ProviderID:
		return v, true
	case *protocol.ProviderID:
		if v != nil {
			return *v, true
		}
	}
	return protocol.ProviderID{}, false
}

func shortPrefix(id string) string {
	if len(id) > 8 {
		return id[:8]
	}
	return id
}

func lookupProviderName(group protocol.UsageGroup, prefix, provID string, names usageCatalogNames) string {
	if group.Name != nil && *group.Name != "" {
		return *group.Name
	}
	if prefix != "" {
		if v, ok := names.providers[prefix+provID]; ok && v != "" {
			return v
		}
	}
	if v, ok := names.providers[provID]; ok && v != "" {
		return v
	}
	return shortPrefix(provID)
}

func lookupProjectName(group protocol.UsageGroup, prefix, projID string, names usageCatalogNames) string {
	if group.Name != nil && *group.Name != "" {
		return *group.Name
	}
	if prefix != "" {
		if v, ok := names.projects[prefix+projID]; ok && v != "" {
			return v
		}
	}
	if v, ok := names.projects[projID]; ok && v != "" {
		return v
	}
	return shortPrefix(projID)
}

type breakdownIdentity struct {
	id       string
	name     string
	note     string
	provider string
}

func groupIdentity(sg scopedUsageGroup, mode string, names usageCatalogNames, t translateFunc) (breakdownIdentity, bool) {
	g := sg.Group
	prefix := ""
	host := ""
	if sg.Node != "" {
		prefix = sg.Node + ":"
		host = names.hosts[sg.Node]
	}
	switch g.Key.Kind {
	case "model":
		payload, ok := extractModelKey(g.Key.Data)
		if !ok {
			return breakdownIdentity{}, false
		}
		provID := payload.Provider.String()
		provName := lookupProviderName(g, prefix, provID, names)
		switch mode {
		case "model":
			var noteParts []string
			if provName != "" {
				noteParts = append(noteParts, provName)
			}
			if host != "" {
				noteParts = append(noteParts, host)
			}
			return breakdownIdentity{
				id:       prefix + provID + ":" + payload.Model,
				name:     payload.Model,
				note:     strings.Join(noteParts, " · "),
				provider: provID,
			}, true
		case "provider":
			return breakdownIdentity{
				id:       prefix + provID,
				name:     provName,
				note:     host,
				provider: provID,
			}, true
		default:
			return breakdownIdentity{}, false
		}
	case "session":
		if mode != "project" {
			return breakdownIdentity{}, false
		}
		// Conversations without a project, connection, or assistant merge into one Unassigned row.
		return breakdownIdentity{
			id:   "session",
			name: t("usage_unassigned"),
		}, true
	case "project":
		if mode != "project" {
			return breakdownIdentity{}, false
		}
		projID, ok := extractProjectID(g.Key.Data)
		if !ok {
			return breakdownIdentity{}, false
		}
		idStr := projID.String()
		return breakdownIdentity{
			id:   prefix + "project:" + idStr,
			name: lookupProjectName(g, prefix, idStr, names),
			note: host,
		}, true
	case "connection", "assistant":
		if mode != "project" {
			return breakdownIdentity{}, false
		}
		raw, _ := json.Marshal(g.Key.Data)
		name := t("usage_unassigned")
		if g.Name != nil && *g.Name != "" {
			name = *g.Name
		}
		return breakdownIdentity{
			id:   prefix + g.Key.Kind + ":" + string(raw),
			name: name,
			note: host,
		}, true
	default:
		return breakdownIdentity{}, false
	}
}

func buildBreakdown(groups []scopedUsageGroup, mode string, names usageCatalogNames, t translateFunc) []breakdownItem {
	byID := map[string]*breakdownItem{}
	for _, sg := range groups {
		ident, ok := groupIdentity(sg, mode, names, t)
		if !ok {
			continue
		}
		row := byID[ident.id]
		if row == nil {
			row = &breakdownItem{
				id:       ident.id,
				name:     ident.name,
				note:     ident.note,
				provider: ident.provider,
			}
			byID[ident.id] = row
		}
		cov := tokenCoverage(sg.Group.Metrics)
		row.responses += cov.responses
		row.reported += cov.reported
		if cov.tokens != nil {
			if row.tokens == nil {
				row.tokens = &protocol.ConversationUsage{}
			}
			row.tokens.Input += cov.tokens.Input
			row.tokens.Output += cov.tokens.Output
			row.tokens.CachedInput += cov.tokens.CachedInput
			row.tokens.Reasoning += cov.tokens.Reasoning
		}
		if sg.Group.Metrics.Cost != nil {
			if row.costData == nil {
				row.costData = &protocol.UsageCost{}
			}
			row.costData.UsdMicros += sg.Group.Metrics.Cost.UsdMicros
			row.costData.Responses += sg.Group.Metrics.Cost.Responses
		}
	}

	list := make([]breakdownItem, 0, len(byID))
	var knownSum uint64
	for _, row := range byID {
		if p, ok := tokenParts(row.tokens); ok {
			row.hasTotal = true
			row.total = p[0] + p[1] + p[2]
			knownSum += row.total
			if row.reported == row.responses {
				row.hit, row.hasHit = ratioPercent(p[0], row.tokens.Input)
			}
		}
		if row.costData != nil {
			row.cost, row.hasCost = formatCost(row.costData, row.responses)
		}
		list = append(list, *row)
	}
	for i := range list {
		if list[i].hasTotal {
			list[i].share, list[i].hasShare = ratioPercent(list[i].total, knownSum)
		}
	}
	slices.SortFunc(list, func(a, b breakdownItem) int {
		if a.hasTotal != b.hasTotal {
			if a.hasTotal {
				return -1
			}
			return 1
		}
		if a.hasTotal && a.total != b.total {
			return cmp.Compare(b.total, a.total)
		}
		if c := cmp.Compare(a.name, b.name); c != 0 {
			return c
		}
		return cmp.Compare(a.id, b.id)
	})
	return list
}
