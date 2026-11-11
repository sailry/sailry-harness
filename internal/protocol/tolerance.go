package protocol

import (
	"encoding/json"
	"fmt"
	"reflect"
	"sync"
	"sync/atomic"
)

// Tolerance is what a decode does with input this build does not declare.
//
// A controller and a Host are compatible when their protocol versions are
// equal, and contracts change in place at that version. A peer of the same
// version may therefore send members, enum values and union variants this
// build was compiled without, and decoding them must not fail. Input that is
// not a peer's report is authored: a manifest, a settings file, a tool call,
// stored data. It is checked against exactly what this build declares.
type Tolerance uint8

const (
	// Authored is the default of every decoder in this package, including
	// json.Unmarshal on its types. An unknown member of a closed object, an
	// unknown enum value and an unknown union variant are errors.
	Authored Tolerance = iota
	// PeerCommand decodes what a peer asks this build to do. Unknown members
	// are ignored: the peer may know options this build does not have. Unknown
	// enum values and union variants are errors, because this build cannot
	// act on them.
	PeerCommand
	// PeerData decodes what a peer reports: replies, updates, snapshots.
	// Unknown members are ignored, an unknown enum value is kept as the value
	// the peer sent, and an unknown union variant decodes to its kind with an
	// UnknownVariant as content.
	PeerData
)

// UnknownVariant is the content of a union variant this build does not
// declare: the whole union value as the peer sent it. The union's Kind holds
// the undeclared name, so a switch over known kinds takes its default branch.
// Encoding the union writes these bytes back.
type UnknownVariant json.RawMessage

// DecodeAs decodes data, which must hold exactly one JSON value, into what
// target points to with the given tolerance. The tolerance reaches every
// nested value of this module's types.
func DecodeAs(tolerance Tolerance, data []byte, target any) error {
	decoder := newDecoder(data)
	decoder.tolerance = tolerance
	err := decoder.Value(target)
	if err == nil {
		err = decoder.finish()
	}
	decoder.release()
	return syntaxError(data, err)
}

// A tolerant decoder hands a nested value to its type's UnmarshalJSON as
// bytes, and that method starts a decoder of its own. The lease table carries
// the tolerance across that call: the slice handed over is leased for the
// duration of the call, and a decoder that starts on a leased slice inherits
// its tolerance. The key is the address of the slice's first byte, which only
// the callee holds. Authored decodes never touch the table.
var (
	leased atomic.Int64
	leases sync.Map // *byte → Tolerance
)

// lend leases data, which is not empty, at tolerance until the returned
// function is called. A slice an enclosing call already leased stays leased
// by that call.
func lend(data []byte, tolerance Tolerance) func() {
	first := &data[0]
	if _, held := leases.LoadOrStore(first, tolerance); held {
		return func() {}
	}
	leased.Add(1)
	return func() {
		leases.Delete(first)
		leased.Add(-1)
	}
}

// lent reports the tolerance data was handed over with.
func lent(data []byte) Tolerance {
	if leased.Load() == 0 || len(data) == 0 {
		return Authored
	}
	if tolerance, held := leases.Load(&data[0]); held {
		return tolerance.(Tolerance)
	}
	return Authored
}

// hand gives a nested value to its type's own decoder under the tolerance of
// this decode.
func (d *Decoder) hand(data []byte, target json.Unmarshaler) error {
	if d.tolerance == Authored || len(data) == 0 {
		return target.UnmarshalJSON(data)
	}
	defer lend(data, d.tolerance)()
	return target.UnmarshalJSON(data)
}

// unknownEnum settles an enum value this build does not declare, read from
// data by the enum's own UnmarshalJSON.
func unknownEnum[T ~string](data []byte, value *T, decoded string) error {
	return settleEnum(lent(data), value, decoded)
}

// settleEnum keeps an undeclared enum value a peer reports and refuses it in
// any other input.
func settleEnum[T ~string](tolerance Tolerance, value *T, decoded string) error {
	if tolerance != PeerData {
		return fmt.Errorf("unknown %s %q", reflect.TypeFor[T]().Name(), decoded)
	}
	*value = T(decoded)
	return nil
}

// marshalEnum writes an enum value. Declared values and values kept from a
// peer are written as they are; the empty value is never one of either, so it
// marks a field that was not set.
func marshalEnum[T ~string](value T) ([]byte, error) {
	if value == "" {
		return nil, fmt.Errorf("unset %s", reflect.TypeFor[T]().Name())
	}
	return json.Marshal(string(value))
}
