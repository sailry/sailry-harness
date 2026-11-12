// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type KnowledgeSource struct {
	Kind string
	Data any
}
type KnowledgeSourceWorktreePayload struct {
	Worktree WorktreeID `json:"worktree"`
	Path     string     `json:"path"`
}

func DefaultKnowledgeSourceWorktreePayload() KnowledgeSourceWorktreePayload {
	return KnowledgeSourceWorktreePayload{
		Worktree: WorktreeID{},
		Path:     "",
	}
}
func (value *KnowledgeSourceWorktreePayload) UnmarshalJSON(data []byte) error {
	type plain KnowledgeSourceWorktreePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path"}, true); err != nil {
		return err
	}
	*value = KnowledgeSourceWorktreePayload(decoded)
	return nil
}
func (value KnowledgeSourceWorktreePayload) MarshalJSON() ([]byte, error) {
	type plain KnowledgeSourceWorktreePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type KnowledgeSourceDirectoryPayload struct {
	Path string `json:"path"`
}

func DefaultKnowledgeSourceDirectoryPayload() KnowledgeSourceDirectoryPayload {
	return KnowledgeSourceDirectoryPayload{
		Path: "",
	}
}
func (value *KnowledgeSourceDirectoryPayload) UnmarshalJSON(data []byte) error {
	type plain KnowledgeSourceDirectoryPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path"}, true); err != nil {
		return err
	}
	*value = KnowledgeSourceDirectoryPayload(decoded)
	return nil
}
func (value KnowledgeSourceDirectoryPayload) MarshalJSON() ([]byte, error) {
	type plain KnowledgeSourceDirectoryPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type KnowledgeSourceFilePayload struct {
	Path string `json:"path"`
}

func DefaultKnowledgeSourceFilePayload() KnowledgeSourceFilePayload {
	return KnowledgeSourceFilePayload{
		Path: "",
	}
}
func (value *KnowledgeSourceFilePayload) UnmarshalJSON(data []byte) error {
	type plain KnowledgeSourceFilePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path"}, true); err != nil {
		return err
	}
	*value = KnowledgeSourceFilePayload(decoded)
	return nil
}
func (value KnowledgeSourceFilePayload) MarshalJSON() ([]byte, error) {
	type plain KnowledgeSourceFilePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type KnowledgeSourceStoragePayload struct {
	Scope  PluginStorageScope `json:"scope"`
	Prefix string             `json:"prefix"`
}

func DefaultKnowledgeSourceStoragePayload() KnowledgeSourceStoragePayload {
	return KnowledgeSourceStoragePayload{
		Scope:  PluginStorageScope(""),
		Prefix: "",
	}
}
func (value *KnowledgeSourceStoragePayload) UnmarshalJSON(data []byte) error {
	type plain KnowledgeSourceStoragePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"scope", "prefix"}, true); err != nil {
		return err
	}
	*value = KnowledgeSourceStoragePayload(decoded)
	return nil
}
func (value KnowledgeSourceStoragePayload) MarshalJSON() ([]byte, error) {
	type plain KnowledgeSourceStoragePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var knowledgeSourceVariants = variantFactories{"worktree": func() any { return new(KnowledgeSourceWorktreePayload) },
	"directory": func() any { return new(KnowledgeSourceDirectoryPayload) },
	"file":      func() any { return new(KnowledgeSourceFilePayload) },
	"storage":   func() any { return new(KnowledgeSourceStoragePayload) }}

func (value KnowledgeSource) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, knowledgeSourceVariants)
}
func (value *KnowledgeSource) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", knowledgeSourceVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type KnowledgeOwner struct {
	Kind string
	Data any
}
type KnowledgeOwnerPluginPayload struct {
	Name string `json:"name"`
}

func DefaultKnowledgeOwnerPluginPayload() KnowledgeOwnerPluginPayload {
	return KnowledgeOwnerPluginPayload{
		Name: "",
	}
}
func (value *KnowledgeOwnerPluginPayload) UnmarshalJSON(data []byte) error {
	type plain KnowledgeOwnerPluginPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name"}, true); err != nil {
		return err
	}
	*value = KnowledgeOwnerPluginPayload(decoded)
	return nil
}
func (value KnowledgeOwnerPluginPayload) MarshalJSON() ([]byte, error) {
	type plain KnowledgeOwnerPluginPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var knowledgeOwnerVariants = variantFactories{"controller": nil,
	"plugin": func() any { return new(KnowledgeOwnerPluginPayload) }}

func (value KnowledgeOwner) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, knowledgeOwnerVariants)
}
func (value *KnowledgeOwner) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", knowledgeOwnerVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type KnowledgeDefinition struct {
	Name       string          `json:"name"`
	Source     KnowledgeSource `json:"source"`
	Model      *string         `json:"model"`
	Globs      []string        `json:"globs"`
	Tags       []string        `json:"tags"`
	SharedWith []string        `json:"shared_with"`
}

