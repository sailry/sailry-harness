package knowledge

import (
	"encoding/binary"
	"math"
)

// Vector is a compact derived embedding: signed bytes under one scale.
type Vector struct {
	values []int8
	scale  float32
	norm   float32
}

func finite(value float32) bool {
	return !math.IsNaN(float64(value)) && !math.IsInf(float64(value), 0)
}

func length(values []int8) float32 {
	var sum float32
	for _, value := range values {
		sum += float32(value) * float32(value)
	}
	return float32(math.Sqrt(float64(sum)))
}

// Quantize keeps the direction of an embedding in one byte per component.
func Quantize(values []float32) (Vector, error) {
	if len(values) == 0 {
		return Vector{}, invalid("invalid knowledge embedding")
	}
	var peak float32
	for _, value := range values {
		if !finite(value) {
			return Vector{}, invalid("invalid knowledge embedding")
		}
		peak = max(peak, float32(math.Abs(float64(value))))
	}
	if peak == 0 {
		return Vector{}, invalid("knowledge embedding has no direction")
	}
	scale := peak / 127
	if scale == 0 {
		return Vector{}, invalid("knowledge embedding is too small to quantize")
	}
	quantized := make([]int8, len(values))
	for index, value := range values {
		// Rust rounds half away from zero, as math.Round does.
		quantized[index] = int8(min(max(math.Round(float64(value/scale)), -127), 127))
	}
	return Vector{values: quantized, scale: scale, norm: length(quantized)}, nil
}

// Encode stores the scale followed by one byte per component.
func (vector Vector) Encode() []byte {
	bytes := make([]byte, 4, 4+len(vector.values))
	binary.LittleEndian.PutUint32(bytes, math.Float32bits(vector.scale))
	for _, value := range vector.values {
		bytes = append(bytes, byte(value))
	}
	return bytes
}

// DecodeVector reads a stored vector of the collection's dimension.
func DecodeVector(bytes []byte, dimension int) (Vector, error) {
	values, norm, err := AppendVector(make([]int8, 0, max(dimension, 0)), bytes, dimension)
	if err != nil {
		return Vector{}, err
	}
	return Vector{values: values, scale: math.Float32frombits(binary.LittleEndian.Uint32(bytes)), norm: norm}, nil
}

// AppendVector checks a stored vector as DecodeVector does and appends its
// components to values, so that many vectors share one allocation. It
// returns the norm of the appended components.
func AppendVector(values []int8, bytes []byte, dimension int) ([]int8, float32, error) {
	if dimension == 0 || len(bytes) != dimension+4 {
		return values, 0, invalid("invalid knowledge vector dimension")
	}
	scale := math.Float32frombits(binary.LittleEndian.Uint32(bytes))
	if !finite(scale) || scale <= 0 {
		return values, 0, invalid("invalid knowledge vector scale")
	}
	start := len(values)
	for _, value := range bytes[4:] {
		if int8(value) == math.MinInt8 {
			return values[:start], 0, invalid("invalid knowledge vector value")
		}
		values = append(values, int8(value))
	}
	norm := length(values[start:])
	if norm == 0 {
		return values[:start], 0, invalid("knowledge vector has no direction")
	}
	return values, norm, nil
}

// Direction is a query embedding checked once for many comparisons.
type Direction struct {
	values []float32
	norm   float32
}

// NewDirection checks a query embedding as Vector.Cosine does.
func NewDirection(values []float32) (Direction, error) {
	var sum float32
	for _, value := range values {
		if !finite(value) {
			return Direction{}, invalid("knowledge query dimension differs from its index")
		}
		sum += value * value
	}
	norm := float32(math.Sqrt(float64(sum)))
	if norm == 0 || !finite(norm) {
		return Direction{}, invalid("knowledge query embedding has no direction")
	}
	return Direction{values: values, norm: norm}, nil
}

// Cosine compares the query with the components and norm of one stored
// vector of its dimension. It is the arithmetic of Vector.Cosine, in the same
// order, so both give the same bits.
func (query Direction) Cosine(values []int8, norm float32) float32 {
	values = values[:len(query.values)]
	var dot float32
	for index, value := range query.values {
		dot += float32(values[index]) * value
	}
	return min(max(dot/(norm*query.norm), -1), 1)
}

// Cosine compares the stored direction with a query embedding.
func (vector Vector) Cosine(query []float32) (float32, error) {
	if len(query) != len(vector.values) {
		return 0, invalid("knowledge query dimension differs from its index")
	}
	var sum, dot float32
	for index, value := range query {
		if !finite(value) {
			return 0, invalid("knowledge query dimension differs from its index")
		}
		sum += value * value
		dot += float32(vector.values[index]) * value
	}
	norm := float32(math.Sqrt(float64(sum)))
	if norm == 0 || !finite(norm) {
		return 0, invalid("knowledge query embedding has no direction")
	}
	return min(max(dot/(vector.norm*norm), -1), 1), nil
}
