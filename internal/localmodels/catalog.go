package localmodels

import (
	"bytes"
	_ "embed"
	"encoding/hex"
	"encoding/json"
	"maps"
	"slices"
	"strings"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// Registration is one registered definition and its content identity.
type Registration struct {
	ID         string
	Definition protocol.ModelsDefinition
}

// Limit is the number of registered definitions allowed per kind, beside the
// pinned builtins.
const Limit = 128

//go:embed ocr.json
var pinnedOCR []byte

func encoded(definition *protocol.ModelsDefinition) []byte {
	body, err := json.Marshal(definition)
	if err != nil {
		panic("model definition is serializable: " + err.Error())
	}
	return body
}

func same(left, right *protocol.ModelsDefinition) bool {
	return left == right || left != nil && right != nil && bytes.Equal(encoded(left), encoded(right))
}

// ID is the content identity of a definition, independent of asset order.
func ID(definition *protocol.ModelsDefinition) string {
	sorted := *definition
	sorted.Assets = slices.Clone(definition.Assets)
	slices.SortStableFunc(sorted.Assets, func(left, right protocol.ModelsAsset) int { return strings.Compare(left.Name, right.Name) })
	sum := blake3.Sum256(encoded(&sorted))
	return hex.EncodeToString(sum[:])
}

type pinned struct {
	ocr         []protocol.ModelsDefinition
	definitions map[string]*protocol.ModelsDefinition
}

var builtins = sync.OnceValue(func() pinned {
	var ocr []protocol.ModelsDefinition
	if err := json.Unmarshal(pinnedOCR, &ocr); err != nil {
		panic("pinned OCR catalog is valid: " + err.Error())
	}
	definitions := map[string]*protocol.ModelsDefinition{}
	for id, definition := range embedding() {
		definitions[id] = definition
	}
	for index := range ocr {
		definitions[ID(&ocr[index])] = &ocr[index]
	}
	for _, definition := range definitions {
		if err := ValidateDefinition(definition); err != nil {
			panic("pinned local model is valid: " + err.Error())
		}
	}
	return pinned{ocr: ocr, definitions: definitions}
})

// OCR lists the pinned OCR definitions.
func OCR() []protocol.ModelsDefinition { return slices.Clone(builtins().ocr) }

// Builtin reports whether id names a pinned embedding model.
func Builtin(id string) bool {
	return id == DefaultEmbedding || id == "bge-small-zh" || id == "potion-multi"
}

// Definitions joins the pinned builtins with registered definitions.
func Definitions(records []Registration) (map[string]*protocol.ModelsDefinition, error) {
	definitions := maps.Clone(builtins().definitions)
	counts := map[protocol.ModelsKind]int{}
	for index := range records {
		record := &records[index]
		if err := ValidateDefinition(&record.Definition); err != nil {
			return nil, err
		}
		kind := Kind(record.Definition.Format)
		if !ValidID(record.ID) || kind == protocol.ModelsKindSpeech {
			return nil, invalid("invalid Node model registration")
		}
		if previous, present := definitions[record.ID]; present && !same(previous, &record.Definition) {
			return nil, invalid("model identity has conflicting definitions")
		}
		definitions[record.ID] = &record.Definition
		counts[kind]++
	}
	for _, kind := range NodeKinds {
		if counts[kind] > Limit {
			return nil, invalid("local model catalog exceeds its limit")
		}
	}
	return definitions, nil
}

type pinnedFile struct {
	name   string
	bytes  uint64
	sha256 string
	// source overrides the repository, revision and path of the model.
	source *[3]string
}

func embedding() map[string]*protocol.ModelsDefinition {
	transformer := func(dimension uint16, tokenTypes bool) protocol.ModelsFormat {
		return protocol.ModelsFormat{Kind: "transformer", Data: &protocol.ModelsFormatTransformerPayload{Dimension: dimension, Pooling: protocol.ModelsPoolingCls, TokenTypes: tokenTypes}}
	}
	models := []struct {
		id, name, repository, revision, license string
		format                                  protocol.ModelsFormat
		files                                   []pinnedFile
	}{
		{
			id: DefaultEmbedding, name: "Granite Multilingual", license: "Apache-2.0",
			repository: "ibm-granite/granite-embedding-97m-multilingual-r2", revision: "835ad14087e140460703cf0fae09f97d469d65c2",
			format: transformer(384, false),
			files: []pinnedFile{
				{name: "config.json", bytes: 1216, sha256: "de948b0bdc6f356afad7a84b276d8dd7e7fe10fb9add1bb5e610621c28e41ebc"},
				{name: "tokenizer.json", bytes: 25301672, sha256: "4f2842d568e2724370aec203652a42ac783c7937f8347a1a2cc7506d71f1582f"},
				{name: "model.onnx", bytes: 390004608, sha256: "68e592b160673d30250824c1116bc6ab33f70efb22b97c9e1d7ce1e69c1c9d70",
					source: &[3]string{"ibm-granite/granite-embedding-97m-multilingual-r2", "835ad14087e140460703cf0fae09f97d469d65c2", "onnx/model.onnx"}},
			},
		},
		{
			id: "bge-small-zh", name: "BGE Small Chinese", license: "MIT",
			repository: "BAAI/bge-small-zh-v1.5", revision: "7999e1d3359715c523056ef9478215996d62a620",
			format: transformer(512, true),
			files: []pinnedFile{
				{name: "config.json", bytes: 776, sha256: "3853a7979202c348751b753e36f579c41d8da7d36af617d3d907e1fc9b441f2a"},
				{name: "tokenizer.json", bytes: 439125, sha256: "48cea5d44424912a6fd1ea647bf4fe50b55ab8b1e5879c3275f80e339e8fae26"},
				{name: "model.onnx", bytes: 94851877, sha256: "69a0b846f4f116b5e6aabf9546ea6754d02264f3211a13a1bd69b31b8040749a",
					source: &[3]string{"Xenova/bge-small-zh-v1.5", "75c43b069aac4d136ba6bc1122f995fedcfd2781", "onnx/model.onnx"}},
			},
		},
		{
			id: "potion-multi", name: "Potion Multilingual", license: "MIT",
			repository: "minishlab/potion-multilingual-128M", revision: "73908c3438cf03b6a01bcb9611d62b23d0726f08",
			format: protocol.ModelsFormat{Kind: "static", Data: &protocol.ModelsFormatStaticPayload{Dimension: 256}},
			files: []pinnedFile{
				{name: "config.json", bytes: 271, sha256: "595e4cab2093732efd5dbe084fd5c1826b5eea693b73b4c1fd971672867d2e54"},
				{name: "tokenizer.json", bytes: 18616131, sha256: "19f1909063da3cfe3bd83a782381f040dccea475f4816de11116444a73e1b6a1"},
				{name: "model.safetensors", bytes: 512361560, sha256: "14b5eb39cb4ce5666da8ad1f3dc6be4346e9b2d601c073302fa0a31bf7943397"},
			},
		},
	}
	definitions := map[string]*protocol.ModelsDefinition{}
	for _, model := range models {
		source := protocol.ModelsSource{Hub: protocol.ModelsHubHuggingFace, Repository: model.repository, Revision: model.revision}
		definition := &protocol.ModelsDefinition{Source: source, Name: model.name, License: model.license, Format: model.format}
		for _, file := range model.files {
			asset := protocol.ModelsAsset{Source: source, Path: file.name, Name: file.name, Bytes: file.bytes, Sha256: file.sha256}
			if file.source != nil {
				asset.Source.Repository, asset.Source.Revision, asset.Path = file.source[0], file.source[1], file.source[2]
			}
			definition.Assets = append(definition.Assets, asset)
		}
		definitions[model.id] = definition
	}
	return definitions
}