func DefaultKnowledgeDefinition() KnowledgeDefinition {
	return KnowledgeDefinition{
		Name:       "",
		Source:     KnowledgeSource{},
		Model:      nil,
		Globs:      []string{},
		Tags:       []string{},
		SharedWith: []string{},
	}
}
func (value *KnowledgeDefinition) UnmarshalJSON(data []byte) error {
	type plain KnowledgeDefinition
	decoded := plain{}
	decoded.Model = nil
	decoded.Globs = []string{}
	decoded.Tags = []string{}
	decoded.SharedWith = []string{}
	if err := DecodeObject(data, &decoded, []string{"name", "source"}, true); err != nil {
		return err
	}
	*value = KnowledgeDefinition(decoded)
	return nil
}
func (value KnowledgeDefinition) MarshalJSON() ([]byte, error) {
	type plain KnowledgeDefinition
	encoded := plain(value)
	encoded.Globs = normalizeCollections(encoded.Globs)
	encoded.Tags = normalizeCollections(encoded.Tags)
	encoded.SharedWith = normalizeCollections(encoded.SharedWith)
	return json.Marshal(encoded)
}

type KnowledgeDraft struct {
	Name   string               `json:"name"`
	Source KnowledgeDraftSource `json:"source"`
	Model  *string              `json:"model"`
	Globs  []string             `json:"globs"`
	Tags   []string             `json:"tags"`
}

func DefaultKnowledgeDraft() KnowledgeDraft {
	return KnowledgeDraft{
		Name:   "",
		Source: KnowledgeDraftSource{},
		Model:  nil,
		Globs:  []string{},
		Tags:   []string{},
	}
}
func (value *KnowledgeDraft) UnmarshalJSON(data []byte) error {
	type plain KnowledgeDraft
	decoded := plain{}
	decoded.Model = nil
	decoded.Globs = []string{}
	decoded.Tags = []string{}
	if err := DecodeObject(data, &decoded, []string{"name", "source"}, true); err != nil {
		return err
	}
	*value = KnowledgeDraft(decoded)
	return nil
}
func (value KnowledgeDraft) MarshalJSON() ([]byte, error) {
	type plain KnowledgeDraft
	encoded := plain(value)
	encoded.Globs = normalizeCollections(encoded.Globs)
	encoded.Tags = normalizeCollections(encoded.Tags)
	return json.Marshal(encoded)
}

type KnowledgeDraftSource struct {
	Kind string
	Data any
}
type KnowledgeDraftSourceWorktreePayload struct {
	Path string `json:"path"`
}

func DefaultKnowledgeDraftSourceWorktreePayload() KnowledgeDraftSourceWorktreePayload {
	return KnowledgeDraftSourceWorktreePayload{
		Path: "",
	}
}
func (value *KnowledgeDraftSourceWorktreePayload) UnmarshalJSON(data []byte) error {
	type plain KnowledgeDraftSourceWorktreePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path"}, true); err != nil {
		return err
	}
	*value = KnowledgeDraftSourceWorktreePayload(decoded)
	return nil
}
func (value KnowledgeDraftSourceWorktreePayload) MarshalJSON() ([]byte, error) {
	type plain KnowledgeDraftSourceWorktreePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type KnowledgeDraftSourceStoragePayload struct {
	Scope  PluginStorageScope `json:"scope"`
	Prefix string             `json:"prefix"`
}

func DefaultKnowledgeDraftSourceStoragePayload() KnowledgeDraftSourceStoragePayload {
	return KnowledgeDraftSourceStoragePayload{
		Scope:  PluginStorageScope(""),
		Prefix: "",
	}
}
func (value *KnowledgeDraftSourceStoragePayload) UnmarshalJSON(data []byte) error {
	type plain KnowledgeDraftSourceStoragePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"scope", "prefix"}, true); err != nil {
		return err
	}
	*value = KnowledgeDraftSourceStoragePayload(decoded)
	return nil
}
func (value KnowledgeDraftSourceStoragePayload) MarshalJSON() ([]byte, error) {
	type plain KnowledgeDraftSourceStoragePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var knowledgeDraftSourceVariants = variantFactories{"worktree": func() any { return new(KnowledgeDraftSourceWorktreePayload) },
	"storage": func() any { return new(KnowledgeDraftSourceStoragePayload) }}

func (value KnowledgeDraftSource) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, knowledgeDraftSourceVariants)
}
func (value *KnowledgeDraftSource) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", knowledgeDraftSourceVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type KnowledgeStatus string

