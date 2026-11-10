// Package localmodels owns the models one Node manages: anonymous Hub
// discovery, immutable pinned definitions, verified installations and the
// scheduler that serializes local inference.
package localmodels

import (
	"fmt"
	"math"
	"net/url"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/localmodels/assets"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	// DefaultEmbedding is the embedding model selected before any choice is saved.
	DefaultEmbedding = "granite-multi"
	// MaxAssets bounds the files of one definition.
	MaxAssets = 8
	// MaxAssetBytes bounds one model file.
	MaxAssetBytes uint64 = 2 * 1024 * 1024 * 1024
	// MaxPrefixBytes bounds one embedding input prefix.
	MaxPrefixBytes = 1024
)

// NodeKinds are the model kinds a Node manages. Speech belongs to the controller.
var NodeKinds = [...]protocol.ModelsKind{protocol.ModelsKindEmbedding, protocol.ModelsKindOcr, protocol.ModelsKindDecision}

func newFault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}

func invalid(message string) error { return newFault(protocol.ErrorCodeInvalidRequest, message) }

// code reports the fault code of an error produced by this package.
func code(err error) protocol.ErrorCode {
	if fault, ok := err.(*protocol.Fault); ok {
		return fault.Code
	}
	return ""
}

// Failure reports a local runtime or storage failure.
func Failure(reason any) error {
	return newFault(protocol.ErrorCodeUnavailable, fmt.Sprintf("local model failed: %v", reason))
}

// Cancelled reports a cancelled local model operation.
func Cancelled() error {
	return newFault(protocol.ErrorCodeCancelled, "local model operation was cancelled")
}

func missing() error {
	return newFault(protocol.ErrorCodeNotConfigured, "local model is not installed")
}

func conflict() error {
	return newFault(protocol.ErrorCodeRevisionConflict, "local model configuration or assets changed")
}

func speech() error { return invalid("speech belongs to the controller") }

// ValidID reports whether id can name a model and its directory.
func ValidID(id string) bool {
	if id == "" || len(id) > 128 {
		return false
	}
	for _, char := range []byte(id) {
		if !(char >= 'a' && char <= 'z' || char >= '0' && char <= '9' || char == '-') {
			return false
		}
	}
	return true
}

// payload reads a format variant decoded as a pointer or built as a value.
func payload[T any](format protocol.ModelsFormat) (T, bool) {
	switch data := format.Data.(type) {
	case T:
		return data, true
	case *T:
		if data != nil {
			return *data, true
		}
	}
	var zero T
	return zero, false
}

// Kind is the model kind a format belongs to.
func Kind(format protocol.ModelsFormat) protocol.ModelsKind {
	switch format.Kind {
	case "transformer", "static":
		return protocol.ModelsKindEmbedding
	case "sense_voice":
		return protocol.ModelsKindSpeech
	case "decision":
		return protocol.ModelsKindDecision
	case "paddle_ocr":
		return protocol.ModelsKindOcr
	}
	return ""
}

// Dimension is the vector size of an embedding format.
func Dimension(format protocol.ModelsFormat) (uint16, bool) {
	if data, ok := payload[protocol.ModelsFormatTransformerPayload](format); ok && format.Kind == "transformer" {
		return data.Dimension, true
	}
	if data, ok := payload[protocol.ModelsFormatStaticPayload](format); ok && format.Kind == "static" {
		return data.Dimension, true
	}
	return 0, false
}

func alphanumeric(char byte) bool {
	return char >= 'a' && char <= 'z' || char >= 'A' && char <= 'Z' || char >= '0' && char <= '9'
}

func hexadecimal(value string, length int) bool {
	if len(value) != length {
		return false
	}
	for _, char := range []byte(value) {
		if !(char >= '0' && char <= '9' || char >= 'a' && char <= 'f' || char >= 'A' && char <= 'F') {
			return false
		}
	}
	return true
}

func commit(value string) bool { return hexadecimal(value, 40) }

func digest(value string) bool { return hexadecimal(value, 64) }

// ValidateSource checks a repository and a branch, tag or commit name.
func ValidateSource(source protocol.ModelsSource) error {
	if !assets.ValidSource(source) {
		return invalid("invalid model repository or revision")
	}
	return nil
}

