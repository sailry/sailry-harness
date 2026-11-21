package observation

import (
	"cmp"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"errors"
	"hash"
	"slices"
	"sync"

	"github.com/buger/jsonparser"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	maxResponsesBody   = 8 << 20
	maxResponsesItems  = 4096
	maxResponsesFields = 128
)

var errResponsesShape = errors.New("unsupported Responses request shape")

type responsesTraceKey struct{}

// ResponsesTrace belongs to one execution. Only the latest receipt's keyed
// digests and item sizes are retained, and only comparisons enter history.
type ResponsesTrace struct {
	key      [32]byte
	session  protocol.SessionID
	turn     protocol.TurnID
	mu       sync.Mutex
	previous responsesReceipt
}

type responsesReceipt struct {
	request string
	attempt uint64
	turn    protocol.TurnID
	cached  *uint64
	shape   *ResponsesShape
}

type ResponsesShape struct {
	instructions, tools, cacheKey, retention, other [32]byte
	instructionsBytes                               uint64
	input                                           []responsesItem
}

func ResponsesTraceFrom(ctx context.Context) *ResponsesTrace {
	trace, _ := ctx.Value(responsesTraceKey{}).(*ResponsesTrace)
	return trace
}

func (trace *ResponsesTrace) Prepare(raw []byte, inventory *Inventory) (Result, *ResponsesShape) {
	return prepare(raw, inventory, trace)
}

// The decoded body supplies validity and shape limits. Hash the original field
// spans directly so quoting, item order and precision stay as sent.
func (trace *ResponsesTrace) shape(raw []byte, body map[string]any) *ResponsesShape {
	if len(raw) > maxResponsesBody || len(body) > maxResponsesFields {
		return nil
	}
	for key := range body {
		if len(key) > 255 {
			return nil
		}
	}
	for _, name := range []string{"previous_response_id", "previous_interaction_id", "conversation", "prompt"} {
		if body[name] != nil {
			return nil
		}
	}
	items, isArray := body["input"].([]any)
	count := 1
	switch body["input"].(type) {
	case []any:
		if len(items) > maxResponsesItems {
			return nil
		}
		count = len(items)
		for _, item := range items {
			object, ok := item.(map[string]any)
			if !ok || object["type"] == "item_reference" {
				return nil
			}
		}
	case string:
	default:
		return nil
	}
	shape := &ResponsesShape{input: make([]responsesItem, 0, count)}
	seen := make(map[string]bool, len(body))
	type fieldDigest struct {
		name string
		sum  [32]byte
	}
	var other []fieldDigest
	mac := hmac.New(sha256.New, trace.key[:])
	err := jsonparser.ObjectEach(raw, func(key, value []byte, kind jsonparser.ValueType, _ int) error {
		if len(key) > 255 || seen[string(key)] || len(seen) == len(body) {
			return errResponsesShape
		}
		name := string(key)
		seen[name] = true
		switch name {
		case "input":
			if kind == jsonparser.String && !isArray {
				shape.input = append(shape.input, responsesItem{digest: digestResponseValue(mac, kind, value), size: uint32(responseValueBytes(kind, value)), kind: responseUser})
				return nil
			}
			if kind != jsonparser.Array || !isArray {
				return errResponsesShape
			}
			_, err := jsonparser.ArrayEach(value, func(item []byte, kind jsonparser.ValueType, _ int, err error) {
				if err == nil && kind == jsonparser.Object && len(shape.input) < count {
					shape.input = append(shape.input, responsesItem{
						digest: digestResponseValue(mac, kind, item),
						size:   uint32(len(item)),
						kind:   responseItemKind(items[len(shape.input)].(map[string]any)),
					})
				}
			})
			return err
		case "instructions":
			shape.instructions = digestResponseValue(mac, kind, value)
			shape.instructionsBytes = responseValueBytes(kind, value)
		case "tools":
			shape.tools = digestResponseValue(mac, kind, value)
		case "prompt_cache_key":
			shape.cacheKey = digestResponseValue(mac, kind, value)
		case "prompt_cache_retention":
			shape.retention = digestResponseValue(mac, kind, value)
		default:
			other = append(other, fieldDigest{name, digestResponseValue(mac, kind, value)})
		}
		return nil
	})
	if err != nil || !seen["input"] || len(shape.input) != count || len(seen) != len(body) {
		return nil
	}
	slices.SortFunc(other, func(a, b fieldDigest) int { return cmp.Compare(a.name, b.name) })
	for _, field := range other {
		mac.Write([]byte{byte(len(field.name))})
		mac.Write([]byte(field.name))
		mac.Write(field.sum[:])
	}
	mac.Sum(shape.other[:0])
	return shape
}

func digestResponseValue(mac hash.Hash, kind jsonparser.ValueType, value []byte) (digest [32]byte) {
	mac.Write([]byte{byte(kind)})
	mac.Write(value)
	mac.Sum(digest[:0])
	mac.Reset()
	return digest
}

func responseValueBytes(kind jsonparser.ValueType, value []byte) uint64 {
	size := len(value)
	if kind == jsonparser.String {
		size += 2 // jsonparser omits the surrounding quotes.
	}
	return uint64(size)
}

// Record advances in attempt order. Missing capture breaks adjacency, since
// an unobserved or unconfirmed request may have reached the upstream cache.
func (trace *ResponsesTrace) Record(request string, attempt uint64, cached *uint64, shape *ResponsesShape) *protocol.ConversationResponsesBoundary {
	trace.mu.Lock()
	defer trace.mu.Unlock()
	previous := trace.previous
	trace.previous = responsesReceipt{}
	if shape == nil {
		return nil
	}
	trace.previous = responsesReceipt{request: request, attempt: attempt, turn: trace.turn, cached: cached, shape: shape}
	if previous.shape == nil {
		return nil
	}
	var shared int
	var sharedBytes uint64
	for shared < min(len(previous.shape.input), len(shape.input)) && previous.shape.input[shared] == shape.input[shared] {
		sharedBytes += uint64(shape.input[shared].size)
		shared++
	}
	boundary := &protocol.ConversationResponsesBoundary{
		PreviousRequestID: previous.request, PreviousAttempt: previous.attempt, PreviousCachedInput: previous.cached,
		InstructionsEqual: previous.shape.instructions == shape.instructions,
		ToolsEqual:        previous.shape.tools == shape.tools,
		CacheOptionsEqual: previous.shape.cacheKey == shape.cacheKey && previous.shape.retention == shape.retention,
		OtherFieldsEqual:  previous.shape.other == shape.other,
		SharedInputItems:  uint64(shared), SharedInputBytes: &sharedBytes,
		PreviousInputItems: uint64(len(previous.shape.input)), InputItems: uint64(len(shape.input)),
	}
	if previous.turn != trace.turn {
		boundary.PreviousTurn = previous.turn
	}
	if !boundary.InstructionsEqual {
		boundary.InstructionsBytes = &protocol.ConversationResponsesFieldBytes{Previous: previous.shape.instructionsBytes, Current: shape.instructionsBytes}
	}
	boundary.InputChanges, boundary.InputReorderIncomplete = responseInputChanges(previous.shape.input[shared:], shape.input[shared:])
	return boundary
}

func (trace *ResponsesTrace) Clear() {
	trace.Record("", 0, nil, nil)
}
