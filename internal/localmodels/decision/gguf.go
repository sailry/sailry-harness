package decision

import (
	"bufio"
	"encoding/binary"
	"io"
	"math"
	"os"
	"slices"
	"strconv"
	"strings"
	"unicode/utf8"
)

// Bounds of GGUF v3 validation before a file reaches native code.
const (
	maxEntries  = 262_144
	maxMetadata = 64 * 1024 * 1024
	maxString   = 16 * 1024 * 1024
	maxArray    = 1_048_576
	// maxFile is the protocol bound of one model asset.
	maxFile = 2 * 1024 * 1024 * 1024
)

// Algorithm is the decision layout a model was trained for.
type Algorithm uint8

const (
	// Laya reads one head column at each option marker of an encoder.
	Laya Algorithm = iota
	// OpenJev reads the logits of single-letter option labels.
	OpenJev
)

// Metadata is the decision description of a validated GGUF file.
type Metadata struct {
	Architecture string
	Algorithm    Algorithm
	Context      int

	template     string
	head         int
	temperatures map[string]float32
	bias         map[int][]float32
}

type reader struct {
	file     *bufio.Reader
	length   uint64
	position uint64
}

func (r *reader) bytes(count uint64) ([]byte, error) {
	if count > r.length-r.position {
		return nil, invalid("truncated GGUF data")
	}
	data := make([]byte, count)
	if _, err := io.ReadFull(r.file, data); err != nil {
		return nil, failed(err)
	}
	r.position += count
	return data, nil
}

func (r *reader) skip(count uint64) error {
	if count > r.length-r.position {
		return invalid("truncated GGUF data")
	}
	if _, err := r.file.Discard(int(count)); err != nil {
		return failed(err)
	}
	r.position += count
	return nil
}

func (r *reader) scalar(size uint64) (uint64, error) {
	data, err := r.bytes(size)
	if err != nil {
		return 0, err
	}
	var padded [8]byte
	copy(padded[:], data)
	return binary.LittleEndian.Uint64(padded[:]), nil
}

func (r *reader) u32() (uint32, error) {
	value, err := r.scalar(4)
	return uint32(value), err
}

func (r *reader) u64() (uint64, error) { return r.scalar(8) }

func (r *reader) text(keep bool, limit uint64) (string, error) {
	length, err := r.u64()
	if err != nil {
		return "", err
	}
	if length > limit {
		return "", invalid("GGUF string exceeds its limit")
	}
	if !keep {
		return "", r.skip(length)
	}
	if length > r.length-r.position {
		return "", invalid("truncated GGUF string")
	}
	data, err := r.bytes(length)
	if err != nil {
		return "", err
	}
	if !utf8.Valid(data) {
		return "", invalid("GGUF metadata is not UTF-8")
	}
	return string(data), nil
}

// value returns a uint64, int64, float64 or string, or nil for values the
// decision metadata never reads.
func (r *reader) value(kind uint32, keep bool) (any, error) {
	switch kind {
	case 0:
		return r.scalar(1)
	case 2:
		return r.scalar(2)
	case 4:
		return r.scalar(4)
	case 10:
		return r.scalar(8)
	case 1:
		value, err := r.scalar(1)
		return int64(int8(value)), err
	case 3:
		value, err := r.scalar(2)
		return int64(int16(value)), err
	case 5:
		value, err := r.scalar(4)
		return int64(int32(value)), err
	case 11:
		value, err := r.scalar(8)
		return int64(value), err
	case 6:
		value, err := r.scalar(4)
		return float64(math.Float32frombits(uint32(value))), err
	case 12:
		value, err := r.scalar(8)
		return math.Float64frombits(value), err
	case 7:
		value, err := r.scalar(1)
		if err == nil && value > 1 {
			err = invalid("invalid GGUF boolean")
		}
		return nil, err
	case 8:
		value, err := r.text(keep, maxString)
		if err != nil || !keep {
			return nil, err
		}
		return value, nil
	case 9:
		element, err := r.u32()
		if err != nil {
			return nil, err
		}
		count, err := r.u64()
		if err != nil {
			return nil, err
		}
		if count > maxArray || element == 9 || element > 12 {
			return nil, invalid("invalid GGUF array")
		}
		for range count {
			if _, err := r.value(element, false); err != nil {
				return nil, err
			}
			if r.position > maxMetadata {
				return nil, invalid("GGUF metadata exceeds its limit")
			}
		}
		return nil, nil
	}
	return nil, invalid("unknown GGUF metadata type")
}

