// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type CodeIndexGraphView string

const (
	CodeIndexGraphViewCallers CodeIndexGraphView = "callers"
	CodeIndexGraphViewCallees CodeIndexGraphView = "callees"
	CodeIndexGraphViewContext CodeIndexGraphView = "context"
	CodeIndexGraphViewImpact  CodeIndexGraphView = "impact"
	CodeIndexGraphViewMap     CodeIndexGraphView = "map"
)

func (value CodeIndexGraphView) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *CodeIndexGraphView) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch CodeIndexGraphView(decoded) {
	case CodeIndexGraphViewCallers, CodeIndexGraphViewCallees, CodeIndexGraphViewContext, CodeIndexGraphViewImpact, CodeIndexGraphViewMap:
		*value = CodeIndexGraphView(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value CodeIndexGraphView) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *CodeIndexGraphView) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type CodeIndexGraph struct {
	View   CodeIndexGraphView `json:"view"`
	Name   *string            `json:"name"`
	Paths  []string           `json:"paths"`
	Line   *uint32            `json:"line"`
	Depth  uint8              `json:"depth"`
	Offset uint64             `json:"offset"`
	Limit  uint64             `json:"limit"`
}

func DefaultCodeIndexGraph() CodeIndexGraph {
	return CodeIndexGraph{
		View:   CodeIndexGraphView(""),
		Name:   nil,
		Paths:  []string{},
		Line:   nil,
		Depth:  0,
		Offset: 0,
		Limit:  0,
	}
}
func (value *CodeIndexGraph) UnmarshalJSON(data []byte) error {
	type plain CodeIndexGraph
	decoded := plain{}
	decoded.Name = nil
	decoded.Paths = []string{}
	decoded.Line = nil
	decoded.Depth = 3
	decoded.Offset = 0
	decoded.Limit = 50
	if err := DecodeObject(data, &decoded, []string{"view"}, true); err != nil {
		return err
	}
	*value = CodeIndexGraph(decoded)
	return nil
}
func (value CodeIndexGraph) MarshalJSON() ([]byte, error) {
	type plain CodeIndexGraph
	encoded := plain(value)
	encoded.Paths = normalizeCollections(encoded.Paths)
	return json.Marshal(encoded)
}

type CodeIndexGraphConfidence string

const (
	CodeIndexGraphConfidenceLocal     CodeIndexGraphConfidence = "local"
	CodeIndexGraphConfidenceImported  CodeIndexGraphConfidence = "imported"
	CodeIndexGraphConfidenceDirectory CodeIndexGraphConfidence = "directory"
	CodeIndexGraphConfidenceWorkspace CodeIndexGraphConfidence = "workspace"
)

func (value CodeIndexGraphConfidence) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *CodeIndexGraphConfidence) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch CodeIndexGraphConfidence(decoded) {
	case CodeIndexGraphConfidenceLocal, CodeIndexGraphConfidenceImported, CodeIndexGraphConfidenceDirectory, CodeIndexGraphConfidenceWorkspace:
		*value = CodeIndexGraphConfidence(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value CodeIndexGraphConfidence) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *CodeIndexGraphConfidence) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type CodeIndexGraphDefinition struct {
	Path      string `json:"path"`
	Name      string `json:"name"`
	Kind      string `json:"kind"`
	StartLine uint32 `json:"start_line"`
	EndLine   uint32 `json:"end_line"`
}