const (
	KnowledgeStatusPending  KnowledgeStatus = "pending"
	KnowledgeStatusIndexing KnowledgeStatus = "indexing"
	KnowledgeStatusReady    KnowledgeStatus = "ready"
	KnowledgeStatusPaused   KnowledgeStatus = "paused"
)

func (value KnowledgeStatus) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *KnowledgeStatus) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch KnowledgeStatus(decoded) {
	case KnowledgeStatusPending, KnowledgeStatusIndexing, KnowledgeStatusReady, KnowledgeStatusPaused:
		*value = KnowledgeStatus(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value KnowledgeStatus) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *KnowledgeStatus) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

// KnowledgeFailure is one source the last pass could not read or index. An
// empty path names the collection itself.
type KnowledgeFailure struct {
	Path   string `json:"path"`
	Reason string `json:"reason"`
}

func DefaultKnowledgeFailure() KnowledgeFailure {
	return KnowledgeFailure{
		Path:   "",
		Reason: "",
	}
}
func (value *KnowledgeFailure) UnmarshalJSON(data []byte) error {
	type plain KnowledgeFailure
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "reason"}, false); err != nil {
		return err
	}
	*value = KnowledgeFailure(decoded)
	return nil
}
func (value KnowledgeFailure) MarshalJSON() ([]byte, error) {
	type plain KnowledgeFailure
	encoded := plain(value)
	return json.Marshal(encoded)
}

type KnowledgeProgress struct {
	Status     KnowledgeStatus `json:"status"`
	Sources    uint64          `json:"sources"`
	Completed  uint64          `json:"completed"`
	Chunks     uint64          `json:"chunks"`
	Skipped    uint64          `json:"skipped"`
	Model      *string         `json:"model"`
	Generation uint64          `json:"generation"`
	// Failed counts the sources the last pass could not read or index;
	// Failures names the first of them.
	Failed   uint64             `json:"failed"`
	Failures []KnowledgeFailure `json:"failures"`
}

func DefaultKnowledgeProgress() KnowledgeProgress {
	return KnowledgeProgress{
		Status:     KnowledgeStatusPending,
		Sources:    0,
		Completed:  0,
		Chunks:     0,
		Skipped:    0,
		Model:      nil,
		Generation: 0,
		Failed:     0,
		Failures:   []KnowledgeFailure{},
	}
}
func (value *KnowledgeProgress) UnmarshalJSON(data []byte) error {
	type plain KnowledgeProgress
	decoded := plain{}
	decoded.Failed = 0
	decoded.Failures = []KnowledgeFailure{}
	if err := DecodeObject(data, &decoded, []string{"status", "sources", "completed", "chunks", "skipped", "generation"}, false); err != nil {
		return err
	}
	*value = KnowledgeProgress(decoded)
	return nil
}
func (value KnowledgeProgress) MarshalJSON() ([]byte, error) {
	type plain KnowledgeProgress
	encoded := plain(value)
	encoded.Failures = normalizeCollections(encoded.Failures)
	return json.Marshal(encoded)
}

type KnowledgeCollection struct {
	ID         KnowledgeID         `json:"id"`
	Revision   uint64              `json:"revision"`
	Rebuild    uint64              `json:"rebuild"`
	Owner      KnowledgeOwner      `json:"owner"`
	Definition KnowledgeDefinition `json:"definition"`
	Paused     bool                `json:"paused"`
	Progress   KnowledgeProgress   `json:"progress"`
}