func unsigned(value any) (uint64, bool) {
	switch value := value.(type) {
	case uint64:
		return value, true
	case int64:
		return uint64(value), value >= 0
	}
	return 0, false
}

func float(value any) (float32, bool) {
	switch value := value.(type) {
	case float64:
		return float32(value), true
	case int64:
		return float32(value), true
	case uint64:
		return float32(value), true
	}
	return 0, false
}

func finite(value float32) bool {
	return !math.IsNaN(float64(value)) && !math.IsInf(float64(value), 0)
}

// Validate checks one ordinary, unsplit GGUF v3 model without initializing
// the native runtime.
func Validate(path string) (*Metadata, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return nil, failed(err)
	}
	if !info.Mode().IsRegular() || info.Size() > maxFile {
		return nil, invalid("GGUF must be a bounded ordinary file")
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, failed(err)
	}
	defer file.Close()
	r := &reader{file: bufio.NewReaderSize(file, 64*1024), length: uint64(info.Size())}
	magic, err := r.bytes(4)
	if err != nil {
		return nil, err
	}
	version, err := r.u32()
	if err != nil {
		return nil, err
	}
	if string(magic) != "GGUF" || version != 3 {
		return nil, invalid("GGUF version must be 3")
	}
	tensors, err := r.u64()
	if err != nil {
		return nil, err
	}
	count, err := r.u64()
	if err != nil {
		return nil, err
	}
	if tensors == 0 || tensors > maxEntries || count > maxEntries {
		return nil, invalid("GGUF entry count exceeds its limit")
	}
	values := map[string]any{}
	keys := map[string]bool{}
	for range count {
		key, err := r.text(true, 4096)
		if err != nil {
			return nil, err
		}
		if key == "" || keys[key] {
			return nil, invalid("invalid or duplicate GGUF key")
		}
		keys[key] = true
		kind, err := r.u32()
		if err != nil {
			return nil, err
		}
		keep := key == "general.architecture" || key == "general.alignment" || key == "split.count" ||
			key == "tokenizer.chat_template.systemone" || strings.HasSuffix(key, ".context_length") ||
			strings.Contains(key, ".decision.")
		value, err := r.value(kind, keep)
		if err != nil {
			return nil, err
		}
		if keep {
			values[key] = value
		}
		if r.position > maxMetadata {
			return nil, invalid("GGUF metadata exceeds its limit")
		}
	}
	alignment := uint64(32)
	if value, present := values["general.alignment"]; present {
		var valid bool
		if alignment, valid = unsigned(value); !valid {
			return nil, invalid("invalid GGUF alignment")
		}
	}
	if alignment == 0 || alignment&(alignment-1) != 0 || alignment > 4096 {
		return nil, invalid("invalid GGUF alignment")
	}
	if value, present := values["split.count"]; present {
		if split, valid := unsigned(value); !valid || split != 1 {
			return nil, invalid("split GGUF models are unsupported")
		}
	}
	ranges := make([][2]uint64, 0, tensors)
	names := map[string]bool{}
	for range tensors {
		name, err := r.text(true, 4096)
		if err != nil {
			return nil, err
		}
		if name == "" || names[name] {
			return nil, invalid("invalid or duplicate GGUF tensor")
		}
		names[name] = true
		dimensions, err := r.u32()
		if err != nil {
			return nil, err
		}
		if dimensions < 1 || dimensions > 4 {
			return nil, invalid("invalid GGUF tensor dimensions")
		}
		elements, row := uint64(1), uint64(0)
		for index := range dimensions {
			dimension, err := r.u64()
			if err != nil {
				return nil, err
			}
			if dimension == 0 || dimension > math.MaxInt64 {
				return nil, invalid("invalid GGUF tensor dimension")
			}
			if index == 0 {
				row = dimension
			}
			if elements > math.MaxUint64/dimension {
				return nil, invalid("GGUF tensor dimensions overflow")
			}
			elements *= dimension
		}
		kind, err := r.u32()
		if err != nil {
			return nil, err
		}
		layout, supported := tensorLayouts[kind]
		if !supported {
			return nil, invalid("unsupported GGUF tensor type")
		}
		if row%layout[0] != 0 {
			return nil, invalid("GGUF tensor row is not block aligned")
		}
		blocks := elements / layout[0]
		if blocks > math.MaxUint64/layout[1] {
			return nil, invalid("GGUF tensor size overflows")
		}
		offset, err := r.u64()
		if err != nil {
			return nil, err
		}
		if offset%alignment != 0 {
			return nil, invalid("GGUF tensor offset is not aligned")
		}
		end := offset + blocks*layout[1]
		if end < offset {
			return nil, invalid("GGUF tensor range overflows")
		}
		ranges = append(ranges, [2]uint64{offset, end})
		if r.position > maxMetadata {
			return nil, invalid("GGUF tensor table exceeds its limit")
		}
	}
	// The metadata bound keeps this sum far from overflow.
	start := (r.position + alignment - 1) &^ (alignment - 1)
	if start > r.length {
		return nil, invalid("truncated GGUF tensor data")
	}
	slices.SortFunc(ranges, func(left, right [2]uint64) int {
		return slices.Compare(left[:], right[:])
	})
	previous := uint64(0)
	for _, span := range ranges {
		if span[0] < previous || span[1] > r.length-start {
			return nil, invalid("invalid GGUF tensor range")
		}
		previous = span[1]
	}
	return describe(values)
}

