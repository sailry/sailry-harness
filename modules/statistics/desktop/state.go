package statisticsdesktop

import (
	"encoding/hex"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type rangeSpec struct {
	key  string
	days int
	unit string
}

var usageRanges = []rangeSpec{
	{key: "24h", days: 1, unit: "half_hour"},
	{key: "7d", days: 7, unit: "day"},
	{key: "30d", days: 30, unit: "day"},
	{key: "90d", days: 90, unit: "week"},
	{key: "1y", days: 365, unit: "week"},
}

var breakdownModes = []string{"model", "provider", "project"}

func rangeByKey(key string) (rangeSpec, bool) {
	for _, r := range usageRanges {
		if r.key == key {
			return r, true
		}
	}
	return rangeSpec{}, false
}

func rangeUnit(key string) string {
	if r, ok := rangeByKey(key); ok {
		return r.unit
	}
	return "day"
}

func rangeHasCalendar(key string) bool {
	return key == "30d" || key == "90d" || key == "1y"
}

func nodeHex(id protocol.NodeID) string {
	return hex.EncodeToString(id[:])
}

func parseNodeHex(s string) (protocol.NodeID, bool) {
	var id protocol.NodeID
	if raw, err := hex.DecodeString(s); err == nil && len(raw) == len(id) {
		copy(id[:], raw)
		return id, true
	}
	return protocol.NodeID{}, false
}

func makeResourceID(node string, id string) string {
	return node + ":" + id
}

func splitResourceID(value, defaultNode string) (node string, id string) {
	if at := strings.IndexByte(value, ':'); at >= 0 {
		return value[:at], value[at+1:]
	}
	return defaultNode, value
}

func queryDimensionForBreakdown(breakdown string) protocol.UsageDimension {
	if breakdown == "project" {
		return protocol.UsageDimensionProject
	}
	return protocol.UsageDimensionModel
}

func localUTCOffsetMinutes(now time.Time) int32 {
	_, sec := now.Zone()
	return int32(sec / 60)
}

func buildUsageQuery(rangeKey string, now time.Time, grouping protocol.UsageDimension) protocol.UsageQuery {
	spec, ok := rangeByKey(rangeKey)
	if !ok {
		spec = usageRanges[2] // 30d default
	}
	offsetMin := localUTCOffsetMinutes(now)
	shift := int64(offsetMin) * minuteMs
	width := dayMs
	if spec.key == "24h" {
		width = halfHourMs
	}
	nowMs := now.UnixMilli()
	// End at the next local boundary so the current bucket is complete.
	endMs := (floorDiv(nowMs+shift, width)+1)*width - shift
	startMs := endMs - int64(spec.days)*dayMs
	q := protocol.DefaultUsageQuery()
	q.StartMs = startMs
	q.EndMs = endMs
	q.UtcOffsetMinutes = offsetMin
	q.Dimension = grouping
	return q
}

type usageData struct {
	node     string
	query    protocol.UsageQuery
	totals   protocol.UsageMetrics
	days     []protocol.UsagePoint
	recent   []protocol.UsagePoint
	groups   []scopedUsageGroup
	requests protocol.UsagePage
}

type usageViewState struct {
	report         *usageData
	connected      bool
	refreshing     bool
	partial        bool
	err            *protocol.Fault
	resourceCursor string
}

type usageInventorySource struct {
	node      string
	label     string
	projects  []protocol.Project
	providers []protocol.ConversationProvider
}

type scopedResourceKey struct {
	node string
	key  protocol.UsageKey
}

type filterChoice struct {
	id    string
	label string
}

type filterChoices struct {
	projects  []filterChoice
	providers []filterChoice
	models    []filterChoice
}

type heatmapCache struct {
	span   int
	cursor string
	start  int64
	end    int64
	offset int32
	days   []protocol.UsagePoint
}

func isRevokedFault(err *protocol.Fault) bool {
	if err == nil {
		return false
	}
	switch err.Code {
	case protocol.ErrorCodeNotConfigured,
		protocol.ErrorCodeNotFound,
		protocol.ErrorCodePermissionDenied,
		protocol.ErrorCodeRevisionConflict:
		return true
	default:
		return false
	}
}

// usagePageState holds presentation state for the Usage workspace page.
type usagePageState struct {
	home       string
	node       string
	project    string
	provider   string
	rangeKey   string
	breakdown  string
	expanded   bool
	query      protocol.UsageQuery
	generation uint64
	page       int
	cursors    []*protocol.UsagePosition
	view       *usageViewState
	previous   *usageData
	now        func() time.Time
}

func newUsagePageState(home protocol.NodeID, now func() time.Time) *usagePageState {
	if now == nil {
		now = time.Now
	}
	h := nodeHex(home)
	s := &usagePageState{
		home:      h,
		node:      h,
		rangeKey:  "30d",
		breakdown: "model",
		page:      1,
		cursors:   []*protocol.UsagePosition{nil},
		now:       now,
	}
	s.query = buildUsageQuery(s.rangeKey, now(), queryDimensionForBreakdown(s.breakdown))
	return s
}

func (s *usagePageState) allNodes() bool {
	return s.project == "" && s.provider == ""
}

func (s *usagePageState) data() *usageData {
	if s.previous != nil {
		return s.previous
	}
	if s.view != nil {
		return s.view.report
	}
	return nil
}

func (s *usagePageState) selectedValue(field string) string {
	switch field {
	case "project":
		return s.project
	case "provider":
		return s.provider
	case "model":
		if len(s.query.Models) > 0 {
			return s.query.Models[0]
		}
	}
	return ""
}

func (s *usagePageState) resourceCursor() string {
	if s.view == nil {
		return ""
	}
	return s.view.resourceCursor
}

func (s *usagePageState) ready() bool {
	return s.previous == nil && s.data() != nil && s.view != nil &&
		s.view.connected && !s.view.refreshing && !s.view.partial && s.view.err == nil
}

func (s *usagePageState) resetPagination() {
	s.page = 1
	s.cursors = []*protocol.UsagePosition{nil}
	s.query.Before = nil
	s.previous = nil
}

func (s *usagePageState) advance() bool {
	cur := buildUsageQuery(s.rangeKey, s.now(), s.query.Dimension)
	if cur.StartMs == s.query.StartMs && cur.EndMs == s.query.EndMs && cur.UtcOffsetMinutes == s.query.UtcOffsetMinutes {
		return false
	}
	s.query.StartMs = cur.StartMs
	s.query.EndMs = cur.EndMs
	s.query.UtcOffsetMinutes = cur.UtcOffsetMinutes
	return true
}

func (s *usagePageState) selectFilter(field, value string) bool {
	prev := s.data()
	if field == "range" {
		prev = nil
		if _, ok := rangeByKey(value); !ok || value == s.rangeKey {
			return false
		}
		dim := s.query.Dimension
		projects := slices.Clone(s.query.Projects)
		providers := slices.Clone(s.query.Providers)
		models := slices.Clone(s.query.Models)
		s.rangeKey = value
		s.query = buildUsageQuery(value, s.now(), dim)
		s.query.Projects = projects
		s.query.Providers = providers
		s.query.Models = models
	} else if field == "project" {
		if value == s.project {
			return false
		}
		s.provider = ""
		s.query.Worktrees = []protocol.WorktreeID{}
		s.query.Providers = []protocol.ProviderID{}
		s.query.Models = []string{}
		if value == "" {
			s.project = ""
			s.node = s.home
			s.query.Projects = []protocol.ProjectID{}
		} else {
			node, rawID := splitResourceID(value, s.home)
			pid, err := uuid.Parse(rawID)
			if err != nil {
				return false
			}
			s.project = makeResourceID(node, rawID)
			s.node = node
			s.query.Projects = []protocol.ProjectID{protocol.ProjectID(pid)}
		}
	} else if field == "provider" {
		if value == s.provider {
			return false
		}
		s.query.Models = []string{}
		if value == "" {
			s.provider = ""
			if s.project == "" {
				s.node = s.home
			}
			s.query.Providers = []protocol.ProviderID{}
		} else {
			node, rawID := splitResourceID(value, s.node)
			if s.project != "" && node != s.node {
				return false
			}
			pid, err := uuid.Parse(rawID)
			if err != nil {
				return false
			}
			s.provider = makeResourceID(node, rawID)
			if s.project == "" {
				s.node = node
			}
			s.query.Providers = []protocol.ProviderID{protocol.ProviderID(pid)}
		}
	} else if field == "model" {
		cur := s.selectedValue("model")
		if value == cur {
			return false
		}
		if value == "" {
			s.query.Models = []string{}
		} else {
			s.query.Models = []string{value}
		}
	} else {
		return false
	}
	s.resetPagination()
	s.previous = prev
	s.view = nil
	return true
}

func (s *usagePageState) selectBreakdown(mode string) bool {
	if !slices.Contains(breakdownModes, mode) || mode == s.breakdown {
		return false
	}
	s.breakdown = mode
	s.expanded = false
	nextDim := queryDimensionForBreakdown(mode)
	if nextDim == s.query.Dimension {
		return false
	}
	// Request pages do not depend on breakdown grouping, so keep the page.
	s.previous = s.data()
	s.query.Dimension = nextDim
	s.view = nil
	return true
}

func (s *usagePageState) turnPage(page int) bool {
	if !s.ready() || page < 1 || page == s.page || page > len(s.cursors)+1 {
		return false
	}
	data := s.data()
	if page > len(s.cursors) {
		if page != s.page+1 || !data.requests.HasMore || len(data.requests.Items) == 0 {
			return false
		}
		lastPos := data.requests.Items[len(data.requests.Items)-1].Position
		s.cursors = append(s.cursors, &lastPos)
	}
	s.previous = data
	s.page = page
	if c := s.cursors[page-1]; c != nil {
		cp := *c
		s.query.Before = &cp
	} else {
		s.query.Before = nil
	}
	s.view = nil
	return true
}

func (s *usagePageState) nextGeneration() uint64 {
	s.generation++
	return s.generation
}

func (s *usagePageState) accept(view usageViewState, generation uint64) bool {
	if generation != s.generation {
		return false
	}
	cp := view
	s.view = &cp
	if isRevokedFault(s.view.err) {
		s.previous = nil
	}
	if s.view.report != nil && !s.view.refreshing {
		s.previous = nil
	}
	return true
}

func buildFilterChoices(s *usagePageState, inventory []usageInventorySource, resources []scopedResourceKey, t translateFunc) filterChoices {
	multi := len(inventory) > 1
	projectMap := map[string]string{"": t("usage_all_projects")}
	projectOrder := []string{""}
	addProject := func(id, label string) {
		if _, ok := projectMap[id]; !ok {
			projectOrder = append(projectOrder, id)
		}
		projectMap[id] = label
	}
	providerMap := map[string]string{"": t("usage_all_providers")}
	providerOrder := []string{""}
	addProvider := func(id, label string) {
		if _, ok := providerMap[id]; !ok {
			providerOrder = append(providerOrder, id)
		}
		providerMap[id] = label
	}
	modelSet := map[string]struct{}{}

	for _, src := range inventory {
		labelFor := func(name string) string {
			if !multi {
				return name
			}
			host := src.label
			if host == "" {
				host = shortPrefix(src.node)
			}
			return name + " · " + host
		}
		for _, proj := range src.projects {
			addProject(makeResourceID(src.node, proj.ID.String()), labelFor(proj.Name))
		}
		if s.project != "" && src.node != s.node {
			continue
		}
		for _, prov := range src.providers {
			rid := makeResourceID(src.node, prov.ID.String())
			addProvider(rid, labelFor(prov.Name))
			if s.provider == "" || s.provider == rid {
				for _, m := range prov.Models {
					modelSet[m.ID] = struct{}{}
				}
			}
		}
	}

	for _, rk := range resources {
		node := rk.node
		if node == "" {
			node = s.node
		}
		if s.project != "" && node != s.node {
			continue
		}
		switch rk.key.Kind {
		case "provider":
			if pid, ok := extractProviderID(rk.key.Data); ok {
				raw := pid.String()
				rid := makeResourceID(node, raw)
				if _, exists := providerMap[rid]; !exists {
					addProvider(rid, raw)
				}
			}
		case "model":
			if mp, ok := extractModelKey(rk.key.Data); ok {
				rid := makeResourceID(node, mp.Provider.String())
				if s.provider == "" || s.provider == rid {
					modelSet[mp.Model] = struct{}{}
				}
			}
		}
	}

	if s.project != "" {
		if _, ok := projectMap[s.project]; !ok {
			addProject(s.project, s.project)
		}
	}
	if s.provider != "" {
		if _, ok := providerMap[s.provider]; !ok {
			addProvider(s.provider, s.provider)
		}
	}

	projects := make([]filterChoice, 0, len(projectOrder))
	for _, id := range projectOrder {
		projects = append(projects, filterChoice{id: id, label: projectMap[id]})
	}
	providers := make([]filterChoice, 0, len(providerOrder))
	for _, id := range providerOrder {
		providers = append(providers, filterChoice{id: id, label: providerMap[id]})
	}

	sortedModels := make([]string, 0, len(modelSet))
	for m := range modelSet {
		sortedModels = append(sortedModels, m)
	}
	slices.Sort(sortedModels)
	models := make([]filterChoice, 0, len(sortedModels)+2)
	models = append(models, filterChoice{id: "", label: t("usage_all_models")})
	seenModel := map[string]bool{"": true}
	for _, m := range sortedModels {
		models = append(models, filterChoice{id: m, label: m})
		seenModel[m] = true
	}
	if sel := s.selectedValue("model"); sel != "" && !seenModel[sel] {
		models = append(models, filterChoice{id: sel, label: sel})
	}

	return filterChoices{
		projects:  projects,
		providers: providers,
		models:    models,
	}
}

func buildCatalogNames(inventory []usageInventorySource, primaryNode string) usageCatalogNames {
	names := usageCatalogNames{
		providers: map[string]string{},
		projects:  map[string]string{},
		hosts:     map[string]string{},
	}
	for _, src := range inventory {
		host := src.label
		if host == "" {
			host = shortPrefix(src.node)
		}
		names.hosts[src.node] = host
		for _, p := range src.providers {
			id := p.ID.String()
			names.providers[src.node+":"+id] = p.Name
			if src.node == primaryNode {
				names.providers[id] = p.Name
			}
		}
		for _, p := range src.projects {
			id := p.ID.String()
			names.projects[src.node+":"+id] = p.Name
			if src.node == primaryNode {
				names.projects[id] = p.Name
			}
		}
	}
	return names
}

func reportToUsageData(report protocol.UsageReport) *usageData {
	node := nodeHex(report.Node)
	groups := make([]scopedUsageGroup, len(report.Groups))
	for i, g := range report.Groups {
		groups[i] = scopedUsageGroup{Node: node, Group: g}
	}
	return &usageData{
		node:     node,
		query:    report.Query,
		totals:   report.Totals,
		days:     report.Days,
		recent:   report.Recent,
		groups:   groups,
		requests: report.Requests,
	}
}

func singleReportCursor(report protocol.UsageReport) string {
	return nodeHex(report.Node) + ":" + strconv.FormatUint(report.Cursor, 10)
}
