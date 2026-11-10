package localmodels

import (
	"bytes"
	"context"
	"encoding/json"
	"maps"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// readPrefixes reads the query and document prompts of a Sentence
// Transformers export.
func readPrefixes(ctx context.Context, hub *catalog, found *repository) (*string, *string, error) {
	entry, present := found.files["config_sentence_transformers.json"]
	if !present {
		return nil, nil, nil
	}
	bad := invalid("model input prompt metadata is invalid")
	value, _, err := document(ctx, hub, entry, "metadata.json")
	if err != nil {
		return nil, nil, err
	}
	if _, object := value.(map[string]any); !object {
		return nil, nil, bad
	}
	prompts, declared := field(value, "prompts")
	if _, object := prompts.(map[string]any); declared && !object {
		return nil, nil, bad
	}
	prompt := func(name string) (*string, error) {
		raw, present := field(prompts, name)
		if !present {
			return nil, nil
		}
		result, ok := raw.(string)
		if !ok || len(result) > MaxPrefixBytes || strings.Contains(result, "\x00") {
			return nil, bad
		}
		return &result, nil
	}
	var fallback *string
	if raw, present := field(value, "default_prompt_name"); present && raw != nil {
		name, _ := raw.(string)
		if !slices.Contains([]string{"query", "document", "passage", "corpus"}, name) {
			return nil, nil, invalid("model requires an unsupported default prompt")
		}
		if fallback, err = prompt(name); err != nil {
			return nil, nil, err
		}
		if fallback == nil {
			return nil, nil, bad
		}
	}
	query, err := prompt("query")
	if err != nil {
		return nil, nil, err
	}
	if query == nil {
		query = fallback
	}
	passage := (*string)(nil)
	for _, name := range []string{"document", "passage", "corpus"} {
		found, err := prompt(name)
		if err != nil {
			return nil, nil, err
		}
		if passage == nil {
			passage = found
		}
	}
	if passage == nil {
		passage = fallback
	}
	return query, passage, nil
}

// decisionDefinition pins one immutable, root-level GGUF without reading its
// header.
func decisionDefinition(ctx context.Context, hub *catalog, found *repository) ([]protocol.ModelsAsset, error) {
	selected, err := decisionSelect(found.files)
	if err != nil {
		return nil, err
	}
	asset, err := pin(ctx, hub, selected, "model.gguf")
	if err != nil {
		return nil, err
	}
	return []protocol.ModelsAsset{asset}, nil
}

func decisionSelect(files map[string]*file) (*file, error) {
	var candidates []*file
	for _, path := range slices.Sorted(maps.Keys(files)) {
		entry := files[path]
		name := strings.ToLower(entry.path)
		if !strings.Contains(name, "/") && strings.HasSuffix(name, ".gguf") && !sharded(name) && entry.bytes > 0 && entry.bytes <= MaxAssetBytes {
			candidates = append(candidates, entry)
		}
	}
	for _, quantization := range []string{"q8_0", "q4_k_m"} {
		var selected []*file
		for _, entry := range candidates {
			if strings.Contains(strings.ToLower(entry.path), quantization) {
				selected = append(selected, entry)
			}
		}
		switch len(selected) {
		case 0:
		case 1:
			return selected[0], nil
		default:
			return nil, invalid("decision model quantization is ambiguous")
		}
	}
	switch len(candidates) {
	case 0:
		return nil, invalid("repository has no supported decision model")
	case 1:
		return candidates[0], nil
	}
	return nil, invalid("decision model file is ambiguous")
}

// sharded reports a split export such as model-00001-of-00002.gguf.
func sharded(name string) bool {
	stem := strings.TrimSuffix(name, ".gguf")
	split := strings.LastIndex(stem, "-of-")
	if split < 0 {
		return false
	}
	prefix, total := stem[:split], stem[split+len("-of-"):]
	dash := strings.LastIndex(prefix, "-")
	if dash < 0 {
		return false
	}
	numeric := func(value string) bool {
		return value != "" && !strings.ContainsFunc(value, func(char rune) bool { return char < '0' || char > '9' })
	}
	return numeric(prefix[dash+1:]) && numeric(total)
}

type ocrRole struct {
	path string
	name string
}

// ocrProfile reads the repository's OCR profile, or recognizes a known Paddle
// file set, and checks every role is a bounded immutable file.
func ocrProfile(ctx context.Context, hub *catalog, found *repository) (protocol.ModelsFormat, []ocrRole, error) {
	var format protocol.ModelsFormat
	var profile struct {
		Family      protocol.ModelsPaddleFamily `json:"family"`
		Detector    *string                     `json:"detector"`
		Recognizer  *string                     `json:"recognizer"`
		Dictionary  *string                     `json:"dictionary"`
		Orientation *string                     `json:"orientation"`
	}
	if entry, present := found.files["ocr.json"]; present {
		if entry.bytes > metadataBytes {
			return format, nil, invalid("OCR profile exceeds its metadata limit")
		}
		body, _, err := small(ctx, hub, entry, "metadata.json")
		if err != nil {
			return format, nil, err
		}
		if _, err := decode(body); err != nil {
			return format, nil, invalid("model asset contains invalid JSON: " + entry.path)
		}
		decoder := json.NewDecoder(bytes.NewReader(body))
		decoder.DisallowUnknownFields()
		if decoder.Decode(&profile) != nil || profile.Family == "" || profile.Detector == nil || profile.Recognizer == nil || profile.Dictionary == nil {
			return format, nil, invalid("OCR profile is invalid")
		}
	} else {
		known := [][4]string{
			{"v6", "pp-ocrv6_tiny_det.onnx", "pp-ocrv6_tiny_rec.onnx", "ppocrv6_tiny_dict.txt"},
			{"v5", "pp-ocrv5_mobile_det.onnx", "pp-ocrv5_mobile_rec.onnx", "ppocrv5_dict.txt"},
			{"v6", "pp-ocrv6_small_det.onnx", "pp-ocrv6_small_rec.onnx", "ppocrv6_dict.txt"},
		}
		index := slices.IndexFunc(known, func(set [4]string) bool {
			return found.files[set[1]] != nil && found.files[set[2]] != nil && found.files[set[3]] != nil
		})
		if index < 0 {
			return format, nil, invalid("a supported Paddle OCR profile is required")
		}
		set := known[index]
		profile.Family, profile.Detector, profile.Recognizer, profile.Dictionary = protocol.ModelsPaddleFamily(set[0]), &set[1], &set[2], &set[3]
		if orientation := "pp-lcnet_x0_25_textline_ori.onnx"; found.files[orientation] != nil {
			profile.Orientation = &orientation
		}
	}
	roles := []ocrRole{{*profile.Detector, "detector.onnx"}, {*profile.Recognizer, "recognizer.onnx"}, {*profile.Dictionary, "dictionary.txt"}}
	if profile.Orientation != nil {
		roles = append(roles, ocrRole{*profile.Orientation, "orientation.onnx"})
	}
	for _, role := range roles {
		entry, err := required(found, role.path)
		if err != nil {
			return format, nil, err
		}
		if !ocrBounded(entry, role.name) {
			return format, nil, invalid("OCR asset requires a bounded immutable file revision")
		}
	}
	format = protocol.ModelsFormat{Kind: "paddle_ocr", Data: &protocol.ModelsFormatPaddleOcrPayload{Family: profile.Family, Orientation: profile.Orientation != nil}}
	return format, roles, nil
}

func ocrBounded(entry *file, name string) bool {
	limit := MaxAssetBytes
	if name == "dictionary.txt" {
		limit = smallBytes
	}
	return entry.bytes > 0 && entry.bytes <= limit && commit(entry.source.Revision)
}

func ocrAsset(ctx context.Context, hub *catalog, entry *file, name string) (protocol.ModelsAsset, error) {
	if name == "dictionary.txt" {
		_, asset, err := small(ctx, hub, entry, name)
		return asset, err
	}
	return pin(ctx, hub, entry, name)
}

func ocrDefinition(ctx context.Context, hub *catalog, found *repository) (protocol.ModelsFormat, []protocol.ModelsAsset, error) {
	format, roles, err := ocrProfile(ctx, hub, found)
	if err != nil {
		return format, nil, err
	}
	var assets []protocol.ModelsAsset
	for _, role := range roles {
		entry, err := required(found, role.path)
		if err != nil {
			return format, nil, err
		}
		asset, err := ocrAsset(ctx, hub, entry, role.name)
		if err != nil {
			return format, nil, err
		}
		assets = append(assets, asset)
	}
	return format, assets, nil
}

// ocrRefresh updates the same OCR profile, retaining the detector, recognizer
// and dictionary roles.
func ocrRefresh(ctx context.Context, hub *catalog, previous *protocol.ModelsDefinition) (*protocol.ModelsDefinition, error) {
	sources := []protocol.ModelsSource{previous.Source}
	for _, asset := range previous.Assets {
		if asset.Source.Hub != previous.Source.Hub {
			return nil, invalid("OCR update requires one model Hub")
		}
		sources = append(sources, asset.Source)
	}
	var names []string
	latest := map[string]*repository{}
	for _, source := range sources {
		if latest[source.Repository] != nil {
			continue
		}
		source.Revision = "HEAD"
		found, err := resolveRepository(ctx, hub, source)
		if err != nil {
			return nil, err
		}
		latest[source.Repository] = found
		names = append(names, source.Repository)
	}
	definition := *previous
	definition.Assets = nil
	for _, asset := range previous.Assets {
		entry, err := required(latest[asset.Source.Repository], asset.Path)
		if err != nil {
			return nil, err
		}
		if !ocrBounded(entry, asset.Name) {
			return nil, invalid("OCR update requires bounded immutable assets")
		}
		updated, err := ocrAsset(ctx, hub, entry, asset.Name)
		if err != nil {
			return nil, err
		}
		definition.Assets = append(definition.Assets, updated)
	}
	primary := latest[names[0]]
	definition.Source, definition.License = primary.source, primary.license
	return &definition, ValidateDefinition(&definition)
}