func describe(values map[string]any) (*Metadata, error) {
	architecture, _ := values["general.architecture"].(string)
	valid := architecture != "" && len(architecture) <= 128
	for _, char := range []byte(architecture) {
		valid = valid && (char == '-' || char >= '0' && char <= '9' || char >= 'a' && char <= 'z' || char >= 'A' && char <= 'Z')
	}
	if !valid {
		return nil, invalid("GGUF architecture is missing")
	}
	prefix := architecture + ".decision."
	metadata := &Metadata{Architecture: architecture, temperatures: map[string]float32{}, bias: map[int][]float32{}}
	switch kind, _ := values[prefix+"type"].(string); kind {
	case "laya":
		metadata.Algorithm = Laya
	case "openjev":
		metadata.Algorithm = OpenJev
	default:
		return nil, invalid("unsupported GGUF decision type")
	}
	context, present := unsigned(values[architecture+".context_length"])
	if !present || context == 0 || context > 1_048_576 {
		return nil, invalid("GGUF context length is missing")
	}
	metadata.Context = int(context)
	metadata.template, _ = values["tokenizer.chat_template.systemone"].(string)
	if metadata.template == "" || len(metadata.template) > 128*1024 || strings.ContainsRune(metadata.template, 0) {
		return nil, invalid("GGUF systemone template is missing")
	}
	head, _ := unsigned(values[prefix+"max_head_tokens"])
	if metadata.Algorithm == Laya && (head == 0 || head > context) {
		return nil, invalid("GGUF decision head limit is invalid")
	}
	metadata.head = int(head)
	for key, value := range values {
		if name, found := strings.CutPrefix(key, prefix+"temperature."); found {
			temperature, numeric := float(value)
			if !numeric || !finite(temperature) || temperature <= 0 {
				return nil, invalid("invalid GGUF decision temperature")
			}
			metadata.temperatures[name] = temperature
		} else if name, found := strings.CutPrefix(key, prefix+"score_bias."); found {
			left, right, _ := strings.Cut(name, ".")
			levels, errLevels := strconv.ParseUint(left, 10, 31)
			index, errIndex := strconv.ParseUint(right, 10, 31)
			bias, numeric := float(value)
			if errLevels != nil || errIndex != nil || levels < 2 || levels > 255 || index >= levels || !numeric || !finite(bias) {
				return nil, invalid("invalid GGUF decision score bias")
			}
			if metadata.bias[int(levels)] == nil {
				metadata.bias[int(levels)] = make([]float32, levels)
			}
			metadata.bias[int(levels)][index] = bias
		}
	}
	return metadata, nil
}

// tensorLayouts maps a ggml_type of llama.cpp b11756 (ggml-common.h, ggml.c)
// to its elements and bytes per block.
var tensorLayouts = map[uint32][2]uint64{
	0: {1, 4}, 26: {1, 4}, 1: {1, 2}, 25: {1, 2}, 30: {1, 2}, 2: {32, 18}, 20: {32, 18},
	3: {32, 20}, 6: {32, 22}, 7: {32, 24}, 8: {32, 34}, 9: {32, 36}, 10: {256, 84},
	11: {256, 110}, 21: {256, 110}, 12: {256, 144}, 13: {256, 176}, 14: {256, 210},
	15: {256, 292}, 16: {256, 66}, 35: {256, 66}, 17: {256, 74}, 18: {256, 98}, 19: {256, 50},
	22: {256, 82}, 23: {256, 136}, 24: {1, 1}, 27: {1, 8}, 28: {1, 8}, 29: {256, 56},
	34: {256, 54}, 39: {32, 17}, 40: {64, 36}, 41: {128, 18}, 42: {64, 18},
}