func DefaultKnowledgeCollection() KnowledgeCollection {
	return KnowledgeCollection{
		ID:         KnowledgeID{},
		Revision:   0,
		Rebuild:    0,
		Owner:      KnowledgeOwner{},
		Definition: DefaultKnowledgeDefinition(),
		Paused:     false,
		Progress:   DefaultKnowledgeProgress(),
	}
}
func (value *KnowledgeCollection) UnmarshalJSON(data []byte) error {
	type plain KnowledgeCollection
	decoded := plain{}
	decoded.Rebuild = 0
	decoded.Paused = false
	if err := DecodeObject(data, &decoded, []string{"id", "revision", "owner", "definition", "progress"}, false); err != nil {
		return err
	}
	*value = KnowledgeCollection(decoded)
	return nil
}
func (value KnowledgeCollection) MarshalJSON() ([]byte, error) {
	type plain KnowledgeCollection
	encoded := plain(value)
	return json.Marshal(encoded)
}

type KnowledgeMode string

const (
	KnowledgeModeKeyword  KnowledgeMode = "keyword"
	KnowledgeModeSemantic KnowledgeMode = "semantic"
	KnowledgeModeHybrid   KnowledgeMode = "hybrid"
)

func (value KnowledgeMode) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *KnowledgeMode) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch KnowledgeMode(decoded) {
	case KnowledgeModeKeyword, KnowledgeModeSemantic, KnowledgeModeHybrid:
		*value = KnowledgeMode(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value KnowledgeMode) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *KnowledgeMode) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type KnowledgeQuery struct {
	Query            string        `json:"query"`
	Collections      []KnowledgeID `json:"collections"`
	Mode             KnowledgeMode `json:"mode"`
	Tags             []string      `json:"tags"`
	Path             *string       `json:"path"`
	ModifiedAfterMs  *uint64       `json:"modified_after_ms"`
	ModifiedBeforeMs *uint64       `json:"modified_before_ms"`
	Limit            uint16        `json:"limit"`
	Offset           uint32        `json:"offset"`
}

func DefaultKnowledgeQuery() KnowledgeQuery {
	return KnowledgeQuery{
		Query:            "",
		Collections:      []KnowledgeID{},
		Mode:             KnowledgeModeHybrid,
		Tags:             []string{},
		Path:             nil,
		ModifiedAfterMs:  nil,
		ModifiedBeforeMs: nil,
		Limit:            0,
		Offset:           0,
	}
}
func (value *KnowledgeQuery) UnmarshalJSON(data []byte) error {
	type plain KnowledgeQuery
	decoded := plain{}
	decoded.Collections = []KnowledgeID{}
	decoded.Mode = KnowledgeModeHybrid
	decoded.Tags = []string{}
	decoded.Path = nil
	decoded.ModifiedAfterMs = nil
	decoded.ModifiedBeforeMs = nil
	decoded.Limit = 5
	decoded.Offset = 0
	if err := DecodeObject(data, &decoded, []string{"query"}, true); err != nil {
		return err
	}
	*value = KnowledgeQuery(decoded)
	return nil
}
func (value KnowledgeQuery) MarshalJSON() ([]byte, error) {
	type plain KnowledgeQuery
	encoded := plain(value)
	encoded.Collections = normalizeCollections(encoded.Collections)
	encoded.Tags = normalizeCollections(encoded.Tags)
	return json.Marshal(encoded)
}

type KnowledgePosition struct {
	Kind string
	Data any
}
type KnowledgePositionLinesPayload struct {
	Start uint32 `json:"start"`
	End   uint32 `json:"end"`
}

