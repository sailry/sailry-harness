// Package embedding turns text into normalized vectors with a transformer
// model on the CPU. Model files are acquired and verified by the model
// registry before an engine loads them.
package embedding

import (
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"os"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/localmodels/embedding/tokenizer"
	"github.com/sailry/sailry-harness/internal/localmodels/onnx"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// MaxInputTokens is the model window, special tokens and prefix included.
const MaxInputTokens = 512

// ErrTooLong reports an input that exceeds the model window. Inputs are
// never truncated.
var ErrTooLong = fmt.Errorf("embedding input exceeds the %d-token window", MaxInputTokens)

// Input selects the prefix a definition prescribes for a text.
type Input uint8

const (
	Query Input = iota
	Document
)

// Engine is one loaded embedding model. It is not safe for concurrent use.
type Engine struct {
	dimension      int
	tokenizer      *tokenizer.Tokenizer
	session        *onnx.Session
	types          bool
	pooling        protocol.ModelsPooling
	queryPrefix    string
	documentPrefix string
}

func invalid(message string) error { return errors.New("embedding model is invalid: " + message) }

// Load opens the model of a registered definition from its verified files.
func Load(definition *protocol.ModelsDefinition, directory string) (*Engine, error) {
	var format protocol.ModelsFormatTransformerPayload
	switch data := definition.Format.Data.(type) {
	case protocol.ModelsFormatTransformerPayload:
		format = data
	case *protocol.ModelsFormatTransformerPayload:
		format = *data
	default:
		// Static tables and the tokenizers they need are not ported.
		return nil, invalid("model format is not a supported embedding")
	}
	tokens, err := tokenizer.Load(filepath.Join(directory, "tokenizer.json"))
	if err != nil {
		return nil, fmt.Errorf("embedding tokenizer failed: %w", err)
	}
	config, err := os.ReadFile(filepath.Join(directory, "config.json"))
	if err != nil {
		return nil, fmt.Errorf("embedding file could not be read: %w", err)
	}
	var object map[string]json.RawMessage
	if err := json.Unmarshal(config, &object); err != nil || object == nil {
		return nil, invalid("model configuration must be an object")
	}
	model := filepath.Join(directory, "model.onnx")
	if err := onnx.Validate(model); err != nil {
		return nil, fmt.Errorf("embedding file could not be read: %w", err)
	}
	if err := onnx.Initialize(); err != nil {
		return nil, fmt.Errorf("embedding runtime could not load: %w", err)
	}
	session, err := onnx.NewSession(model)
	if err != nil {
		return nil, fmt.Errorf("embedding inference failed: %w", err)
	}
	engine := &Engine{
		dimension: int(format.Dimension), tokenizer: tokens, session: session,
		types: format.TokenTypes, pooling: format.Pooling,
	}
	if !engine.compatible() {
		session.Close()
		return nil, invalid("ONNX model interface is invalid")
	}
	if definition.DocumentPrefix != nil {
		prefix, err := tokens.Encode(*definition.DocumentPrefix, true)
		if err != nil {
			session.Close()
			return nil, fmt.Errorf("embedding tokenizer failed: %w", err)
		}
		if len(prefix.IDs) > MaxInputTokens/2 {
			session.Close()
			return nil, invalid("document prefix leaves fewer than 256 content tokens")
		}
		engine.documentPrefix = *definition.DocumentPrefix
	}
	if definition.QueryPrefix != nil {
		engine.queryPrefix = *definition.QueryPrefix
	}
	return engine, nil
}

func (engine *Engine) names() []string {
	if engine.types {
		return []string{"input_ids", "attention_mask", "token_type_ids"}
	}
	return []string{"input_ids", "attention_mask"}
}

// compatible checks the model interface: int64 [batch, tokens] inputs and
// one float32 [batch, tokens, dimension] output.
func (engine *Engine) compatible() bool {
	batch := func(size int64) bool { return size == -1 || size == 1 }
	inputs, outputs := engine.session.Inputs, engine.session.Outputs
	names := engine.names()
	if len(inputs) != len(names) || len(outputs) != 1 {
		return false
	}
	for _, name := range names {
		found := false
		for _, input := range inputs {
			found = found || input.Name == name && input.Element == onnx.ElementInt64 &&
				len(input.Shape) == 2 && batch(input.Shape[0]) && input.Shape[1] == -1
		}
		if !found {
			return false
		}
	}
	shape := outputs[0].Shape
	return outputs[0].Element == onnx.ElementFloat32 && len(shape) == 3 && batch(shape[0]) && shape[1] == -1 &&
		(shape[2] == -1 || shape[2] == int64(engine.dimension))
}

// Dimension is the length of every vector.
func (engine *Engine) Dimension() int { return engine.dimension }

// Offsets returns the byte range of every token of text, without special
// tokens or truncation.
func (engine *Engine) Offsets(text string) ([][2]int, error) {
	encoding, err := engine.tokenizer.Encode(text, false)
	if err != nil {
		return nil, fmt.Errorf("embedding tokenizer failed: %w", err)
	}
	return encoding.Offsets, nil
}

// Encode returns one unit-length vector per text. An output of zeros stays
// zero.
func (engine *Engine) Encode(input Input, texts []string) ([][]float32, error) {
	prefix := engine.documentPrefix
	if input == Query {
		prefix = engine.queryPrefix
	}
	vectors := make([][]float32, 0, len(texts))
	for _, text := range texts {
		encoding, err := engine.tokenizer.Encode(prefix+text, true)
		if err != nil {
			return nil, fmt.Errorf("embedding tokenizer failed: %w", err)
		}
		if len(encoding.IDs) > MaxInputTokens {
			return nil, ErrTooLong
		}
		vector, err := engine.run(encoding.IDs)
		if err != nil {
			return nil, err
		}
		if err := normalize(vector); err != nil {
			return nil, err
		}
		vectors = append(vectors, vector)
	}
	return vectors, nil
}

// run evaluates one unpadded sequence and pools its token states.
func (engine *Engine) run(tokens []int) ([]float32, error) {
	mismatch := invalid("ONNX output shape does not match its input")
	if len(tokens) == 0 {
		return nil, mismatch
	}
	ids := make([]int64, len(tokens))
	mask := make([]int64, len(tokens))
	for index, token := range tokens {
		ids[index], mask[index] = int64(token), 1
	}
	shape := []int64{1, int64(len(tokens))}
	inputs := []onnx.Tensor{
		{Name: "input_ids", Shape: shape, Int64: ids},
		{Name: "attention_mask", Shape: shape, Int64: mask},
	}
	if engine.types {
		// A single sequence is segment zero throughout.
		inputs = append(inputs, onnx.Tensor{Name: "token_type_ids", Shape: shape, Int64: make([]int64, len(tokens))})
	}
	outputs, err := engine.session.Run(inputs)
	if err != nil {
		return nil, fmt.Errorf("embedding inference failed: %w", err)
	}
	output := outputs[0]
	if len(output.Shape) != 3 || output.Shape[0] != 1 || output.Shape[1] != int64(len(tokens)) ||
		output.Shape[2] != int64(engine.dimension) || len(output.Float32) != len(tokens)*engine.dimension {
		return nil, mismatch
	}
	return pool(output.Float32, engine.dimension, engine.pooling), nil
}

// pool reduces the token states of one unpadded sequence to a vector.
func pool(values []float32, dimension int, pooling protocol.ModelsPooling) []float32 {
	vector := make([]float32, dimension)
	if pooling != protocol.ModelsPoolingMean {
		copy(vector, values[:dimension])
		return vector
	}
	sum := make([]float64, dimension)
	for index, value := range values {
		sum[index%dimension] += float64(value)
	}
	tokens := float64(len(values) / dimension)
	for index := range vector {
		vector[index] = float32(sum[index] / tokens)
	}
	return vector
}

// normalize scales a vector to unit length in double precision.
func normalize(vector []float32) error {
	var squares float64
	for _, value := range vector {
		if math.IsNaN(float64(value)) || math.IsInf(float64(value), 0) {
			return invalid("output contains a nonfinite value")
		}
		squares += float64(value) * float64(value)
	}
	if norm := math.Sqrt(squares); norm > 0 {
		for index, value := range vector {
			vector[index] = float32(float64(value) / norm)
		}
	}
	return nil
}

// Close releases the model.
func (engine *Engine) Close() { engine.session.Close() }