func DefaultCodeIndexGraphDefinition() CodeIndexGraphDefinition {
	return CodeIndexGraphDefinition{
		Path:      "",
		Name:      "",
		Kind:      "",
		StartLine: 0,
		EndLine:   0,
	}
}
func (value *CodeIndexGraphDefinition) UnmarshalJSON(data []byte) error {
	type plain CodeIndexGraphDefinition
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "name", "kind", "start_line", "end_line"}, false); err != nil {
		return err
	}
	*value = CodeIndexGraphDefinition(decoded)
	return nil
}
func (value CodeIndexGraphDefinition) MarshalJSON() ([]byte, error) {
	type plain CodeIndexGraphDefinition
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeIndexGraphReference struct {
	Path       string                     `json:"path"`
	Name       string                     `json:"name"`
	Kind       string                     `json:"kind"`
	Line       uint32                     `json:"line"`
	Column     uint32                     `json:"column"`
	Count      uint32                     `json:"count"`
	Scope      *CodeIndexGraphDefinition  `json:"scope"`
	Targets    []CodeIndexGraphDefinition `json:"targets"`
	Candidates uint64                     `json:"candidates"`
	Confidence *CodeIndexGraphConfidence  `json:"confidence"`
	Ambiguous  bool                       `json:"ambiguous"`
}

func DefaultCodeIndexGraphReference() CodeIndexGraphReference {
	return CodeIndexGraphReference{
		Path:       "",
		Name:       "",
		Kind:       "",
		Line:       0,
		Column:     0,
		Count:      0,
		Scope:      nil,
		Targets:    []CodeIndexGraphDefinition{},
		Candidates: 0,
		Confidence: nil,
		Ambiguous:  false,
	}
}
func (value *CodeIndexGraphReference) UnmarshalJSON(data []byte) error {
	type plain CodeIndexGraphReference
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "name", "kind", "line", "column", "count", "targets", "candidates", "ambiguous"}, false); err != nil {
		return err
	}
	*value = CodeIndexGraphReference(decoded)
	return nil
}
func (value CodeIndexGraphReference) MarshalJSON() ([]byte, error) {
	type plain CodeIndexGraphReference
	encoded := plain(value)
	encoded.Targets = normalizeCollections(encoded.Targets)
	return json.Marshal(encoded)
}

type CodeIndexGraphAffected struct {
	Name       string                   `json:"name"`
	Kind       string                   `json:"kind"`
	StartLine  uint32                   `json:"start_line"`
	EndLine    uint32                   `json:"end_line"`
	Depth      uint8                    `json:"depth"`
	Confidence CodeIndexGraphConfidence `json:"confidence"`
	Ambiguous  bool                     `json:"ambiguous"`
}

func DefaultCodeIndexGraphAffected() CodeIndexGraphAffected {
	return CodeIndexGraphAffected{
		Name:       "",
		Kind:       "",
		StartLine:  0,
		EndLine:    0,
		Depth:      0,
		Confidence: CodeIndexGraphConfidence(""),
		Ambiguous:  false,
	}
}
func (value *CodeIndexGraphAffected) UnmarshalJSON(data []byte) error {
	type plain CodeIndexGraphAffected
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "kind", "start_line", "end_line", "depth", "confidence", "ambiguous"}, false); err != nil {
		return err
	}
	*value = CodeIndexGraphAffected(decoded)
	return nil
}
func (value CodeIndexGraphAffected) MarshalJSON() ([]byte, error) {
	type plain CodeIndexGraphAffected
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeIndexGraphImpacted struct {
	Path       string                   `json:"path"`
	Depth      uint8                    `json:"depth"`
	Confidence CodeIndexGraphConfidence `json:"confidence"`
	Ambiguous  bool                     `json:"ambiguous"`
	Symbols    []CodeIndexGraphAffected `json:"symbols"`
	Truncated  bool                     `json:"truncated"`
}

func DefaultCodeIndexGraphImpacted() CodeIndexGraphImpacted {
	return CodeIndexGraphImpacted{
		Path:       "",
		Depth:      0,
		Confidence: CodeIndexGraphConfidence(""),
		Ambiguous:  false,
		Symbols:    []CodeIndexGraphAffected{},
		Truncated:  false,
	}
}
func (value *CodeIndexGraphImpacted) UnmarshalJSON(data []byte) error {
	type plain CodeIndexGraphImpacted
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "depth", "confidence", "ambiguous", "symbols", "truncated"}, false); err != nil {
		return err
	}
	*value = CodeIndexGraphImpacted(decoded)
	return nil
}
func (value CodeIndexGraphImpacted) MarshalJSON() ([]byte, error) {
	type plain CodeIndexGraphImpacted
	encoded := plain(value)
	encoded.Symbols = normalizeCollections(encoded.Symbols)
	return json.Marshal(encoded)
}

type CodeIndexGraphRanked struct {
	Name       string  `json:"name"`
	Kind       string  `json:"kind"`
	StartLine  uint32  `json:"start_line"`
	EndLine    uint32  `json:"end_line"`
	Signature  *string `json:"signature"`
	References uint32  `json:"references"`
}