func DefaultKnowledgePositionLinesPayload() KnowledgePositionLinesPayload {
	return KnowledgePositionLinesPayload{
		Start: 0,
		End:   0,
	}
}
func (value *KnowledgePositionLinesPayload) UnmarshalJSON(data []byte) error {
	type plain KnowledgePositionLinesPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"start", "end"}, true); err != nil {
		return err
	}
	*value = KnowledgePositionLinesPayload(decoded)
	return nil
}
func (value KnowledgePositionLinesPayload) MarshalJSON() ([]byte, error) {
	type plain KnowledgePositionLinesPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type KnowledgePositionPagePayload struct {
	Page uint32 `json:"page"`
}

func DefaultKnowledgePositionPagePayload() KnowledgePositionPagePayload {
	return KnowledgePositionPagePayload{
		Page: 0,
	}
}
func (value *KnowledgePositionPagePayload) UnmarshalJSON(data []byte) error {
	type plain KnowledgePositionPagePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"page"}, true); err != nil {
		return err
	}
	*value = KnowledgePositionPagePayload(decoded)
	return nil
}
func (value KnowledgePositionPagePayload) MarshalJSON() ([]byte, error) {
	type plain KnowledgePositionPagePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type KnowledgePositionCellsPayload struct {
	Sheet string `json:"sheet"`
	Start string `json:"start"`
	End   string `json:"end"`
}

func DefaultKnowledgePositionCellsPayload() KnowledgePositionCellsPayload {
	return KnowledgePositionCellsPayload{
		Sheet: "",
		Start: "",
		End:   "",
	}
}
func (value *KnowledgePositionCellsPayload) UnmarshalJSON(data []byte) error {
	type plain KnowledgePositionCellsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"sheet", "start", "end"}, true); err != nil {
		return err
	}
	*value = KnowledgePositionCellsPayload(decoded)
	return nil
}
func (value KnowledgePositionCellsPayload) MarshalJSON() ([]byte, error) {
	type plain KnowledgePositionCellsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type KnowledgePositionSlidePayload struct {
	Slide uint32 `json:"slide"`
}

func DefaultKnowledgePositionSlidePayload() KnowledgePositionSlidePayload {
	return KnowledgePositionSlidePayload{
		Slide: 0,
	}
}
func (value *KnowledgePositionSlidePayload) UnmarshalJSON(data []byte) error {
	type plain KnowledgePositionSlidePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"slide"}, true); err != nil {
		return err
	}
	*value = KnowledgePositionSlidePayload(decoded)
	return nil
}
func (value KnowledgePositionSlidePayload) MarshalJSON() ([]byte, error) {
	type plain KnowledgePositionSlidePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type KnowledgePositionSectionPayload struct {
	Name string `json:"name"`
}

func DefaultKnowledgePositionSectionPayload() KnowledgePositionSectionPayload {
	return KnowledgePositionSectionPayload{
		Name: "",
	}
}
func (value *KnowledgePositionSectionPayload) UnmarshalJSON(data []byte) error {
	type plain KnowledgePositionSectionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name"}, true); err != nil {
		return err
	}
	*value = KnowledgePositionSectionPayload(decoded)
	return nil
}
func (value KnowledgePositionSectionPayload) MarshalJSON() ([]byte, error) {
	type plain KnowledgePositionSectionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type KnowledgePositionValuePayload struct {
	Key     string `json:"key"`
	Pointer string `json:"pointer"`
}

func DefaultKnowledgePositionValuePayload() KnowledgePositionValuePayload {
	return KnowledgePositionValuePayload{
		Key:     "",
		Pointer: "",
	}
}
func (value *KnowledgePositionValuePayload) UnmarshalJSON(data []byte) error {
	type plain KnowledgePositionValuePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key", "pointer"}, true); err != nil {
		return err
	}
	*value = KnowledgePositionValuePayload(decoded)
	return nil
}
func (value KnowledgePositionValuePayload) MarshalJSON() ([]byte, error) {
	type plain KnowledgePositionValuePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var knowledgePositionVariants = variantFactories{"lines": func() any { return new(KnowledgePositionLinesPayload) },
	"page":    func() any { return new(KnowledgePositionPagePayload) },
	"cells":   func() any { return new(KnowledgePositionCellsPayload) },
	"slide":   func() any { return new(KnowledgePositionSlidePayload) },
	"section": func() any { return new(KnowledgePositionSectionPayload) },
	"value":   func() any { return new(KnowledgePositionValuePayload) }}

func (value KnowledgePosition) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, knowledgePositionVariants)
}
func (value *KnowledgePosition) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", knowledgePositionVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type KnowledgeCitation struct {
	Collection KnowledgeID       `json:"collection"`
	Chunk      string            `json:"chunk"`
	Revision   string            `json:"revision"`
	Path       string            `json:"path"`
	Position   KnowledgePosition `json:"position"`
	Link       string            `json:"link"`
}

func DefaultKnowledgeCitation() KnowledgeCitation {
	return KnowledgeCitation{
		Collection: KnowledgeID{},
		Chunk:      "",
		Revision:   "",
		Path:       "",
		Position:   KnowledgePosition{},
		Link:       "",
	}
}
func (value *KnowledgeCitation) UnmarshalJSON(data []byte) error {
	type plain KnowledgeCitation
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"collection", "chunk", "revision", "path", "position", "link"}, false); err != nil {
		return err
	}
	*value = KnowledgeCitation(decoded)
	return nil
}
func (value KnowledgeCitation) MarshalJSON() ([]byte, error) {
	type plain KnowledgeCitation
	encoded := plain(value)
	return json.Marshal(encoded)
}

