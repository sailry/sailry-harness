// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type UsageQuery struct {
	StartMs          int64          `json:"start_ms"`
	EndMs            int64          `json:"end_ms"`
	UtcOffsetMinutes int32          `json:"utc_offset_minutes"`
	Dimension        UsageDimension `json:"dimension"`
	Projects         []ProjectID    `json:"projects"`
	Worktrees        []WorktreeID   `json:"worktrees"`
	Providers        []ProviderID   `json:"providers"`
	Models           []string       `json:"models"`
	Before           *UsagePosition `json:"before"`
}

func DefaultUsageQuery() UsageQuery {
	return UsageQuery{
		StartMs:          0,
		EndMs:            0,
		UtcOffsetMinutes: 0,
		Dimension:        UsageDimension(""),
		Projects:         []ProjectID{},
		Worktrees:        []WorktreeID{},
		Providers:        []ProviderID{},
		Models:           []string{},
		Before:           nil,
	}
}
func (value *UsageQuery) UnmarshalJSON(data []byte) error {
	type plain UsageQuery
	decoded := plain{}
	decoded.UtcOffsetMinutes = 0
	if err := DecodeObject(data, &decoded, []string{"start_ms", "end_ms", "dimension", "projects", "worktrees", "providers", "models"}, false); err != nil {
		return err
	}
	*value = UsageQuery(decoded)
	return nil
}
func (value UsageQuery) MarshalJSON() ([]byte, error) {
	type plain UsageQuery
	encoded := plain(value)
	encoded.Projects = normalizeCollections(encoded.Projects)
	encoded.Worktrees = normalizeCollections(encoded.Worktrees)
	encoded.Providers = normalizeCollections(encoded.Providers)
	encoded.Models = normalizeCollections(encoded.Models)
	return json.Marshal(encoded)
}

type UsagePosition struct {
	TimestampMs int64  `json:"timestamp_ms"`
	Node        NodeID `json:"node"`
	Sequence    uint64 `json:"sequence"`
}

func DefaultUsagePosition() UsagePosition {
	return UsagePosition{
		TimestampMs: 0,
		Node:        NodeID{},
		Sequence:    0,
	}
}
func (value *UsagePosition) UnmarshalJSON(data []byte) error {
	type plain UsagePosition
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"timestamp_ms", "node", "sequence"}, false); err != nil {
		return err
	}
	*value = UsagePosition(decoded)
	return nil
}
func (value UsagePosition) MarshalJSON() ([]byte, error) {
	type plain UsagePosition
	encoded := plain(value)
	return json.Marshal(encoded)
}

type UsageRequest struct {
	Position     UsagePosition     `json:"position"`
	Session      SessionID         `json:"session"`
	SessionTitle *string           `json:"session_title"`
	Turn         TurnID            `json:"turn"`
	Project      *ProjectID        `json:"project"`
	ScopeName    string            `json:"scope_name"`
	Worktree     WorktreeID        `json:"worktree"`
	Provider     ProviderID        `json:"provider"`
	ProviderName string            `json:"provider_name"`
	Model        string            `json:"model"`
	Tokens       ConversationUsage `json:"tokens"`
	UsdMicros    *uint64           `json:"usd_micros"`
	ElapsedUs    *uint64           `json:"elapsed_us"`
	FirstTokenUs *uint64           `json:"first_token_us"`
}

func DefaultUsageRequest() UsageRequest {
	return UsageRequest{
		Position:     DefaultUsagePosition(),
		Session:      SessionID{},
		SessionTitle: nil,
		Turn:         TurnID{},
		Project:      nil,
		ScopeName:    "",
		Worktree:     WorktreeID{},
		Provider:     ProviderID{},
		ProviderName: "",
		Model:        "",
		Tokens:       DefaultConversationUsage(),
		UsdMicros:    nil,
		ElapsedUs:    nil,
		FirstTokenUs: nil,
	}
}
func (value *UsageRequest) UnmarshalJSON(data []byte) error {
	type plain UsageRequest
	decoded := plain{}
	decoded.SessionTitle = nil
	if err := DecodeObject(data, &decoded, []string{"position", "session", "turn", "scope_name", "worktree", "provider", "provider_name", "model", "tokens"}, false); err != nil {
		return err
	}
	*value = UsageRequest(decoded)
	return nil
}
func (value UsageRequest) MarshalJSON() ([]byte, error) {
	type plain UsageRequest
	encoded := plain(value)
	return json.Marshal(encoded)
}

type UsagePage struct {
	Items   []UsageRequest `json:"items"`
	HasMore bool           `json:"has_more"`
}

func DefaultUsagePage() UsagePage {
	return UsagePage{
		Items:   []UsageRequest{},
		HasMore: false,
	}
}
func (value *UsagePage) UnmarshalJSON(data []byte) error {
	type plain UsagePage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"items", "has_more"}, false); err != nil {
		return err
	}
	*value = UsagePage(decoded)
	return nil
}
func (value UsagePage) MarshalJSON() ([]byte, error) {
	type plain UsagePage
	encoded := plain(value)
	encoded.Items = normalizeCollections(encoded.Items)
	return json.Marshal(encoded)
}

