package localmodels

import (
	"context"
	"crypto/sha1"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func resolveDefinition(ctx context.Context, hub *catalog, kind protocol.ModelsKind, source protocol.ModelsSource) (*protocol.ModelsDefinition, error) {
	found, err := resolveRepository(ctx, hub, source)
	if err != nil {
		return nil, err
	}
	definition := &protocol.ModelsDefinition{Source: found.source, Name: found.name, License: found.license}
	switch kind {
	case protocol.ModelsKindEmbedding:
		definition.QueryPrefix, definition.DocumentPrefix, err = readPrefixes(ctx, hub, found)
		if err != nil {
			return nil, err
		}
		prefixed := definition.QueryPrefix != nil && *definition.QueryPrefix != "" || definition.DocumentPrefix != nil && *definition.DocumentPrefix != ""
		definition.Format, definition.Assets, err = resolveEmbedding(ctx, hub, found, prefixed)
	case protocol.ModelsKindSpeech:
		definition.Format, definition.Assets, err = resolveSpeech(ctx, hub, found)
	case protocol.ModelsKindOcr:
		definition.Format, definition.Assets, err = ocrDefinition(ctx, hub, found)
	case protocol.ModelsKindDecision:
		definition.Format = protocol.ModelsFormat{Kind: "decision", Data: &protocol.ModelsFormatDecisionPayload{}}
		definition.Assets, err = decisionDefinition(ctx, hub, found)
	default:
		err = invalid("invalid model kind")
	}
	if err != nil {
		return nil, err
	}
	return definition, ValidateDefinition(definition)
}

func resolveRepository(ctx context.Context, hub *catalog, source protocol.ModelsSource) (*repository, error) {
	if err := ValidateSource(source); err != nil {
		return nil, err
	}
	if source.Revision == "HEAD" {
		if source.Hub == protocol.ModelsHubHuggingFace {
			source.Revision = "main"
		} else {
			head, err := modelscopeHead(ctx, hub, source.Repository)
			if err != nil {
				return nil, err
			}
			source = head
		}
	}
	if source.Hub == protocol.ModelsHubHuggingFace {
		return huggingfaceRepository(ctx, hub, source)
	}
	return modelscopeRepository(ctx, hub, source)
}

func resolveEmbedding(ctx context.Context, hub *catalog, found *repository, prefixed bool) (protocol.ModelsFormat, []protocol.ModelsAsset, error) {
	format, config, err := embeddingProfile(ctx, hub, found, prefixed)
	if err != nil {
		return format, nil, err
	}
	tokenizer, err := required(found, "tokenizer.json")
	if err != nil {
		return format, nil, err
	}
	_, tokens, err := small(ctx, hub, tokenizer, "tokenizer.json")
	if err != nil {
		return format, nil, err
	}
	name, weights := "model.onnx", (*file)(nil)
	if format.Kind == "static" {
		name = "model.safetensors"
		weights, err = required(found, name)
	} else {
		weights, err = graph(found, false)
	}
	if err != nil {
		return format, nil, err
	}
	model, err := pin(ctx, hub, weights, name)
	if err != nil {
		return format, nil, err
	}
	return format, []protocol.ModelsAsset{config, tokens, model}, nil
}

func embeddingProfile(ctx context.Context, hub *catalog, found *repository, prefixed bool) (protocol.ModelsFormat, protocol.ModelsAsset, error) {
	var format protocol.ModelsFormat
	config, err := required(found, "config.json")
	if err != nil {
		return format, protocol.ModelsAsset{}, err
	}
	value, asset, err := document(ctx, hub, config, "config.json")
	if err != nil {
		return format, asset, err
	}
	if kind, _ := text(value, "model_type"); kind == "model2vec" {
		if normalized, _ := flag(value, "normalize"); !normalized {
			return format, asset, invalid("static model requires normalized mean pooling")
		}
		size, err := dimension(value, "hidden_dim")
		if err != nil {
			return format, asset, err
		}
		return protocol.ModelsFormat{Kind: "static", Data: &protocol.ModelsFormatStaticPayload{Dimension: size}}, asset, nil
	}
	size, err := dimension(value, "hidden_size")
	if err != nil {
		return format, asset, err
	}
	mode, err := pooling(ctx, hub, found, value, size, prefixed)
	if err != nil {
		return format, asset, err
	}
	tokenTypes := false
	if raw, present := field(value, "type_vocab_size"); present {
		vocabulary, ok := unsigned(raw)
		if !ok {
			return format, asset, invalid("model token type configuration is invalid")
		}
		tokenTypes = vocabulary > 0
	}
	return protocol.ModelsFormat{Kind: "transformer", Data: &protocol.ModelsFormatTransformerPayload{Dimension: size, Pooling: mode, TokenTypes: tokenTypes}}, asset, nil
}

func resolveSpeech(ctx context.Context, hub *catalog, found *repository) (protocol.ModelsFormat, []protocol.ModelsAsset, error) {
	format := protocol.ModelsFormat{Kind: "sense_voice", Data: &protocol.ModelsFormatSenseVoicePayload{}}
	if err := speechProfile(ctx, hub, found); err != nil {
		return format, nil, err
	}
	// Repository metadata identifies a candidate. Installation still verifies
	// the actual single-file graph and constructs the SenseVoice recognizer.
	weights, err := graph(found, true)
	if err != nil {
		return format, nil, err
	}
	model, err := pin(ctx, hub, weights, "model.onnx")
	if err != nil {
		return format, nil, err
	}
	table, err := required(found, "tokens.txt")
	if err != nil {
		return format, nil, err
	}
	_, tokens, err := small(ctx, hub, table, "tokens.txt")
	if err != nil {
		return format, nil, err
	}
	return format, []protocol.ModelsAsset{model, tokens}, nil
}

func speechProfile(ctx context.Context, hub *catalog, found *repository) error {
	var letters strings.Builder
	for _, char := range []byte(found.source.Repository) {
		if alphanumeric(char) {
			letters.WriteByte(char)
		}
	}
	senseVoice := strings.Contains(strings.ToLower(letters.String()), "sensevoice")
	if config, present := found.files["config.json"]; present {
		value, _, err := document(ctx, hub, config, "config.json")
		if err != nil {
			return err
		}
		kind, _ := text(value, "model_type")
		senseVoice = kind == "sense_voice" || kind == "sensevoice" || kind == "SenseVoice"
	}
	if !senseVoice {
		return invalid("only SenseVoice speech models are supported")
	}
	return nil
}

// inspect derives a listing item from repository metadata. It never downloads
// tokenizer, token tables or model weights.
func inspect(ctx context.Context, hub *catalog, kind protocol.ModelsKind, origin protocol.ModelsHub, found candidate) (protocol.ModelsItem, error) {
	var none protocol.ModelsItem
	source := protocol.ModelsSource{Hub: origin, Repository: found.repository, Revision: "main"}
	var tree *repository
	var err error
	if origin == protocol.ModelsHubHuggingFace {
		tree, err = huggingfaceRepository(ctx, hub, source)
	} else {
		if source, err = modelscopeHead(ctx, hub, found.repository); err != nil {
			return none, err
		}
		tree, err = modelscopeRepository(ctx, hub, source)
	}
	if err != nil {
		return none, err
	}
	if !commit(tree.source.Revision) && kind != protocol.ModelsKindOcr {
		return none, newFault(protocol.ErrorCodeUnavailable, "model catalog could not pin the repository revision")
	}
	if kind != protocol.ModelsKindDecision {
		for _, entry := range tree.files {
			metadata := strings.HasSuffix(entry.path, "config.json") || entry.path == "modules.json" || entry.path == "config_sentence_transformers.json"
			if metadata && entry.bytes > metadataBytes {
				return none, invalid("model configuration exceeds its metadata limit")
			}
		}
	}
	var format protocol.ModelsFormat
	switch kind {
	case protocol.ModelsKindEmbedding:
		query, passage, err := readPrefixes(ctx, hub, tree)
		if err != nil {
			return none, err
		}
		prefixed := query != nil && *query != "" || passage != nil && *passage != ""
		if format, _, err = embeddingProfile(ctx, hub, tree, prefixed); err != nil {
			return none, err
		}
		tokenizer, err := required(tree, "tokenizer.json")
		if err != nil {
			return none, err
		}
		if tokenizer.bytes == 0 || tokenizer.bytes > smallBytes {
			return none, invalid("model tokenizer exceeds its size limit")
		}
		var weights *file
		if format.Kind == "static" {
			weights, err = required(tree, "model.safetensors")
		} else {
			weights, err = graph(tree, false)
		}
		if err != nil {
			return none, err
		}
		if weights.bytes == 0 || weights.bytes > MaxAssetBytes {
			return none, invalid("model asset exceeds its size limit")
		}
	case protocol.ModelsKindSpeech:
		if err := speechProfile(ctx, hub, tree); err != nil {
			return none, err
		}
		tokens, err := required(tree, "tokens.txt")
		if err != nil {
			return none, err
		}
		weights, err := graph(tree, true)
		if err != nil {
			return none, err
		}
		if tokens.bytes == 0 || tokens.bytes > smallBytes || weights.bytes == 0 || weights.bytes > MaxAssetBytes {
			return none, invalid("model asset exceeds its size limit")
		}
		format = protocol.ModelsFormat{Kind: "sense_voice", Data: &protocol.ModelsFormatSenseVoicePayload{}}
	case protocol.ModelsKindOcr:
		if format, _, err = ocrProfile(ctx, hub, tree); err != nil {
			return none, err
		}
	case protocol.ModelsKindDecision:
		if _, err := decisionSelect(tree.files); err != nil {
			return none, err
		}
		format = protocol.ModelsFormat{Kind: "decision", Data: &protocol.ModelsFormatDecisionPayload{}}
	default:
		return none, invalid("invalid model kind")
	}
	return protocol.ModelsItem{Source: tree.source, Name: found.name, Format: format}, nil
}

func pooling(ctx context.Context, hub *catalog, found *repository, config any, size uint16, prefixed bool) (protocol.ModelsPooling, error) {
	path := ""
	if _, present := found.files["1_Pooling/config.json"]; present {
		path = "1_Pooling/config.json"
	}
	if manifest, present := found.files["modules.json"]; present {
		value, _, err := document(ctx, hub, manifest, "metadata.json")
		if err != nil {
			return "", err
		}
		modules, ok := value.([]any)
		if !ok {
			return "", invalid("model module configuration is invalid")
		}
		transformer, seen, normalized := false, false, false
		for _, module := range modules {
			kind, _ := text(module, "type")
			location, located := text(module, "path")
			switch {
			case kind == "sentence_transformers.models.Transformer" && !transformer && !seen && located && location == "":
				transformer = true
			case kind == "sentence_transformers.models.Normalize" && seen && !normalized:
				normalized = true
			case kind == "sentence_transformers.models.Pooling" && transformer && !seen:
				if !located || location == "" {
					return "", invalid("model pooling path is invalid")
				}
				path = location + "/config.json"
				seen = true
			default:
				return "", invalid("model requires an unsupported embedding module")
			}
		}
		if !transformer || !seen {
			return "", invalid("model embedding module sequence is incomplete")
		}
	}
	if path != "" {
		settings, err := required(found, path)
		if err != nil {
			return "", err
		}
		value, _, err := document(ctx, hub, settings, "metadata.json")
		if err != nil {
			return "", err
		}
		mean, meanKnown := flag(value, "pooling_mode_mean_tokens")
		if raw, present := field(value, "include_prompt"); prefixed && meanKnown && mean && present && raw != true {
			return "", invalid("model requires excluding prompt tokens from pooling")
		}
		if width, ok := count(value, "word_embedding_dimension"); !ok || width != uint64(size) {
			return "", invalid("model pooling dimension differs from its configuration")
		}
		for _, key := range []string{"pooling_mode_max_tokens", "pooling_mode_mean_sqrt_len_tokens", "pooling_mode_weightedmean_tokens", "pooling_mode_lasttoken"} {
			if raw, present := field(value, key); present && raw != false {
				return "", invalid("model requires an unsupported pooling operation")
			}
		}
		first, firstKnown := flag(value, "pooling_mode_cls_token")
		switch {
		case firstKnown && meanKnown && first && !mean:
			return protocol.ModelsPoolingCls, nil
		case firstKnown && meanKnown && !first && mean:
			return protocol.ModelsPoolingMean, nil
		}
		return "", invalid("model pooling operation is ambiguous")
	}
	switch mode, _ := text(config, "classifier_pooling"); mode {
	case "cls":
		return protocol.ModelsPoolingCls, nil
	case "mean":
		return protocol.ModelsPoolingMean, nil
	}
	return "", invalid("model pooling metadata is unavailable")
}

// graph selects the single-file ONNX export in preference order.
func graph(found *repository, speech bool) (*file, error) {
	names := []string{"model.onnx", "onnx/model.onnx", "model.int8.onnx", "onnx/model.int8.onnx", "onnx/model_quantized.onnx"}
	if speech {
		names = []string{"model.int8.onnx", "onnx/model.int8.onnx", "model.onnx", "onnx/model.onnx"}
	}
	for _, name := range names {
		if entry, present := found.files[name]; present {
			return entry, nil
		}
	}
	return nil, invalid("model repository has no supported single-file ONNX graph")
}

func required(found *repository, path string) (*file, error) {
	if entry, present := found.files[path]; present {
		return entry, nil
	}
	return nil, invalid("model asset is missing: " + path)
}

func dimension(value any, key string) (uint16, error) {
	size, ok := count(value, key)
	if !ok || size < 1 || size > 4096 {
		return 0, invalid("model embedding dimension is unsupported")
	}
	return uint16(size), nil
}

// document downloads and parses one small JSON file.
func document(ctx context.Context, hub *catalog, entry *file, name string) (any, protocol.ModelsAsset, error) {
	body, asset, err := small(ctx, hub, entry, name)
	if err != nil {
		return nil, asset, err
	}
	value, err := decode(body)
	if err != nil {
		return nil, asset, invalid("model asset contains invalid JSON: " + entry.path)
	}
	return value, asset, nil
}

// pin uses the Hub's SHA-256 for a large file, or downloads a small file to
// compute one.
func pin(ctx context.Context, hub *catalog, entry *file, name string) (protocol.ModelsAsset, error) {
	if entry.sha256 != "" {
		if entry.bytes == 0 || entry.bytes > MaxAssetBytes {
			return protocol.ModelsAsset{}, invalid("model asset exceeds its size limit")
		}
		return pinnedAsset(entry, name, entry.sha256), nil
	}
	_, asset, err := small(ctx, hub, entry, name)
	return asset, err
}

// small downloads a bounded file and checks it against the repository
// metadata: its SHA-256, or its Git blob identity.
func small(ctx context.Context, hub *catalog, entry *file, name string) ([]byte, protocol.ModelsAsset, error) {
	var none protocol.ModelsAsset
	if entry.bytes == 0 || entry.bytes > smallBytes {
		return nil, none, invalid("model metadata asset exceeds its size limit: " + entry.path)
	}
	address, err := hub.file(entry)
	if err != nil {
		return nil, none, err
	}
	body, _, err := hub.get(ctx, address, smallBytes)
	if err != nil {
		return nil, none, err
	}
	changed := newFault(protocol.ErrorCodeRevisionConflict, "model file metadata differs at the selected revision: "+entry.path)
	if uint64(len(body)) != entry.bytes {
		return nil, none, changed
	}
	sum := sha256.Sum256(body)
	checksum := hex.EncodeToString(sum[:])
	if entry.sha256 != "" {
		if !strings.EqualFold(checksum, entry.sha256) {
			return nil, none, changed
		}
	} else {
		blob := sha1.New()
		fmt.Fprintf(blob, "blob %d\x00", len(body))
		blob.Write(body)
		if entry.blob == "" || !strings.EqualFold(hex.EncodeToString(blob.Sum(nil)), entry.blob) {
			return nil, none, changed
		}
	}
	return body, pinnedAsset(entry, name, checksum), nil
}

func pinnedAsset(entry *file, name, checksum string) protocol.ModelsAsset {
	return protocol.ModelsAsset{Source: entry.source, Path: entry.path, Name: name, Bytes: entry.bytes, Sha256: checksum}
}
