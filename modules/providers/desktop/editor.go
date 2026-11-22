package desktop

import (
	"fmt"
	"maps"
	"math"
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// providerStep is the editor's page.
type providerStep int

const (
	stepBrand providerStep = iota
	stepMethod
	stepConfigure
)

// modelDraft is a model in the editor: its ID, context limit and reasoning
// efforts as typed, and whether its form shows.
type modelDraft struct {
	pending bool
	// seeded marks a model the route came with, which another route
	// replaces while its ID is as it came.
	seeded  bool
	model   preview.ChannelModel
	id      string
	context string
	efforts []string
	effort  string // the effort being added
	def     int    // the default effort
	// effortInput is the efforts field's input, which a click in the field
	// focuses; rejected asks the next frame to say an effort was refused.
	effortInput ui.Handle
	rejected    bool
	open        bool
	// hosted keeps the image tool the model came with: ticking Image again
	// restores it rather than the images endpoint.
	hosted bool
	// catalog is the model as it came when its tags were the catalog's,
	// which a reset of the tags shows again; nil when they were the user's.
	catalog *preview.ChannelModel
	lookup  *modelCapabilityLookup
	// relookup asks the catalog for the saved ID's tags, which a row that
	// follows the catalog again after the user's tags does not have.
	relookup bool
}

// providerEditor is the step, the brand, the methods its choice offered and
// the method and route chosen, the fields and the models. editing is the
// channel edited, "" for a new one.
type providerEditor struct {
	resumeSave          func(settings.ProviderDraft, func(settings.ProviderSaveResult))
	generation          uint64
	id                  string
	owner               string
	group               string
	revision            uint64
	targets             []string
	targetIDs           map[string]string
	targetRevisions     map[string]uint64
	targetSucceeded     []string
	targetFailed        []string
	targetUncertain     []string
	targetUnconfirmed   []string
	open                bool
	step                providerStep
	editing             string
	brand               *preview.Brand
	methods             []preview.Method
	method              preview.Method
	route               int
	name, endpoint, key string
	keySet              bool
	originalKey         string
	keyShown            bool
	apiVersion          string    // Azure OpenAI's, asked for where the route is that API
	oauth               [4]string // catalog, user agent, editor, editor plugin versions
	headers             []header  // an API-key channel's request headers, a row each
	headersRemoved      int       // header rows taken out, part of a row's identity
	// billingRate is an API-key channel's billing rate as the percent typed;
	// billingRateBase is the saved rate, nil for a channel without one.
	billingRate     string
	billingRateBase *uint32
	advanced        bool
	models          []modelDraft
	removed         int // models taken out, part of a model row's identity (mcpEditor.removed)
	def             int
	probing         bool
	saving          bool // probing is the save's, not a discovery's
	// returnTo reopens the welcome guide from the first step's Back, when
	// the guide opened the editor; nil otherwise.
	returnTo func()
}

func draftsOf(ms []preview.ChannelModel) []modelDraft {
	out := make([]modelDraft, len(ms))
	for i, x := range ms {
		ctx := ""
		if x.Context > 0 {
			ctx = strconv.Itoa(x.Context)
		}
		x.Inputs, x.Outputs, x.Generates = slices.Clone(x.Inputs), slices.Clone(x.Outputs), slices.Clone(x.Generates)
		out[i] = modelDraft{model: x, id: x.ID, context: ctx, efforts: slices.Clone(x.Efforts), def: max(0, slices.Index(x.Efforts, x.DefaultEffort)),
			hosted: slices.Contains(x.Generates, viewmodel.GenerateImageTool)}
		if !x.CustomCapabilities {
			came := ms[i]
			out[i].catalog = &came
		}
	}
	return out
}

func (e *providerEditor) add(m *Controller) {
	owner := m.ManagedNodeID()
	if m.Live && m.ManagedNode == "" && m.ProviderActions.DefaultTarget != nil {
		if preferred := m.ProviderActions.DefaultTarget(); preferred != "" {
			owner = preferred
		}
	}
	e.addOn(m, owner)
}

func (e *providerEditor) addOn(m *Controller, owner string) {
	*e = providerEditor{open: true, step: stepBrand, generation: e.generation + 1, owner: owner}
	if e.owner != "" {
		e.targets = []string{e.owner}
	}
	if m.Live && m.ProviderActions.NewID != nil {
		e.id = m.ProviderActions.NewID()
	}
}

// CloseProviderEditor drops the draft and actions captured for the previous Node.
func (m *Controller) CloseProviderEditor(nextNode string) {
	m.removing, m.expanded, m.family = "", "", ""
	if e := &m.provider; nextNode != "" && e.open && e.returnTo != nil && e.owner == nextNode {
		// The welcome guide opens its editor on the incoming local Node.
		return
	}
	m.provider = providerEditor{generation: m.provider.generation + 1}
}

func (e *providerEditor) edit(m *Controller, ch *preview.Channel) {
	*e = providerEditor{open: true, step: stepConfigure, editing: ch.ID, owner: ch.Node, group: ch.Group, targetIDs: maps.Clone(ch.TargetIDs), targetRevisions: maps.Clone(ch.TargetRevisions), id: ch.ID, generation: e.generation + 1, name: ch.Name, endpoint: ch.Endpoint,
		keySet: ch.KeySet, models: draftsOf(ch.Models), def: ch.Default}
	if ch.Node != "" {
		e.targets = []string{ch.Node}
	}
	if e.targetRevisions == nil && ch.Node != "" {
		e.targetRevisions = map[string]uint64{ch.Node: ch.Revision}
	}
	e.revision = ch.Revision
	e.oauth = ch.OAuth
	e.apiVersion = ch.APIVersion
	e.headers = headerRows(ch.Headers)
	e.billingRate, e.billingRateBase = formatBillingRate(ch.BillingRateBps), cloneBillingRate(ch.BillingRateBps)
	if m.Live && !ch.ConfigurationReadOnly && (ch.Auth == preview.AuthKey || ch.Auth == viewmodel.AuthOptionalKey && ch.KeySet) && m.ProviderActions.ReadKey != nil {
		id := ch.ID
		generation := e.generation
		m.ProviderActions.ReadKey(ch.Node, id, func(value string) {
			if e.open && e.generation == generation && e.editing == id && e.owner == ch.Node {
				e.keySet, e.originalKey = value != "", value
				if e.key == "" {
					e.key = value
				}
			}
		})
	}
	for i := range m.Brands {
		b := &m.Brands[i]
		for _, x := range b.Methods {
			if ch.Node != "" && x.Node != ch.Node {
				continue
			}
			for j, route := range methodRoutes(x) {
				if route.ID == ch.Method {
					e.brand, e.method, e.route = b, x, j
				}
			}
		}
	}
	// A setting the channel saved without a value shows the one in use.
	for i, value := range e.method.OAuth {
		if e.oauth[i] == "" {
			e.oauth[i] = value
		}
	}
	if e.brand == nil {
		e.method = preview.Method{Node: ch.Node, ID: ch.Method, Name: ch.Name, API: ch.API, Auth: ch.Auth, Keyed: ch.Auth == preview.AuthKey || ch.Auth == viewmodel.AuthOptionalKey, Custom: ch.Endpoint != ""}
		if ch.Auth == preview.AuthOAuth {
			e.method.Login = "provider_access_login"
		}
	}
	if e.azure() && e.apiVersion == "" {
		e.apiVersion = azureDefaultVersion
	}
}

// azureDefaultVersion is the API version an Azure OpenAI form starts with,
// the reference's.
const azureDefaultVersion = "2024-10-21"

// azure reports an Azure OpenAI route: its form asks for the API version
// and names a model by its deployment.
func (e *providerEditor) azure() bool { return e.current().API == "azure_open_ai" }

// methodRoutes are a method's routes: its own, or the method as its only
// one.
func methodRoutes(x preview.Method) []preview.Route {
	if len(x.Routes) > 0 {
		return x.Routes
	}
	return []preview.Route{{ID: x.ID, API: x.API, Endpoint: x.Endpoint, Custom: x.Custom}}
}

// current is the route chosen.
func (e *providerEditor) current() preview.Route {
	routes := methodRoutes(e.method)
	return routes[min(max(e.route, 0), len(routes)-1)]
}

// routeAxis is what a form control switches a route by.
type routeAxis int

const (
	axisProtocol routeAxis = iota
	axisProduct
	axisRegion
)

// choices are the routes an axis's control offers, by index: those that
// differ from the current one in the axis alone (a product keeps the
// protocol, as its region follows in switchRoute), one for each value.
func (e *providerEditor) choices(axis routeAxis) []int {
	routes, cur := methodRoutes(e.method), e.current()
	var out []int
	for i, r := range routes {
		ok, same := false, func(o preview.Route) bool { return false }
		switch axis {
		case axisProtocol:
			ok, same = r.Region == cur.Region && r.Product == cur.Product, func(o preview.Route) bool { return o.API == r.API }
		case axisProduct:
			ok, same = r.API == cur.API, func(o preview.Route) bool { return o.Product == r.Product }
		case axisRegion:
			ok, same = r.API == cur.API && r.Product == cur.Product, func(o preview.Route) bool { return o.Region == r.Region }
		}
		if ok && !slices.ContainsFunc(out, func(j int) bool { return same(routes[j]) }) {
			out = append(out, i)
		}
	}
	return out
}

// switchRoute takes another route of the method. Another product keeps the
// region where it has one. The name, the key and what the user typed stay:
// the endpoint follows only while it is the route's own, and the models the
// route came with are replaced only while none was added or renamed.
func (e *providerEditor) switchRoute(i int) {
	routes, cur := methodRoutes(e.method), e.current()
	if i < 0 || i >= len(routes) {
		return
	}
	if want := routes[i]; e.editing != "" && (want.API == cur.API || want.Product != cur.Product || want.Region != cur.Region) {
		return
	}
	if want := routes[i]; want.Product != cur.Product {
		if j := slices.IndexFunc(routes, func(r preview.Route) bool {
			return r.API == want.API && r.Product == want.Product && r.Region == cur.Region
		}); j >= 0 {
			i = j
		}
	}
	if e.endpoint == cur.Endpoint {
		e.endpoint = routes[i].Endpoint
	}
	e.route = i
	e.refreshImageGeneration()
	if !slices.ContainsFunc(e.models, func(d modelDraft) bool { return !d.seeded || d.id != d.model.ID }) {
		e.models, e.def = nil, 0
		e.seed()
	}
}

// seed starts a new channel without models on the ones its route comes
// with, at the limits and tags a save gives a model typed in.
func (e *providerEditor) seed() {
	if len(e.models) > 0 || e.editing != "" {
		return
	}
	for _, id := range e.current().Models {
		d := draftsOf([]preview.ChannelModel{{ID: id, Name: id, Context: 128000, Vision: true, Tools: true}})[0]
		d.seeded = true
		e.models = append(e.models, d)
	}
}

// choose takes a choice of the brand step, a brand and the methods it
// offers: one method configures it at once, several ask which.
func (e *providerEditor) choose(b *preview.Brand, methods []preview.Method) {
	e.brand, e.methods = b, methods
	if len(methods) == 1 {
		e.configure(methods[0])
		return
	}
	e.step = stepMethod
}

// configure opens the form on a method's first route.
func (e *providerEditor) configure(x preview.Method) {
	e.method, e.route, e.step = x, 0, stepConfigure
	if e.owner == "" {
		e.owner = x.Node
	}
	if len(e.targets) == 0 && e.owner != "" {
		e.targets = []string{e.owner}
	}
	e.targets = e.normalizedTargets()
	e.name, e.endpoint = x.Name, e.current().Endpoint
	e.models, e.def = nil, 0
	e.oauth = x.OAuth
	e.apiVersion = ""
	// A rate typed for another method stays, as the headers do.
	if e.billingRate == "" {
		e.billingRate = formatBillingRate(nil)
	}
	if e.azure() {
		e.apiVersion = azureDefaultVersion
	}
	e.seed()
}

// back is Configure goes to Method when the choice offered several, else
// to Brand.
func (e *providerEditor) back() {
	if e.step == stepConfigure && len(e.methods) > 1 {
		e.step = stepMethod
		return
	}
	if back := e.returnTo; back != nil {
		// Opened from the welcome guide, whose picker was the first step: back
		// closes the editor and returns.
		e.open, e.returnTo = false, nil
		back()
		return
	}
	e.step = stepBrand
}

func (e *providerEditor) usesKey() bool {
	return e.method.Keyed || e.method.Access == "provider_access_key" || e.method.Access == "provider_access_cloud"
}

func (e *providerEditor) auth() preview.Auth {
	if e.method.Auth == preview.AuthOAuth || e.method.Auth == preview.AuthPlugin || e.method.Auth == viewmodel.AuthOptionalKey {
		return e.method.Auth
	}
	if e.login() {
		return preview.AuthOAuth
	}
	if e.method.Auth == preview.AuthKey || e.usesKey() {
		return preview.AuthKey
	}
	return preview.AuthNone
}

func providerAuthAllowsMultipleTargets(auth preview.Auth) bool {
	return auth == preview.AuthKey || auth == viewmodel.AuthOptionalKey || auth == preview.AuthNone
}

func providerTargetList(targets []string) []string {
	result := make([]string, 0, len(targets))
	for _, target := range targets {
		if target != "" && !slices.Contains(result, target) {
			result = append(result, target)
		}
	}
	slices.Sort(result)
	return result
}

func completeProviderSaveResult(targets []string, result settings.ProviderSaveResult) settings.ProviderSaveResult {
	wanted := providerTargetList(targets)
	result.Uncertain = providerTargetList(result.Uncertain)
	result.Unconfirmed = providerTargetList(result.Unconfirmed)
	uncertain := func(node string) bool {
		return slices.Contains(result.Uncertain, node) || slices.Contains(result.Unconfirmed, node)
	}
	failed := map[string]bool{}
	var extraFailed []string
	for _, node := range result.Failed {
		if uncertain(node) {
			continue
		}
		failed[node] = true
		if !slices.Contains(wanted, node) {
			extraFailed = append(extraFailed, node)
		}
	}
	succeeded := map[string]bool{}
	for _, node := range result.Succeeded {
		if !failed[node] {
			succeeded[node] = true
		}
	}
	result.Succeeded, result.Failed = nil, nil
	for _, node := range wanted {
		if uncertain(node) {
			continue
		}
		if failed[node] {
			result.Failed = append(result.Failed, node)
		} else if succeeded[node] {
			result.Succeeded = append(result.Succeeded, node)
		} else {
			result.Failed = append(result.Failed, node)
		}
	}
	result.Failed = append(result.Failed, providerTargetList(extraFailed)...)
	return result
}

func (e *providerEditor) toggleCopyTarget(node string) {
	if node == "" || node == e.owner || !providerAuthAllowsMultipleTargets(e.auth()) {
		return
	}
	if index := slices.Index(e.targets, node); index >= 0 {
		e.targets = slices.Delete(e.targets, index, index+1)
	} else {
		e.targets = append(e.targets, node)
	}
	e.targetSucceeded, e.targetFailed, e.targetUncertain = nil, nil, nil
	e.targetUnconfirmed = nil
}

func (e *providerEditor) targetHosts(m *Controller) []settings.ProviderHost {
	if m.ProviderActions.Hosts == nil {
		return nil
	}
	hosts := m.ProviderActions.Hosts()
	for _, node := range e.targets {
		if !slices.ContainsFunc(hosts, func(host settings.ProviderHost) bool { return host.Node == node }) {
			hosts = append(hosts, settings.ProviderHost{Node: node, Name: node})
		}
	}
	return hosts
}

func (e *providerEditor) normalizedTargets() []string {
	if e.owner == "" {
		return nil
	}
	if !providerAuthAllowsMultipleTargets(e.auth()) {
		return []string{e.owner}
	}
	targets := providerTargetList(e.targets)
	if !slices.Contains(targets, e.owner) {
		targets = append(targets, e.owner)
	}
	return targets
}

// keyUpdate keeps displaying a stored key from creating a new credential
// on a save that changes only the channel's configuration.
func (e *providerEditor) keyUpdate() string {
	if e.key == e.originalKey {
		return ""
	}
	return e.key
}

func (e *providerEditor) login() bool { return e.method.Login != "" }

// billsByRate reports a channel that saves its own billing rate: one the
// Node keeps as API-key authentication, by a key, an optional key or a
// package preset without one. Sign-ins and plugin connections have none.
func (e *providerEditor) billsByRate() bool {
	switch e.method.Auth {
	case preview.AuthKey, viewmodel.AuthOptionalKey, preview.AuthNone, viewmodel.AuthUnresolved:
		return !e.login()
	}
	return false
}

// billingRateValue is the rate a save sends: nil where the channel has none
// or still bills at the 100% it never saved, else the typed rate.
func (e *providerEditor) billingRateValue() *uint32 {
	if !e.billsByRate() {
		return nil
	}
	bps, _ := parseBillingRate(e.billingRate)
	if e.billingRateBase == nil && bps == protocol.DefaultBillingRateBps {
		return nil
	}
	return &bps
}

func cloneBillingRate(bps *uint32) *uint32 {
	if bps == nil {
		return nil
	}
	value := *bps
	return &value
}

// formatBillingRate is a rate in basis points as the percent the form shows,
// with the decimals it has: 10000 is "100", 1250 "12.5", nil "100".
func formatBillingRate(bps *uint32) string {
	value := protocol.DefaultBillingRateBps
	if bps != nil {
		value = *bps
	}
	text := strconv.FormatUint(uint64(value/100), 10)
	if fraction := value % 100; fraction != 0 {
		text += "." + strings.TrimRight(fmt.Sprintf("%02d", fraction), "0")
	}
	return text
}

// parseBillingRate reads a typed percent, a trailing % allowed, into basis
// points. ok is false for a sign, more than two decimals or more than a
// uint32 holds.
func parseBillingRate(text string) (bps uint32, ok bool) {
	text = strings.TrimSpace(strings.TrimSuffix(strings.TrimSpace(text), "%"))
	whole, fraction, _ := strings.Cut(text, ".")
	digits := func(s string) bool {
		return !strings.ContainsFunc(s, func(r rune) bool { return r < '0' || r > '9' })
	}
	if whole+fraction == "" || len(fraction) > 2 || !digits(whole) || !digits(fraction) {
		return 0, false
	}
	var value uint64
	if whole != "" {
		n, err := strconv.ParseUint(whole, 10, 64)
		if err != nil || n > math.MaxUint32 {
			return 0, false
		}
		value = n * 100
	}
	if fraction != "" {
		n, _ := strconv.ParseUint(fraction+strings.Repeat("0", 2-len(fraction)), 10, 64)
		value += n
	}
	if value > math.MaxUint32 {
		return 0, false
	}
	return uint32(value), true
}

// Check validates the editor's configuration: the i18n key of the problem, or
// "".
func (e *providerEditor) Check() string {
	if strings.TrimSpace(e.name) == "" {
		return "provider_name_required"
	}
	if e.current().Custom && !strings.HasPrefix(e.endpoint, "http://") && !strings.HasPrefix(e.endpoint, "https://") {
		return "provider_endpoint_required"
	}
	if e.usesKey() && e.method.Auth != viewmodel.AuthOptionalKey && !e.keySet && strings.TrimSpace(e.key) == "" {
		return "provider_key_required"
	}
	if e.azure() && strings.TrimSpace(e.apiVersion) == "" {
		return "provider_cloud_required"
	}
	if !e.login() {
		if _, ok := parseHeaders(e.headers); !ok {
			return "provider_headers_invalid"
		}
	}
	if e.billsByRate() {
		if _, ok := parseBillingRate(e.billingRate); !ok {
			return "provider_billing_rate_invalid"
		}
	}
	if len(e.models) == 0 && !e.login() {
		return "provider_model_required"
	}
	seen := map[string]bool{}
	for _, d := range e.models {
		id := strings.TrimSpace(d.id)
		if id == "" {
			return "provider_model_required"
		}
		if seen[id] {
			return "provider_model_duplicate"
		}
		seen[id] = true
		if n, err := strconv.ParseUint(d.context, 10, 32); err != nil || n == 0 || n < uint64(d.model.Output) {
			return "provider_limits_invalid"
		}
		// The efforts are edited in place: each named, none twice.
		named := map[string]bool{}
		for _, x := range d.efforts {
			x = strings.TrimSpace(x)
			if x == "" || named[x] {
				return "provider_effort_invalid"
			}
			named[x] = true
		}
	}
	return ""
}

// header is a request header's row as the editor shows it.
type header struct{ Name, Value string }

// parseHeaders reads the header rows; rows left empty are skipped. ok is
// false when a row has a value but no name. The Node checks the names and
// values it accepts.
func parseHeaders(rows []header) (headers map[string]string, ok bool) {
	for _, row := range rows {
		name, value := strings.TrimSpace(row.Name), strings.TrimSpace(row.Value)
		if name == "" {
			if value != "" {
				return nil, false
			}
			continue
		}
		if headers == nil {
			headers = map[string]string{}
		}
		headers[name] = value
	}
	return headers, true
}

// headerRows are headers as rows in name order.
func headerRows(headers map[string]string) []header {
	rows := make([]header, 0, len(headers))
	for _, name := range slices.Sorted(maps.Keys(headers)) {
		rows = append(rows, header{name, headers[name]})
	}
	return rows
}

// save writes the channel; the editor stays open with its draft when
// Check finds a problem.
func (e *providerEditor) save(m *Controller) string {
	if problem := e.Check(); problem != "" {
		return problem
	}
	targets := e.normalizedTargets()
	if m.Live && m.ProviderActions.Hosts != nil &&
		(len(targets) == 0 || !providerAuthAllowsMultipleTargets(e.auth()) && len(targets) != 1) {
		return "provider_host_required"
	}
	models := make([]preview.ChannelModel, len(e.models))
	for i, d := range e.models {
		x := d.model
		x.ID = strings.TrimSpace(d.id)
		if x.Name == "" || x.Name == d.model.ID {
			x.Name = x.ID
		}
		x.Context, _ = strconv.Atoi(d.context)
		x.Efforts = nil
		for _, effort := range d.efforts {
			x.Efforts = append(x.Efforts, strings.TrimSpace(effort))
		}
		if len(x.Efforts) > 0 {
			x.DefaultEffort = x.Efforts[min(max(d.def, 0), len(x.Efforts)-1)]
		} else {
			x.DefaultEffort = "default"
		}
		models[i] = x
	}
	auth := e.auth()
	brand := ""
	if e.brand != nil {
		brand = e.brand.ID
	}
	route := e.current()
	rate := e.billingRateValue()
	if m.Live && m.ProviderActions.Save != nil {
		if e.probing {
			return ""
		}
		e.probing, e.saving = true, true
		channel := preview.Channel{Node: e.owner, Group: e.group, Targets: slices.Clone(targets), TargetIDs: maps.Clone(e.targetIDs), TargetRevisions: maps.Clone(e.targetRevisions), ID: e.id, Revision: e.revision, Name: strings.TrimSpace(e.name), Brand: brand, Icon: e.method.Icon, Method: route.ID, API: route.API, Endpoint: e.endpoint, Auth: auth, Enabled: true, KeySet: e.keySet, Models: models, Default: e.def, BillingRateBps: rate}
		if e.azure() {
			channel.APIVersion = strings.TrimSpace(e.apiVersion)
		}
		if !e.login() {
			channel.Headers, _ = parseHeaders(e.headers)
		}
		generation := e.generation
		signIn := e.login() && e.editing == ""
		signInNode, id := e.owner, e.id
		e.targetSucceeded, e.targetFailed, e.targetUncertain = nil, nil, nil
		e.targetUnconfirmed = nil
		save := m.ProviderActions.Save
		if e.resumeSave != nil {
			save = e.resumeSave
		}
		save(settings.ProviderDraft{Channel: channel, Key: e.keyUpdate(), ExpectedRevision: e.revision, EditorGeneration: generation, OAuth: e.oauth, Group: e.group, Targets: slices.Clone(targets), TargetRevisions: maps.Clone(e.targetRevisions)}, func(result settings.ProviderSaveResult) {
			if e.generation != generation || !e.open {
				return
			}
			e.probing, e.saving = false, false
			e.resumeSave = result.Resume
			result = completeProviderSaveResult(targets, result)
			e.targetSucceeded, e.targetFailed = slices.Clone(result.Succeeded), slices.Clone(result.Failed)
			e.targetUncertain = slices.Clone(result.Uncertain)
			e.targetUnconfirmed = slices.Clone(result.Unconfirmed)
			if result.Group != "" || result.TargetIDs != nil {
				e.group = result.Group
			}
			if e.targetIDs == nil {
				e.targetIDs = map[string]string{}
			}
			for node, id := range result.TargetIDs {
				e.targetIDs[node] = id
			}
			if e.targetRevisions == nil {
				e.targetRevisions = map[string]uint64{}
			}
			for node, revision := range result.TargetRevisions {
				e.targetRevisions[node] = revision
			}
			if revision, ok := result.TargetRevisions[e.owner]; ok {
				e.revision = revision
			}
			if len(result.Failed)+len(result.Uncertain)+len(result.Unconfirmed) > 0 {
				if signIn && slices.Contains(result.Succeeded, signInNode) {
					if targetID := result.TargetIDs[signInNode]; targetID != "" {
						id = targetID
					}
					m.signIn(signInNode, id)
				}
				return
			}
			e.open = false
			e.key = ""
			if signIn {
				if targetID := result.TargetIDs[signInNode]; targetID != "" {
					id = targetID
				}
				m.signIn(signInNode, id)
			}
		})
		return ""
	}
	if ch := m.channel(channelKey(e.owner, e.editing)); ch != nil {
		ch.Name, ch.Endpoint, ch.Models, ch.Default = strings.TrimSpace(e.name), e.endpoint, models, e.def
		ch.Method, ch.API = route.ID, route.API
		if e.azure() {
			ch.APIVersion = strings.TrimSpace(e.apiVersion)
		}
		if !e.login() {
			ch.Headers, _ = parseHeaders(e.headers)
		}
		ch.BillingRateBps = rate
		ch.KeySet = ch.KeySet || e.key != ""
		return ""
	}
	id := fmt.Sprintf("%s-%d", e.method.ID, len(m.Channels)+1)
	m.Channels = append(m.Channels, preview.Channel{Node: e.owner, Group: e.group, Targets: slices.Clone(targets), TargetIDs: maps.Clone(e.targetIDs), ID: id, Name: strings.TrimSpace(e.name), Brand: brand, Icon: e.method.Icon, Method: route.ID,
		API: route.API, Endpoint: e.endpoint, Auth: auth, Enabled: true, KeySet: e.key != "", Models: models, Default: e.def, BillingRateBps: rate})
	if e.login() {
		node := e.owner
		if len(targets) > 0 {
			node = targets[0]
		}
		m.signIn(node, id)
	}
	return ""
}