type UsageDimension string

const (
	UsageDimensionProvider UsageDimension = "provider"
	UsageDimensionModel    UsageDimension = "model"
	UsageDimensionProject  UsageDimension = "project"
	UsageDimensionWorktree UsageDimension = "worktree"
)

func (value UsageDimension) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *UsageDimension) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch UsageDimension(decoded) {
	case UsageDimensionProvider, UsageDimensionModel, UsageDimensionProject, UsageDimensionWorktree:
		*value = UsageDimension(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value UsageDimension) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *UsageDimension) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type UsageMetrics struct {
	Responses  uint64             `json:"responses"`
	Tokens     *ConversationUsage `json:"tokens"`
	Cost       *UsageCost         `json:"cost"`
	Generation *UsageGeneration   `json:"generation"`
}

func DefaultUsageMetrics() UsageMetrics {
	return UsageMetrics{
		Responses:  0,
		Tokens:     nil,
		Cost:       nil,
		Generation: nil,
	}
}
func (value *UsageMetrics) UnmarshalJSON(data []byte) error {
	type plain UsageMetrics
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"responses"}, false); err != nil {
		return err
	}
	*value = UsageMetrics(decoded)
	return nil
}
func (value UsageMetrics) MarshalJSON() ([]byte, error) {
	type plain UsageMetrics
	encoded := plain(value)
	return json.Marshal(encoded)
}

type UsageCost struct {
	UsdMicros uint64              `json:"usd_micros"`
	Responses uint64              `json:"responses"`
	Breakdown *UsageCostBreakdown `json:"breakdown"`
}

func DefaultUsageCost() UsageCost {
	return UsageCost{
		UsdMicros: 0,
		Responses: 0,
		Breakdown: nil,
	}
}
func (value *UsageCost) UnmarshalJSON(data []byte) error {
	type plain UsageCost
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"usd_micros", "responses"}, false); err != nil {
		return err
	}
	*value = UsageCost(decoded)
	return nil
}
func (value UsageCost) MarshalJSON() ([]byte, error) {
	type plain UsageCost
	encoded := plain(value)
	return json.Marshal(encoded)
}

type UsageCostBreakdown struct {
	Input      uint64 `json:"input"`
	Output     uint64 `json:"output"`
	CacheRead  uint64 `json:"cache_read"`
	CacheWrite uint64 `json:"cache_write"`
}

func DefaultUsageCostBreakdown() UsageCostBreakdown {
	return UsageCostBreakdown{
		Input:      0,
		Output:     0,
		CacheRead:  0,
		CacheWrite: 0,
	}
}
func (value *UsageCostBreakdown) UnmarshalJSON(data []byte) error {
	type plain UsageCostBreakdown
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"input", "output", "cache_read", "cache_write"}, false); err != nil {
		return err
	}
	*value = UsageCostBreakdown(decoded)
	return nil
}
func (value UsageCostBreakdown) MarshalJSON() ([]byte, error) {
	type plain UsageCostBreakdown
	encoded := plain(value)
	return json.Marshal(encoded)
}

type UsageGeneration struct {
	ElapsedUs    uint64 `json:"elapsed_us"`
	OutputTokens uint64 `json:"output_tokens"`
	Responses    uint64 `json:"responses"`
}

func DefaultUsageGeneration() UsageGeneration {
	return UsageGeneration{
		ElapsedUs:    0,
		OutputTokens: 0,
		Responses:    0,
	}
}
func (value *UsageGeneration) UnmarshalJSON(data []byte) error {
	type plain UsageGeneration
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"elapsed_us", "output_tokens", "responses"}, false); err != nil {
		return err
	}
	*value = UsageGeneration(decoded)
	return nil
}
func (value UsageGeneration) MarshalJSON() ([]byte, error) {
	type plain UsageGeneration
	encoded := plain(value)
	return json.Marshal(encoded)
}

type UsagePoint struct {
	StartMs int64                        `json:"start_ms"`
	Metrics UsageMetrics                 `json:"metrics"`
	Models  map[string]ConversationUsage `json:"models"`
}

func DefaultUsagePoint() UsagePoint {
	return UsagePoint{
		StartMs: 0,
		Metrics: DefaultUsageMetrics(),
		Models:  map[string]ConversationUsage{},
	}
}
func (value *UsagePoint) UnmarshalJSON(data []byte) error {
	type plain UsagePoint
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"start_ms", "metrics", "models"}, false); err != nil {
		return err
	}
	*value = UsagePoint(decoded)
	return nil
}
func (value UsagePoint) MarshalJSON() ([]byte, error) {
	type plain UsagePoint
	encoded := plain(value)
	encoded.Models = normalizeCollections(encoded.Models)
	return json.Marshal(encoded)
}

type UsageKey struct {
	Kind string
	Data any
}
type UsageKeyModelPayload struct {
	Provider ProviderID `json:"provider"`
	Model    string     `json:"model"`
}