// ValidateDefinition checks a pinned definition and its format's file set.
func ValidateDefinition(definition *protocol.ModelsDefinition) error {
	if err := ValidateSource(definition.Source); err != nil {
		return err
	}
	kind := Kind(definition.Format)
	bad := strings.TrimSpace(definition.Name) == "" || len(definition.Name) > 256 || strings.Contains(definition.Name, "\x00") ||
		len(definition.License) > 256 || strings.Contains(definition.License, "\x00") ||
		kind != protocol.ModelsKindEmbedding && (definition.QueryPrefix != nil || definition.DocumentPrefix != nil) ||
		len(definition.Assets) == 0 || len(definition.Assets) > MaxAssets
	for _, prefix := range []*string{definition.QueryPrefix, definition.DocumentPrefix} {
		bad = bad || prefix != nil && (len(*prefix) > MaxPrefixBytes || strings.Contains(*prefix, "\x00"))
	}
	if size, embedding := Dimension(definition.Format); embedding && (size < 1 || size > 4096) {
		bad = true
	}
	var expected []string
	switch definition.Format.Kind {
	case "transformer":
		_, ok := payload[protocol.ModelsFormatTransformerPayload](definition.Format)
		bad = bad || !ok
		expected = []string{"config.json", "tokenizer.json", "model.onnx"}
	case "static":
		_, ok := payload[protocol.ModelsFormatStaticPayload](definition.Format)
		bad = bad || !ok
		expected = []string{"config.json", "tokenizer.json", "model.safetensors"}
	case "sense_voice":
		expected = []string{"model.onnx", "tokens.txt"}
	case "decision":
		expected = []string{"model.gguf"}
	case "paddle_ocr":
		data, ok := payload[protocol.ModelsFormatPaddleOcrPayload](definition.Format)
		bad = bad || !ok
		expected = []string{"detector.onnx", "recognizer.onnx", "dictionary.txt"}
		if data.Orientation {
			expected = append(expected, "orientation.onnx")
		}
	default:
		bad = true
	}
	if bad {
		return invalid("invalid model definition")
	}
	names := map[string]bool{}
	for _, asset := range definition.Assets {
		if err := ValidateSource(asset.Source); err != nil {
			return err
		}
		path := len(asset.Path) <= 1024
		for part := range strings.SplitSeq(asset.Path, "/") {
			path = path && part != "" && part != "." && part != ".."
			for _, char := range []byte(part) {
				path = path && (alphanumeric(char) || char == '-' || char == '_' || char == '.')
			}
		}
		if !commit(asset.Source.Revision) || !path || !slices.Contains(expected, asset.Name) || names[asset.Name] ||
			asset.Bytes == 0 || asset.Bytes > MaxAssetBytes || !digest(asset.Sha256) {
			return invalid("invalid pinned model asset")
		}
		names[asset.Name] = true
	}
	if len(names) != len(expected) {
		return invalid("model assets do not match its format")
	}
	return nil
}

// ValidateSearch bounds a Hub search request.
func ValidateSearch(search *protocol.ModelsSearch) error {
	if len(search.Query) > 512 || strings.Contains(search.Query, "\x00") ||
		search.Cursor != nil && (len(*search.Cursor) > 8192 || strings.Contains(*search.Cursor, "\x00")) {
		return invalid("invalid model search")
	}
	return nil
}

func initial(kind protocol.ModelsKind) protocol.ModelsLocalChoice {
	choice := protocol.ModelsLocalChoice{Enabled: true}
	if kind == protocol.ModelsKindEmbedding {
		model := DefaultEmbedding
		choice.Model = &model
	}
	return choice
}

// FillDefaults adds the initial choice of every Node kind that has none.
func FillDefaults(settings *protocol.ModelsLocalSettings) {
	if settings.Choices == nil {
		settings.Choices = map[protocol.ModelsKind]protocol.ModelsLocalChoice{}
	}
	for _, kind := range NodeKinds {
		if _, present := settings.Choices[kind]; !present {
			settings.Choices[kind] = initial(kind)
		}
	}
}

// Choice is the saved or initial choice of one kind.
func Choice(settings *protocol.ModelsLocalSettings, kind protocol.ModelsKind) protocol.ModelsLocalChoice {
	if choice, present := settings.Choices[kind]; present {
		return choice
	}
	return initial(kind)
}

// Selected is the enabled model of one kind, or "" when none is selected.
func Selected(settings *protocol.ModelsLocalSettings, kind protocol.ModelsKind) string {
	choice := Choice(settings, kind)
	if !choice.Enabled || choice.Model == nil {
		return ""
	}
	return *choice.Model
}

func sameChoice(left, right protocol.ModelsLocalChoice) bool {
	return left.Enabled == right.Enabled && (left.Model == nil) == (right.Model == nil) && (left.Model == nil || *left.Model == *right.Model)
}

// origin parses an HTTPS base address without credentials, query or fragment.
func origin(value string) (*url.URL, bool) {
	address, err := url.Parse(value)
	return address, err == nil && assets.Origin(address)
}

// ValidateSettings checks the choices and the optional Hugging Face mirror.
func ValidateSettings(settings *protocol.ModelsLocalSettings) error {
	bad := settings.Revision > math.MaxInt64
	for kind, choice := range settings.Choices {
		bad = bad || !slices.Contains(NodeKinds[:], kind) || choice.Model != nil && !ValidID(*choice.Model)
	}
	if bad {
		return invalid("invalid local model settings")
	}
	if settings.Mirror != nil {
		if _, err := url.Parse(*settings.Mirror); err != nil {
			return invalid("invalid model mirror")
		}
		if _, ok := origin(*settings.Mirror); !ok || len(*settings.Mirror) > 4096 {
			return invalid("model mirror requires an HTTPS URL without credentials")
		}
	}
	return nil
}
