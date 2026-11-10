package onnx

import (
	"errors"
	"io"
	"os"

	"google.golang.org/protobuf/encoding/protowire"
)

// message is the projection of one ONNX 1.22.0 message: the fields that hold
// nested messages able to reference tensor data. Every other field, weight
// arrays included, is skipped without being read.
type message struct {
	nested map[protowire.Number]*message
	tensor bool
}

var (
	tensorMessage    = &message{tensor: true}
	sparseMessage    = &message{nested: map[protowire.Number]*message{1: tensorMessage, 2: tensorMessage}}
	attributeMessage = &message{}
	nodeMessage      = &message{nested: map[protowire.Number]*message{5: attributeMessage}}
	graphMessage     = &message{nested: map[protowire.Number]*message{1: nodeMessage, 5: tensorMessage, 15: sparseMessage}}
	functionMessage  = &message{nested: map[protowire.Number]*message{7: nodeMessage, 11: attributeMessage}}
	trainingMessage  = &message{nested: map[protowire.Number]*message{1: graphMessage, 2: graphMessage}}
	modelMessage     = &message{nested: map[protowire.Number]*message{7: graphMessage, 20: trainingMessage, 25: functionMessage}}
)

func init() {
	// Attributes and graphs contain each other.
	attributeMessage.nested = map[protowire.Number]*message{
		5: tensorMessage, 10: tensorMessage, 6: graphMessage, 11: graphMessage, 22: sparseMessage, 23: sparseMessage,
	}
}

var errMalformed = errors.New("ONNX model is malformed")

// Validate accepts a single-file model: it has a graph and no tensor stored
// outside the file. The file must not change while it is read.
func Validate(path string) error {
	source, err := os.Open(path)
	if err != nil {
		return err
	}
	defer source.Close()
	info, err := source.Stat()
	if err != nil {
		return err
	}
	graphs := 0
	err = scan(source, 0, info.Size(), modelMessage, depth, func(number protowire.Number) {
		if number == 7 {
			graphs++
		}
	})
	if err != nil {
		return err
	}
	if graphs == 0 {
		return errors.New("ONNX model graph is missing")
	}
	return nil
}

// depth bounds message nesting, as the reference decoder does.
const depth = 100

var errExternal = errors.New("ONNX model requires external tensor data")

// scan walks the fields of one message stored in source[start:end]. visit
// receives the number of every nested message of the outermost call.
func scan(source io.ReaderAt, start, end int64, schema *message, remaining int, visit func(protowire.Number)) error {
	if remaining == 0 {
		return errMalformed
	}
	// A tag and a length are at most ten bytes each.
	var header [20]byte
	for offset := start; offset < end; {
		size, err := source.ReadAt(header[:min(int64(len(header)), end-offset)], offset)
		if err != nil && err != io.EOF {
			return err
		}
		number, kind, used := protowire.ConsumeTag(header[:size])
		if used < 0 {
			return errMalformed
		}
		child := schema.nested[number]
		// Tensor field 13 is external_data and field 14 is data_location.
		marker, location := schema.tensor && number == 13, schema.tensor && number == 14
		switch kind {
		case protowire.BytesType:
			length, lengthSize := protowire.ConsumeVarint(header[used:size])
			body := offset + int64(used+lengthSize)
			if lengthSize < 0 || length > uint64(end-body) {
				return errMalformed
			}
			next := body + int64(length)
			switch {
			case marker:
				return errExternal
			case location:
				// A packed encoding of the enum occurrences.
				if length > 1024 {
					return errMalformed
				}
				packed := make([]byte, length)
				if _, err := source.ReadAt(packed, body); err != nil {
					return err
				}
				for len(packed) > 0 {
					value, valueSize := protowire.ConsumeVarint(packed)
					if valueSize < 0 {
						return errMalformed
					}
					if uint32(value) == 1 {
						return errExternal
					}
					packed = packed[valueSize:]
				}
			case child != nil:
				if visit != nil {
					visit(number)
				}
				if err := scan(source, body, next, child, remaining-1, nil); err != nil {
					return err
				}
			}
			offset = next
		case protowire.VarintType:
			value, valueSize := protowire.ConsumeVarint(header[used:size])
			if valueSize < 0 || child != nil || marker {
				return errMalformed
			}
			// data_location 1 is EXTERNAL; every occurrence counts.
			if location && uint32(value) == 1 {
				return errExternal
			}
			offset += int64(used + valueSize)
		case protowire.Fixed32Type, protowire.Fixed64Type:
			width := int64(4)
			if kind == protowire.Fixed64Type {
				width = 8
			}
			if child != nil || marker || location || offset+int64(used)+width > end {
				return errMalformed
			}
			offset += int64(used) + width
		default:
			return errMalformed
		}
	}
	return nil
}