func DefaultUsageKeyModelPayload() UsageKeyModelPayload {
	return UsageKeyModelPayload{
		Provider: ProviderID{},
		Model:    "",
	}
}
func (value *UsageKeyModelPayload) UnmarshalJSON(data []byte) error {
	type plain UsageKeyModelPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"provider", "model"}, false); err != nil {
		return err
	}
	*value = UsageKeyModelPayload(decoded)
	return nil
}
func (value UsageKeyModelPayload) MarshalJSON() ([]byte, error) {
	type plain UsageKeyModelPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type UsageKeyAssistantPayload struct {
	Package string `json:"package"`
	ID      string `json:"id"`
}

func DefaultUsageKeyAssistantPayload() UsageKeyAssistantPayload {
	return UsageKeyAssistantPayload{
		Package: "",
		ID:      "",
	}
}
func (value *UsageKeyAssistantPayload) UnmarshalJSON(data []byte) error {
	type plain UsageKeyAssistantPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "id"}, false); err != nil {
		return err
	}
	*value = UsageKeyAssistantPayload(decoded)
	return nil
}
func (value UsageKeyAssistantPayload) MarshalJSON() ([]byte, error) {
	type plain UsageKeyAssistantPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type UsageKeyWorktreePayload struct {
	Project  ProjectID  `json:"project"`
	Worktree WorktreeID `json:"worktree"`
}

func DefaultUsageKeyWorktreePayload() UsageKeyWorktreePayload {
	return UsageKeyWorktreePayload{
		Project:  ProjectID{},
		Worktree: WorktreeID{},
	}
}
func (value *UsageKeyWorktreePayload) UnmarshalJSON(data []byte) error {
	type plain UsageKeyWorktreePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"project", "worktree"}, false); err != nil {
		return err
	}
	*value = UsageKeyWorktreePayload(decoded)
	return nil
}
func (value UsageKeyWorktreePayload) MarshalJSON() ([]byte, error) {
	type plain UsageKeyWorktreePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var usageKeyVariants = variantFactories{"provider": func() any { return new(ProviderID) },
	"model":      func() any { return new(UsageKeyModelPayload) },
	"project":    func() any { return new(ProjectID) },
	"session":    func() any { return new(SessionID) },
	"connection": func() any { return new(ConnectionResource) },
	"assistant":  func() any { return new(UsageKeyAssistantPayload) },
	"worktree":   func() any { return new(UsageKeyWorktreePayload) }}

func (value UsageKey) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, usageKeyVariants)
}
func (value *UsageKey) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", usageKeyVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type UsageGroup struct {
	Key     UsageKey     `json:"key"`
	Name    *string      `json:"name"`
	Metrics UsageMetrics `json:"metrics"`
}

func DefaultUsageGroup() UsageGroup {
	return UsageGroup{
		Key:     UsageKey{},
		Name:    nil,
		Metrics: DefaultUsageMetrics(),
	}
}
func (value *UsageGroup) UnmarshalJSON(data []byte) error {
	type plain UsageGroup
	decoded := plain{}
	decoded.Name = nil
	if err := DecodeObject(data, &decoded, []string{"key", "metrics"}, false); err != nil {
		return err
	}
	*value = UsageGroup(decoded)
	return nil
}
func (value UsageGroup) MarshalJSON() ([]byte, error) {
	type plain UsageGroup
	encoded := plain(value)
	return json.Marshal(encoded)
}

type UsageReport struct {
	Node               NodeID       `json:"node"`
	Cursor             uint64       `json:"cursor"`
	PricingUpdatedAtMs *uint64      `json:"pricing_updated_at_ms"`
	Query              UsageQuery   `json:"query"`
	Totals             UsageMetrics `json:"totals"`
	Days               []UsagePoint `json:"days"`
	Recent             []UsagePoint `json:"recent"`
	Groups             []UsageGroup `json:"groups"`
	Resources          []UsageKey   `json:"resources"`
	Requests           UsagePage    `json:"requests"`
}

func DefaultUsageReport() UsageReport {
	return UsageReport{
		Node:               NodeID{},
		Cursor:             0,
		PricingUpdatedAtMs: nil,
		Query:              DefaultUsageQuery(),
		Totals:             DefaultUsageMetrics(),
		Days:               []UsagePoint{},
		Recent:             []UsagePoint{},
		Groups:             []UsageGroup{},
		Resources:          []UsageKey{},
		Requests:           DefaultUsagePage(),
	}
}
func (value *UsageReport) UnmarshalJSON(data []byte) error {
	type plain UsageReport
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"node", "cursor", "query", "totals", "days", "recent", "groups", "resources", "requests"}, false); err != nil {
		return err
	}
	*value = UsageReport(decoded)
	return nil
}
func (value UsageReport) MarshalJSON() ([]byte, error) {
	type plain UsageReport
	encoded := plain(value)
	encoded.Days = normalizeCollections(encoded.Days)
	encoded.Recent = normalizeCollections(encoded.Recent)
	encoded.Groups = normalizeCollections(encoded.Groups)
	encoded.Resources = normalizeCollections(encoded.Resources)
	return json.Marshal(encoded)
}
