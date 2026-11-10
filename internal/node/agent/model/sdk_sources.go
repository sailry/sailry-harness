package model

import (
	"encoding/json"
	"slices"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
)

const sourcesMetadataKey = "sailry.sources"

type sourcesMetadata struct{ Sources canonical.Sources }

func (*sourcesMetadata) Options()                              {}
func (metadata *sourcesMetadata) MarshalJSON() ([]byte, error) { return json.Marshal(metadata.Sources) }

func (metadata *sourcesMetadata) UnmarshalJSON(data []byte) error {
	return json.Unmarshal(data, &metadata.Sources)
}

// StreamSources returns an independent provider source snapshot. Citation
// offsets remain provider character offsets; Store validates presentation ranges.
func StreamSources(part fantasy.StreamPart) (canonical.Sources, bool) {
	metadata, ok := part.ProviderMetadata[sourcesMetadataKey].(*sourcesMetadata)
	if !ok || metadata == nil {
		return canonical.Sources{}, false
	}
	return cloneSources(metadata.Sources), true
}

func cloneSources(value canonical.Sources) canonical.Sources {
	clone := func(value *string) *string {
		if value == nil {
			return nil
		}
		text := *value
		return &text
	}
	index := func(value *int64) *int64 {
		if value == nil {
			return nil
		}
		number := *value
		return &number
	}
	value.Citations = slices.Clone(value.Citations)
	for i := range value.Citations {
		citation := &value.Citations[i]
		citation.URI, citation.Title = clone(citation.URI), clone(citation.Title)
		citation.Start, citation.End = index(citation.Start), index(citation.End)
	}
	value.Grounding = slices.Clone(value.Grounding)
	for i := range value.Grounding {
		value.Grounding[i].Title = clone(value.Grounding[i].Title)
	}
	value.SearchSuggestions = clone(value.SearchSuggestions)
	return value
}

func (emitter *sdkEmitter) citation(value map[string]any, offset int64) {
	emitter.sourcesKnown = true
	uri, ok := value["url"].(string)
	if !ok {
		return
	}
	citation := canonical.Citation{URI: &uri}
	if title, ok := value["title"].(string); ok {
		citation.Title = &title
	}
	if start, end := wireInt(value["start_index"]), wireInt(value["end_index"]); start != nil && end != nil {
		first, last := *start+offset, *end+offset
		citation.Start, citation.End = &first, &last
	}
	equal := func(left, right *string) bool {
		return left == nil && right == nil || left != nil && right != nil && *left == *right
	}
	equalIndex := func(left, right *int64) bool {
		return left == nil && right == nil || left != nil && right != nil && *left == *right
	}
	if !slices.ContainsFunc(emitter.sources.Citations, func(saved canonical.Citation) bool {
		return equal(saved.URI, citation.URI) && equal(saved.Title, citation.Title) && equalIndex(saved.Start, citation.Start) && equalIndex(saved.End, citation.End)
	}) {
		emitter.sources.Citations = append(emitter.sources.Citations, citation)
	}
}

func (emitter *sdkEmitter) grounding(uri string, title any) {
	emitter.sourcesKnown = true
	if uri == "" || slices.ContainsFunc(emitter.sources.Grounding, func(saved canonical.Grounding) bool { return saved.URI == uri }) {
		return
	}
	grounding := canonical.Grounding{URI: uri}
	if title, ok := title.(string); ok {
		grounding.Title = &title
	}
	emitter.sources.Grounding = append(emitter.sources.Grounding, grounding)
}

func (emitter *sdkEmitter) responseSources(items []any) {
	emitter.sourcesKnown = true
	emitter.sources.Citations = nil
	var offset int64
	for _, raw := range items {
		item := wireObject(raw)
		if wireString(item["type"]) != "message" {
			continue
		}
		for _, raw := range wireArray(item["content"]) {
			block := wireObject(raw)
			if wireString(block["type"]) != "output_text" {
				continue
			}
			for _, raw := range wireArray(block["annotations"]) {
				citation := wireObject(raw)
				if wireString(citation["type"]) == "url_citation" {
					emitter.citation(citation, offset)
				}
			}
			// OpenAI indexes characters, matching the canonical projection contract.
			offset += int64(len([]rune(wireString(block["text"]))))
		}
	}
}

func (emitter *sdkEmitter) chatSources(value map[string]any) {
	if _, present := value["annotations"]; present {
		emitter.sourcesKnown = true
	}
	for _, raw := range wireArray(value["annotations"]) {
		annotation := wireObject(raw)
		if wireString(annotation["type"]) == "url_citation" {
			emitter.citation(wireObject(annotation["url_citation"]), 0)
		}
	}
}

func (emitter *sdkEmitter) anthropicSources(content map[string]any) {
	if _, present := content["citations"]; present {
		emitter.sourcesKnown = true
	}
	for _, raw := range wireArray(content["citations"]) {
		citation := wireObject(raw)
		if wireString(citation["type"]) == "web_search_result_location" {
			// Document char/page positions refer to the input document, not the
			// generated answer; native web citations have no answer offsets.
			emitter.citation(citation, 0)
		}
	}
}

func (emitter *sdkEmitter) googleSources(candidate map[string]any) {
	if _, present := candidate["groundingMetadata"]; present {
		emitter.sourcesKnown = true
	}
	metadata := wireObject(candidate["groundingMetadata"])
	for _, raw := range wireArray(metadata["groundingChunks"]) {
		web := wireObject(wireObject(raw)["web"])
		emitter.grounding(wireString(web["uri"]), web["title"])
	}
	if rendered, ok := wireObject(metadata["searchEntryPoint"])["renderedContent"].(string); ok {
		emitter.sources.SearchSuggestions = &rendered
	}
	// Gemini support segments use byte offsets into generated parts. They are
	// not source excerpts; the source grounding contract has no answer range.
}