func DefaultCodeIndexGraphRanked() CodeIndexGraphRanked {
	return CodeIndexGraphRanked{
		Name:       "",
		Kind:       "",
		StartLine:  0,
		EndLine:    0,
		Signature:  nil,
		References: 0,
	}
}
func (value *CodeIndexGraphRanked) UnmarshalJSON(data []byte) error {
	type plain CodeIndexGraphRanked
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "kind", "start_line", "end_line", "references"}, false); err != nil {
		return err
	}
	*value = CodeIndexGraphRanked(decoded)
	return nil
}
func (value CodeIndexGraphRanked) MarshalJSON() ([]byte, error) {
	type plain CodeIndexGraphRanked
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeIndexGraphEntry struct {
	Path    string                 `json:"path"`
	Score   uint32                 `json:"score"`
	Symbols []CodeIndexGraphRanked `json:"symbols"`
}

func DefaultCodeIndexGraphEntry() CodeIndexGraphEntry {
	return CodeIndexGraphEntry{
		Path:    "",
		Score:   0,
		Symbols: []CodeIndexGraphRanked{},
	}
}
func (value *CodeIndexGraphEntry) UnmarshalJSON(data []byte) error {
	type plain CodeIndexGraphEntry
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "score", "symbols"}, false); err != nil {
		return err
	}
	*value = CodeIndexGraphEntry(decoded)
	return nil
}
func (value CodeIndexGraphEntry) MarshalJSON() ([]byte, error) {
	type plain CodeIndexGraphEntry
	encoded := plain(value)
	encoded.Symbols = normalizeCollections(encoded.Symbols)
	return json.Marshal(encoded)
}

type CodeIndexGraphGraphPage struct {
	View          CodeIndexGraphView         `json:"view"`
	IndexRevision uint64                     `json:"index_revision"`
	Targets       []CodeIndexGraphDefinition `json:"targets"`
	Callers       []CodeIndexGraphReference  `json:"callers"`
	Callees       []CodeIndexGraphReference  `json:"callees"`
	Impact        []CodeIndexGraphImpacted   `json:"impact"`
	Map           []CodeIndexGraphEntry      `json:"map"`
	Total         uint64                     `json:"total"`
	NextOffset    *uint64                    `json:"next_offset"`
	Partial       bool                       `json:"partial"`
	Truncated     bool                       `json:"truncated"`
	Visited       uint64                     `json:"visited"`
	Skipped       uint64                     `json:"skipped"`
	Issues        []CodeIndexIssue           `json:"issues"`
}

func DefaultCodeIndexGraphGraphPage() CodeIndexGraphGraphPage {
	return CodeIndexGraphGraphPage{
		View:          CodeIndexGraphView(""),
		IndexRevision: 0,
		Targets:       []CodeIndexGraphDefinition{},
		Callers:       []CodeIndexGraphReference{},
		Callees:       []CodeIndexGraphReference{},
		Impact:        []CodeIndexGraphImpacted{},
		Map:           []CodeIndexGraphEntry{},
		Total:         0,
		NextOffset:    nil,
		Partial:       false,
		Truncated:     false,
		Visited:       0,
		Skipped:       0,
		Issues:        []CodeIndexIssue{},
	}
}
func (value *CodeIndexGraphGraphPage) UnmarshalJSON(data []byte) error {
	type plain CodeIndexGraphGraphPage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"view", "index_revision", "targets", "callers", "callees", "impact", "map", "total", "partial", "truncated", "visited", "skipped", "issues"}, false); err != nil {
		return err
	}
	*value = CodeIndexGraphGraphPage(decoded)
	return nil
}
func (value CodeIndexGraphGraphPage) MarshalJSON() ([]byte, error) {
	type plain CodeIndexGraphGraphPage
	encoded := plain(value)
	encoded.Targets = normalizeCollections(encoded.Targets)
	encoded.Callers = normalizeCollections(encoded.Callers)
	encoded.Callees = normalizeCollections(encoded.Callees)
	encoded.Impact = normalizeCollections(encoded.Impact)
	encoded.Map = normalizeCollections(encoded.Map)
	encoded.Issues = normalizeCollections(encoded.Issues)
	return json.Marshal(encoded)
}