type KnowledgeCitationRequest struct {
	Collection KnowledgeID `json:"collection"`
	Chunk      string      `json:"chunk"`
	Revision   string      `json:"revision"`
	Context    bool        `json:"context"`
	Offset     uint32      `json:"offset"`
	Limit      uint32      `json:"limit"`
}

func DefaultKnowledgeCitationRequest() KnowledgeCitationRequest {
	return KnowledgeCitationRequest{
		Collection: KnowledgeID{},
		Chunk:      "",
		Revision:   "",
		Context:    false,
		Offset:     0,
		Limit:      0,
	}
}
func (value *KnowledgeCitationRequest) UnmarshalJSON(data []byte) error {
	type plain KnowledgeCitationRequest
	decoded := plain{}
	decoded.Context = false
	decoded.Offset = 0
	decoded.Limit = 16 * 1024
	if err := DecodeObject(data, &decoded, []string{"collection", "chunk", "revision"}, true); err != nil {
		return err
	}
	*value = KnowledgeCitationRequest(decoded)
	return nil
}
func (value KnowledgeCitationRequest) MarshalJSON() ([]byte, error) {
	type plain KnowledgeCitationRequest
	encoded := plain(value)
	return json.Marshal(encoded)
}

type KnowledgeCitationPage struct {
	Citation   KnowledgeCitation `json:"citation"`
	Title      string            `json:"title"`
	Text       string            `json:"text"`
	Offset     uint32            `json:"offset"`
	NextOffset *uint32           `json:"next_offset"`
	Worktree   *WorktreeID       `json:"worktree"`
	Context    bool              `json:"context"`
	Untrusted  bool              `json:"untrusted"`
}

func DefaultKnowledgeCitationPage() KnowledgeCitationPage {
	return KnowledgeCitationPage{
		Citation:   DefaultKnowledgeCitation(),
		Title:      "",
		Text:       "",
		Offset:     0,
		NextOffset: nil,
		Worktree:   nil,
		Context:    false,
		Untrusted:  false,
	}
}
func (value *KnowledgeCitationPage) UnmarshalJSON(data []byte) error {
	type plain KnowledgeCitationPage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"citation", "title", "text", "offset", "context", "untrusted"}, false); err != nil {
		return err
	}
	*value = KnowledgeCitationPage(decoded)
	return nil
}
func (value KnowledgeCitationPage) MarshalJSON() ([]byte, error) {
	type plain KnowledgeCitationPage
	encoded := plain(value)
	return json.Marshal(encoded)
}

type KnowledgeHit struct {
	Title    string            `json:"title"`
	Text     string            `json:"text"`
	Score    uint32            `json:"score"`
	Citation KnowledgeCitation `json:"citation"`
	Stale    bool              `json:"stale"`
}

func DefaultKnowledgeHit() KnowledgeHit {
	return KnowledgeHit{
		Title:    "",
		Text:     "",
		Score:    0,
		Citation: DefaultKnowledgeCitation(),
		Stale:    false,
	}
}
func (value *KnowledgeHit) UnmarshalJSON(data []byte) error {
	type plain KnowledgeHit
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"title", "text", "score", "citation", "stale"}, false); err != nil {
		return err
	}
	*value = KnowledgeHit(decoded)
	return nil
}
func (value KnowledgeHit) MarshalJSON() ([]byte, error) {
	type plain KnowledgeHit
	encoded := plain(value)
	return json.Marshal(encoded)
}

type KnowledgePage struct {
	Hits       []KnowledgeHit `json:"hits"`
	NextOffset *uint32        `json:"next_offset"`
	Mode       KnowledgeMode  `json:"mode"`
	Indexing   bool           `json:"indexing"`
	Model      *string        `json:"model"`
	Untrusted  bool           `json:"untrusted"`
}

func DefaultKnowledgePage() KnowledgePage {
	return KnowledgePage{
		Hits:       []KnowledgeHit{},
		NextOffset: nil,
		Mode:       KnowledgeModeHybrid,
		Indexing:   false,
		Model:      nil,
		Untrusted:  false,
	}
}
func (value *KnowledgePage) UnmarshalJSON(data []byte) error {
	type plain KnowledgePage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"hits", "mode", "indexing", "untrusted"}, false); err != nil {
		return err
	}
	*value = KnowledgePage(decoded)
	return nil
}
func (value KnowledgePage) MarshalJSON() ([]byte, error) {
	type plain KnowledgePage
	encoded := plain(value)
	encoded.Hits = normalizeCollections(encoded.Hits)
	return json.Marshal(encoded)
}
