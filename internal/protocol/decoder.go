package protocol

import (
	"bytes"
	"encoding"
	"encoding/json"
	"encoding/json/jsontext"
	jsonv2 "encoding/json/v2"
	"errors"
	"fmt"
	"io"
	"reflect"
	"slices"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"unicode/utf8"
)

// Decoder reads one JSON document in a single pass for the typed decoders of
// this package. A type that implements DecodeJSON is decoded in place from the
// shared token stream; every other type receives the bytes of its own value.
//
// The accepted inputs and resulting values are those of the reflective
// helpers this replaces: required members, no duplicate known member, unknown
// members rejected only where denied, null only for optional, raw and unit
// values, and raw values retained in the compact HTML-escaped form produced by
// encoding/json.
type Decoder struct {
	current *source
	parents []*source
	spare   []*source
	names   []byte
	// normalized is set inside an object: nested values handed to
	// UnmarshalJSON and retained raw values use the encoding/json form.
	normalized bool
	// strict rejects unknown members of nested structs without a decoder.
	strict bool
	large  bool
	resume resumption
}

// source is one byte slice being tokenized. Reading from a bytes.Buffer makes
// the tokenizer work on the slice itself, so raw values are subslices of data.
type source struct {
	text   jsontext.Decoder
	buffer bytes.Buffer
	data   []byte
}

// resumption describes an internally tagged object whose opening brace, tag
// and the members before the tag were already read. The next Object call
// continues it; any other read first turns it into the object without its tag.
type resumption struct {
	// base, when set, also receives the members its struct declares.
	base *objectState

	pending bool
	start   int
	members int
	tag     string
}

type decodable interface{ DecodeJSON(*Decoder) error }

var (
	textOptions = jsonv2.JoinOptions(jsontext.AllowDuplicateNames(true), jsontext.AllowInvalidUTF8(true))
	decoders    = sync.Pool{New: func() any { return new(Decoder) }}
	errNull     = error(checkError("value must not be null"))
)

// checkError is a violation of the null, numeric token or fixed-length rule.
// The object that holds the value names its field in front of the message.
type checkError string

func (err checkError) Error() string { return string(err) }

// typeError is a JSON value of the wrong kind for its Go type. It leaves this
// package as the *json.UnmarshalTypeError encoding/json reports, naming the
// struct field when a plain field of an object held the value.
type typeError struct {
	value  string
	target reflect.Type
}

func (err *typeError) Error() string {
	return "json: cannot unmarshal " + err.value + " into Go value of type " + err.target.String()
}

func published(err error) error {
	if mismatched, ok := err.(*typeError); ok {
		return &json.UnmarshalTypeError{Value: mismatched.value, Type: mismatched.target}
	}
	return err
}

// Decode decodes data, which must hold exactly one JSON value, into a target
// whose JSON form is an object.
func Decode(data []byte, target interface{ DecodeJSON(*Decoder) error }) error {
	decoder := newDecoder(data)
	err := target.DecodeJSON(decoder)
	if err == nil {
		err = decoder.finish()
	}
	decoder.release()
	if err != nil {
		return objectError(data, err)
	}
	return nil
}

// DecodeValue is Decode for a target whose JSON form is not an object.
func DecodeValue(data []byte, target interface{ DecodeJSON(*Decoder) error }) error {
	decoder := newDecoder(data)
	err := target.DecodeJSON(decoder)
	if err == nil {
		err = decoder.finish()
	}
	decoder.release()
	return syntaxError(data, err)
}

// DecodeObject decodes one JSON object into the struct target points to.
func DecodeObject(data []byte, target any, required []string, denyUnknown bool) error {
	decoder := newDecoder(data)
	err := decoder.Object(target, required, denyUnknown)
	if err == nil {
		err = decoder.finish()
	}
	decoder.release()
	if err != nil {
		return objectError(data, err)
	}
	return nil
}

// UnmarshalUnion decodes one tagged union. An empty tag selects the external
// representation, a tag with content the adjacent one and a tag alone the
// internal one. A nil factory marks a unit variant.
func UnmarshalUnion(data []byte, tag, content string, variants map[string]func() any, denyUnknown bool) (string, any, error) {
	decoder := newDecoder(data)
	kind, value, err := decoder.Union(tag, content, variants, denyUnknown)
	if err == nil {
		err = decoder.finish()
	}
	decoder.release()
	if err != nil {
		if text := bytes.TrimSpace(data); tag == "" && len(text) > 0 && text[0] == '"' {
			return "", nil, syntaxError(data, err)
		}
		return "", nil, objectError(data, err)
	}
	return kind, value, nil
}

func newDecoder(data []byte) *Decoder {
	decoder := decoders.Get().(*Decoder)
	decoder.push(data)
	return decoder
}

func (d *Decoder) release() {
	for d.current != nil {
		d.pop()
	}
	if d.large {
		// A tokenizer keeps its last input until it is reset.
		d.spare, d.large = nil, false
	}
	d.normalized, d.strict, d.resume = false, false, resumption{}
	decoders.Put(d)
}

func (d *Decoder) push(data []byte) {
	var next *source
	if count := len(d.spare); count > 0 {
		next, d.spare = d.spare[count-1], d.spare[:count-1]
	} else {
		next = new(source)
	}
	next.data = data
	next.buffer = *bytes.NewBuffer(data)
	next.text.Reset(&next.buffer, textOptions)
	if d.current != nil {
		d.parents = append(d.parents, d.current)
	}
	if len(data) > 64*1024 {
		d.large = true
	}
	d.current = next
}

func (d *Decoder) pop() {
	d.current.data = nil
	d.spare = append(d.spare, d.current)
	d.current = nil
	if count := len(d.parents); count > 0 {
		d.current, d.parents = d.parents[count-1], d.parents[:count-1]
	}
}

// finish requires the end of the current input.
func (d *Decoder) finish() error {
	_, err := d.current.text.ReadToken()
	if err == io.EOF {
		return nil
	}
	if err == nil {
		err = errors.New("unexpected data after value")
	}
	return err
}

// syntaxError reports malformed input with the error encoding/json returns.
func syntaxError(data []byte, err error) error {
	var syntactic *jsontext.SyntacticError
	if err == nil || !errors.As(err, &syntactic) && err != io.EOF && !errors.Is(err, io.ErrUnexpectedEOF) {
		return published(err)
	}
	var value json.RawMessage
	if malformed := json.Unmarshal(data, &value); malformed != nil {
		return malformed
	}
	return published(err)
}

// objectError reports malformed input where an object is expected. Syntax is
// reported before any other failure, with the errors of the token reader that
// first split objects into members.
func objectError(data []byte, err error) error {
	decoder := json.NewDecoder(bytes.NewReader(data))
	start, malformed := decoder.Token()
	if malformed != nil {
		return malformed
	}
	if start != json.Delim('{') {
		return errors.New("expected object")
	}
	for decoder.More() {
		if _, malformed := decoder.Token(); malformed != nil {
			return malformed
		}
		var value json.RawMessage
		if malformed := decoder.Decode(&value); malformed != nil {
			return malformed
		}
	}
	if _, malformed := decoder.Token(); malformed != nil {
		return malformed
	}
	if decoder.Decode(new(any)) != io.EOF {
		return errors.New("unexpected data after object")
	}
	return published(err)
}

func (d *Decoder) kind() jsontext.Kind { return d.current.text.PeekKind() }

// raw reads the next value and returns its bytes inside the current input.
func (d *Decoder) raw() ([]byte, error) {
	value, err := d.current.text.ReadValue()
	if err != nil {
		return nil, err
	}
	end := int(d.current.text.InputOffset())
	return d.current.data[end-len(value) : end : end], nil
}

func (d *Decoder) skip() error {
	_, err := d.current.text.ReadValue()
	return err
}

// expect consumes one structural token.
func (d *Decoder) expect(kind jsontext.Kind, name string) error {
	if d.kind() != kind {
		if d.kind() == 0 {
			_, err := d.current.text.ReadToken()
			return err
		}
		return fmt.Errorf("expected %s", name)
	}
	_, err := d.current.text.ReadToken()
	return err
}

// name returns the decoded member name held by a raw JSON string. The result
// is valid until the next call.
func (d *Decoder) name(raw []byte) []byte {
	body := raw[1 : len(raw)-1]
	for _, c := range body {
		if c == '\\' || c >= utf8.RuneSelf {
			d.names, _ = jsontext.AppendUnquote(d.names[:0], raw)
			return d.names
		}
	}
	return body
}

// unquote decodes a raw JSON string; invalid UTF-8 becomes U+FFFD as in
// encoding/json.
func unquote(raw []byte) string {
	if bytes.IndexByte(raw, '\\') < 0 && utf8.Valid(raw) {
		return string(raw[1 : len(raw)-1])
	}
	decoded, _ := jsontext.AppendUnquote(nil, raw)
	return string(decoded)
}

// normal returns raw in the form encoding/json gives a nested raw value when
// it re-encodes its parent: insignificant whitespace removed and <, >, &,
// U+2028 and U+2029 escaped. Raw values retained by callers feed digests, so
// these bytes are part of the stored format.
func (d *Decoder) normal(raw []byte) []byte {
	if !d.normalized || isNormal(raw) {
		return raw
	}
	encoded, err := json.Marshal(json.RawMessage(raw))
	if err != nil {
		return raw
	}
	return encoded
}

// isNormal reports whether encoding/json would re-encode raw unchanged. It
// may report false for a value that is in fact unchanged.
func isNormal(raw []byte) bool {
	high := false
	for index := 0; index < len(raw); index++ {
		c := raw[index]
		if c != '"' {
			if c <= ' ' {
				return false
			}
			continue
		}
		for index++; index < len(raw) && raw[index] != '"'; index++ {
			switch c := raw[index]; {
			case c == '\\':
				index++
			case c == '<' || c == '>' || c == '&':
				return false
			case c >= utf8.RuneSelf:
				high = true
			}
		}
	}
	return !high || utf8.Valid(raw) && !bytes.Contains(raw, []byte("\u2028")) && !bytes.Contains(raw, []byte("\u2029"))
}

// String reads a JSON string. A null yields the empty string, which is what
// encoding/json leaves in a string it decodes null into.
func (d *Decoder) String() (string, error) {
	if err := d.settle(); err != nil {
		return "", err
	}
	raw, err := d.raw()
	if err != nil {
		return "", err
	}
	switch raw[0] {
	case '"':
		return unquote(raw), nil
	case 'n':
		return "", nil
	}
	return "", mismatch(raw, reflect.TypeFor[string]())
}

func mismatch(raw []byte, target reflect.Type) error {
	value := "number"
	switch raw[0] {
	case '"':
		value = "string"
	case 't', 'f':
		value = "bool"
	case '{':
		value = "object"
	case '[':
		value = "array"
	case 'n':
		return errNull
	}
	return &typeError{value: value, target: target}
}

// Value decodes the next value into what target points to, by its Go type.
func (d *Decoder) Value(target any) error {
	if err := d.settle(); err != nil {
		return err
	}
	value := reflect.ValueOf(target).Elem()
	return published(codecOf(value.Type()).decode(d, value))
}

// Object decodes one JSON object into the struct target points to. Members
// named in required must be present, and non-null when their field is a
// pointer. Fields keep their value when their member is absent.
func (d *Decoder) Object(target any, required []string, denyUnknown bool) error {
	value := reflect.ValueOf(target).Elem()
	plan := planOf(value.Type())
	resume := d.resume
	d.resume = resumption{}
	normalized, strict := d.normalized, d.strict
	d.normalized, d.strict = true, denyUnknown
	err := d.object(value, plan, plan.need(required), denyUnknown, resume)
	d.normalized, d.strict = normalized, strict
	return err
}

type objectState struct {
	value reflect.Value
	plan  *structPlan
	need  *requirement
	deny  bool
	tag   string
	next  int
	seen  []uint64
	extra uint64
	base  *objectState
}

func (d *Decoder) object(value reflect.Value, plan *structPlan, need *requirement, deny bool, resume resumption) error {
	var words [4]uint64
	state := objectState{value: value, plan: plan, need: need, deny: deny, seen: words[:], base: resume.base}
	if plan.words > len(words) {
		state.seen = make([]uint64, plan.words)
	}
	if resume.pending {
		// The members before the tag are read again from the object's start.
		d.push(d.current.data[resume.start:])
		_, err := d.current.text.ReadToken()
		for index := 0; err == nil && index < resume.members; index++ {
			err = d.member(&state)
		}
		d.pop()
		if err != nil {
			return err
		}
		state.tag = resume.tag
	} else if err := d.expect('{', "object"); err != nil {
		return err
	}
	for d.kind() != '}' {
		if err := d.member(&state); err != nil {
			return err
		}
	}
	if _, err := d.current.text.ReadToken(); err != nil {
		return err
	}
	return state.missing()
}

// missing reports the first required member the object did not hold.
func (state *objectState) missing() error {
	for word, mask := range state.need.mask {
		if missing := mask &^ state.seen[word]; missing != 0 {
			for index := range state.plan.fields {
				if index/64 == word && missing&(1<<(index%64)) != 0 {
					return fmt.Errorf("missing field %q", state.plan.fields[index].name)
				}
			}
		}
	}
	for index, name := range state.need.extra {
		if state.extra&(1<<index) == 0 {
			return fmt.Errorf("missing field %q", name)
		}
	}
	return nil
}

func (d *Decoder) member(state *objectState) error {
	raw, err := d.raw()
	if err != nil {
		return err
	}
	name := d.name(raw)
	if state.tag != "" && string(name) == state.tag {
		return fmt.Errorf("duplicate field %q", name)
	}
	index := state.plan.find(name, state.next)
	if base := state.base; base != nil {
		if shared := base.plan.find(name, base.next); shared >= 0 {
			if index < 0 && state.deny {
				return fmt.Errorf("unknown field %q", name)
			}
			strict := d.strict
			d.strict = base.deny
			if index < 0 {
				err = d.field(base, shared)
				d.strict = strict
				return err
			}
			// Both structs declare the member: each decodes its own copy.
			value, err := d.raw()
			if err != nil {
				return err
			}
			d.push(value)
			err = d.field(base, shared)
			d.pop()
			d.strict = strict
			if err != nil {
				return err
			}
			d.push(value)
			err = d.field(state, index)
			d.pop()
			return err
		}
		base.note(name)
	}
	if index < 0 {
		if state.deny {
			return fmt.Errorf("unknown field %q", name)
		}
		state.note(name)
		return d.skip()
	}
	return d.field(state, index)
}

// note records a required member that the struct does not declare.
func (state *objectState) note(name []byte) {
	for position, extra := range state.need.extra {
		if string(name) == extra {
			state.extra |= 1 << position
		}
	}
}

// field decodes the next value into the field at index of the object's plan.
func (d *Decoder) field(state *objectState, index int) error {
	state.next = index + 1
	word, bit := index/64, uint64(1)<<(index%64)
	if state.seen[word]&bit != 0 {
		return fmt.Errorf("duplicate field %q", state.plan.fields[index].name)
	}
	state.seen[word] |= bit
	field := &state.plan.fields[index]
	if state.need.pointers[word]&bit != 0 && d.kind() == 'n' {
		return fmt.Errorf("field %q must not be null", field.name)
	}
	if err := field.codec.decode(d, state.value.Field(field.index)); err != nil {
		switch failure := err.(type) {
		case checkError:
			return fmt.Errorf("field %q: %w", field.name, err)
		case *typeError:
			return &json.UnmarshalTypeError{Value: failure.value, Type: failure.target, Struct: state.value.Type().Name(), Field: field.name}
		}
		return err
	}
	return nil
}

// Union decodes one tagged union; see UnmarshalUnion.
func (d *Decoder) Union(tag, content string, variants map[string]func() any, denyUnknown bool) (string, any, error) {
	if err := d.settle(); err != nil {
		return "", nil, err
	}
	switch {
	case tag == "":
		return d.external(variants)
	case content != "":
		return d.adjacent(tag, content, variants, denyUnknown)
	}
	return d.internal(tag, variants, denyUnknown)
}

func (d *Decoder) external(variants map[string]func() any) (string, any, error) {
	if d.kind() == '"' {
		raw, err := d.raw()
		if err != nil {
			return "", nil, err
		}
		kind := unquote(raw)
		factory, exists := variants[kind]
		if !exists {
			return "", nil, fmt.Errorf("unknown variant %q", kind)
		}
		if factory != nil {
			return "", nil, fmt.Errorf("missing variant content")
		}
		return kind, nil, nil
	}
	if err := d.expect('{', "object"); err != nil {
		return "", nil, err
	}
	if d.kind() == '}' {
		return "", nil, fmt.Errorf("expected one external variant")
	}
	raw, err := d.raw()
	if err != nil {
		return "", nil, err
	}
	kind := unquote(raw)
	factory, exists := variants[kind]
	if !exists {
		return "", nil, fmt.Errorf("unknown variant %q", kind)
	}
	var value any
	if factory == nil {
		if err := d.unit(); err != nil {
			return "", nil, err
		}
	} else {
		value = factory()
		if err := d.variant(value); err != nil {
			return "", nil, err
		}
	}
	if d.kind() != '}' {
		if d.kind() == 0 {
			_, err := d.current.text.ReadToken()
			return "", nil, err
		}
		return "", nil, fmt.Errorf("expected one external variant")
	}
	if _, err := d.current.text.ReadToken(); err != nil {
		return "", nil, err
	}
	return kind, value, nil
}

// unit accepts the null content of a unit variant.
func (d *Decoder) unit() error {
	raw, err := d.raw()
	if err != nil {
		return err
	}
	if string(raw) != "null" {
		return fmt.Errorf("expected unit variant")
	}
	return nil
}

// tagValue reads a variant name. encoding/json leaves a string unchanged when
// it decodes null, so a null tag names the empty variant.
func (d *Decoder) tagValue() (string, error) {
	raw, err := d.raw()
	if err != nil {
		return "", err
	}
	switch raw[0] {
	case '"':
		return unquote(raw), nil
	case 'n':
		return "", nil
	}
	return "", mismatch(raw, reflect.TypeFor[string]())
}

func (d *Decoder) adjacent(tag, content string, variants map[string]func() any, denyUnknown bool) (string, any, error) {
	if err := d.expect('{', "object"); err != nil {
		return "", nil, err
	}
	var (
		kind                string
		value               any
		factory             func() any
		tagged, carried     bool
		deferred            []byte
		deferredAt, decoded bool
	)
	for d.kind() != '}' {
		raw, err := d.raw()
		if err != nil {
			return "", nil, err
		}
		name := d.name(raw)
		switch string(name) {
		case tag:
			if tagged {
				return "", nil, fmt.Errorf("duplicate field %q", tag)
			}
			tagged = true
			if kind, err = d.tagValue(); err != nil {
				return "", nil, err
			}
			exists := false
			if factory, exists = variants[kind]; !exists {
				return "", nil, fmt.Errorf("unknown variant %q", kind)
			}
		case content:
			if carried {
				return "", nil, fmt.Errorf("duplicate field %q", content)
			}
			carried = true
			if !tagged {
				// The variant is not known yet: keep the bytes and decode them
				// once the tag was read.
				if deferred, err = d.raw(); err != nil {
					return "", nil, err
				}
				deferredAt = true
				continue
			}
			if factory == nil {
				if err := d.unit(); err != nil {
					return "", nil, err
				}
				continue
			}
			value, decoded = factory(), true
			if err := d.variant(value); err != nil {
				return "", nil, err
			}
		default:
			if denyUnknown {
				return "", nil, fmt.Errorf("unknown enum field")
			}
			if err := d.skip(); err != nil {
				return "", nil, err
			}
		}
	}
	if _, err := d.current.text.ReadToken(); err != nil {
		return "", nil, err
	}
	if !tagged {
		return "", nil, fmt.Errorf("missing field %q", tag)
	}
	if factory == nil {
		if deferredAt && string(deferred) != "null" {
			return "", nil, fmt.Errorf("expected unit variant")
		}
		return kind, nil, nil
	}
	if decoded {
		return kind, value, nil
	}
	if !deferredAt {
		return "", nil, fmt.Errorf("missing variant content")
	}
	value = factory()
	d.push(deferred)
	err := d.variant(value)
	d.pop()
	if err != nil {
		return "", nil, err
	}
	return kind, value, nil
}

func (d *Decoder) internal(tag string, variants map[string]func() any, denyUnknown bool) (string, any, error) {
	if err := d.expect('{', "object"); err != nil {
		return "", nil, err
	}
	resume := resumption{pending: true, start: int(d.current.text.InputOffset()) - 1, tag: tag}
	for {
		if d.kind() == '}' {
			return "", nil, fmt.Errorf("missing field %q", tag)
		}
		raw, err := d.raw()
		if err != nil {
			return "", nil, err
		}
		if string(d.name(raw)) == tag {
			break
		}
		if err := d.skip(); err != nil {
			return "", nil, err
		}
		resume.members++
	}
	kind, err := d.tagValue()
	if err != nil {
		return "", nil, err
	}
	factory, exists := variants[kind]
	if !exists {
		return "", nil, fmt.Errorf("unknown variant %q", kind)
	}
	if factory == nil {
		if denyUnknown && resume.members > 0 {
			return "", nil, fmt.Errorf("unknown unit variant field")
		}
		for d.kind() != '}' {
			raw, err := d.raw()
			if err != nil {
				return "", nil, err
			}
			if string(d.name(raw)) == tag {
				return "", nil, fmt.Errorf("duplicate field %q", tag)
			}
			if denyUnknown {
				return "", nil, fmt.Errorf("unknown unit variant field")
			}
			if err := d.skip(); err != nil {
				return "", nil, err
			}
		}
		if _, err := d.current.text.ReadToken(); err != nil {
			return "", nil, err
		}
		return kind, nil, nil
	}
	value := factory()
	strict := d.strict
	d.strict = false
	err = d.flattened(value, resume)
	d.strict = strict
	if err != nil {
		return "", nil, err
	}
	return kind, value, nil
}

// Tagged decodes one object into the struct base points to and, from the same
// members without the tag, into the variant the tag names: DecodeObject
// without denyUnknown followed by an internally tagged UnmarshalUnion.
func (d *Decoder) Tagged(base any, required []string, tag string, variants map[string]func() any) (string, any, error) {
	if err := d.settle(); err != nil {
		return "", nil, err
	}
	if err := d.expect('{', "object"); err != nil {
		return "", nil, err
	}
	source, start := d.current, int(d.current.text.InputOffset())-1
	value := reflect.ValueOf(base).Elem()
	plan := planOf(value.Type())
	var (
		words   [4]uint64
		kind    string
		variant any
		found   bool
	)
	state := objectState{value: value, plan: plan, need: plan.need(required), seen: words[:]}
	if plan.words > len(words) {
		state.seen = make([]uint64, plan.words)
	}
	resume := resumption{pending: true, start: start, tag: tag, base: &state}
	for d.kind() != '}' {
		raw, err := d.raw()
		if err != nil {
			return "", nil, err
		}
		if string(d.name(raw)) == tag {
			found = true
			state.note([]byte(tag))
			break
		}
		if err := d.skip(); err != nil {
			return "", nil, err
		}
		resume.members++
	}
	if found && d.kind() == '"' {
		raw, err := d.raw()
		if err != nil {
			return "", nil, err
		}
		kind = unquote(raw)
		if factory := variants[kind]; factory != nil {
			variant = factory()
		}
		if _, streamed := variant.(decodable); !streamed || plan.find([]byte(tag), 0) >= 0 {
			variant = nil
		}
	}
	if variant == nil {
		// Every other shape takes the two passes over the whole object.
		for depth := source.text.StackDepth(); ; {
			if _, err := source.text.ReadToken(); err != nil {
				return "", nil, err
			}
			if source.text.StackDepth() < depth {
				break
			}
		}
		whole := source.data[start:int(source.text.InputOffset())]
		d.push(whole)
		err := d.Object(base, required, false)
		d.pop()
		if err != nil {
			return "", nil, err
		}
		d.push(whole)
		kind, variant, err = d.Union(tag, "", variants, false)
		d.pop()
		return kind, variant, err
	}
	strict := d.strict
	d.strict = false
	err := d.flattened(variant, resume)
	d.strict = strict
	if err == nil {
		err = state.missing()
	}
	if err != nil {
		return "", nil, err
	}
	return kind, variant, nil
}

// flattened decodes the members of an internally tagged object, without its
// tag, into a variant.
func (d *Decoder) flattened(value any, resume resumption) error {
	if streamed, ok := value.(decodable); ok {
		depth := len(d.parents)
		d.resume = resume
		err := published(streamed.DecodeJSON(d))
		if err == nil && d.resume.pending {
			err = errors.New("variant did not read its content")
		}
		d.resume = resumption{}
		if len(d.parents) > depth {
			// The variant read the object without its tag as a value.
			if err == nil {
				err = d.finish()
			}
			for len(d.parents) > depth {
				d.pop()
			}
		}
		return err
	}
	payload, err := d.payload(resume)
	if err != nil {
		return err
	}
	target := reflect.ValueOf(value).Elem()
	d.push(payload)
	if err = codecOf(target.Type()).decode(d, target); err == nil {
		err = d.finish()
	}
	d.pop()
	return published(err)
}

// settle turns a pending internally tagged object into its own input for a
// variant that does not decode it as an object of this package.
func (d *Decoder) settle() error {
	if !d.resume.pending {
		return nil
	}
	resume := d.resume
	d.resume = resumption{}
	payload, err := d.payload(resume)
	if err != nil {
		return err
	}
	if base := resume.base; base != nil {
		normalized, strict := d.normalized, d.strict
		d.normalized, d.strict = true, base.deny
		d.push(payload)
		_, err := d.current.text.ReadToken()
		for err == nil && d.kind() != '}' {
			err = d.member(base)
		}
		d.pop()
		d.normalized, d.strict = normalized, strict
		if err != nil {
			return err
		}
	}
	d.push(payload)
	return nil
}

// payload reads the rest of an internally tagged object and returns it
// without its tag: member names in their encoding/json form, values as read.
func (d *Decoder) payload(resume resumption) ([]byte, error) {
	out := []byte{'{'}
	add := func() error {
		raw, err := d.raw()
		if err != nil {
			return err
		}
		if resume.tag != "" && string(d.name(raw)) == resume.tag {
			return fmt.Errorf("duplicate field %q", resume.tag)
		}
		if len(out) > 1 {
			out = append(out, ',')
		}
		if isNormal(raw) && bytes.IndexByte(raw, '\\') < 0 {
			out = append(out, raw...)
		} else {
			name, err := json.Marshal(string(d.name(raw)))
			if err != nil {
				return err
			}
			out = append(out, name...)
		}
		out = append(out, ':')
		if raw, err = d.raw(); err != nil {
			return err
		}
		out = append(out, raw...)
		return nil
	}
	d.push(d.current.data[resume.start:])
	_, err := d.current.text.ReadToken()
	tag := resume.tag
	resume.tag = ""
	for index := 0; err == nil && index < resume.members; index++ {
		err = add()
	}
	resume.tag = tag
	d.pop()
	if err != nil {
		return nil, err
	}
	for d.kind() != '}' {
		if err := add(); err != nil {
			return nil, err
		}
	}
	if _, err := d.current.text.ReadToken(); err != nil {
		return nil, err
	}
	return append(out, '}'), nil
}

// variant decodes the content of a variant from the next value.
func (d *Decoder) variant(value any) error {
	strict := d.strict
	d.strict = false
	var err error
	if streamed, ok := value.(decodable); ok && d.kind() != 'n' {
		err = streamed.DecodeJSON(d)
	} else {
		target := reflect.ValueOf(value).Elem()
		err = codecOf(target.Type()).decode(d, target)
	}
	d.strict = strict
	return published(err)
}

// structPlan is the member table of one struct type, built once.
type structPlan struct {
	fields []fieldPlan
	index  map[string]int
	words  int
	first  atomic.Pointer[requirement]
}

type fieldPlan struct {
	name    string
	index   int
	pointer bool
	codec   *codec
}

// requirement is one required member list resolved against a plan.
type requirement struct {
	names    []string
	mask     []uint64
	pointers []uint64
	extra    []string
}

var plans sync.Map // reflect.Type → *structPlan

func planOf(target reflect.Type) *structPlan {
	if plan, ok := plans.Load(target); ok {
		return plan.(*structPlan)
	}
	plan := &structPlan{index: map[string]int{}}
	for index := range target.NumField() {
		field := target.Field(index)
		name := strings.Split(field.Tag.Get("json"), ",")[0]
		if name == "-" || field.PkgPath != "" {
			continue
		}
		if name == "" {
			name = field.Name
		}
		if _, exists := plan.index[name]; exists {
			continue
		}
		plan.index[name] = len(plan.fields)
		plan.fields = append(plan.fields, fieldPlan{name: name, index: index, pointer: field.Type.Kind() == reflect.Pointer, codec: codecOf(field.Type)})
	}
	plan.words = (len(plan.fields) + 63) / 64
	actual, _ := plans.LoadOrStore(target, plan)
	return actual.(*structPlan)
}

func (plan *structPlan) find(name []byte, next int) int {
	if next < len(plan.fields) && plan.fields[next].name == string(name) {
		return next
	}
	if index, ok := plan.index[string(name)]; ok {
		return index
	}
	return -1
}

// need resolves a required list. Each decoder passes the same list on every
// call, so the first one is kept and later calls only compare it.
func (plan *structPlan) need(required []string) *requirement {
	if first := plan.first.Load(); first != nil && slices.Equal(first.names, required) {
		return first
	}
	words := max(plan.words, 4)
	resolved := &requirement{names: slices.Clone(required), mask: make([]uint64, words), pointers: make([]uint64, words)}
	for _, name := range required {
		index, ok := plan.index[name]
		if !ok {
			if len(resolved.extra) < 64 && !slices.Contains(resolved.extra, name) {
				resolved.extra = append(resolved.extra, name)
			}
			continue
		}
		resolved.mask[index/64] |= 1 << (index % 64)
		if plan.fields[index].pointer {
			resolved.pointers[index/64] |= 1 << (index % 64)
		}
	}
	plan.first.CompareAndSwap(nil, resolved)
	return resolved
}

// codec decodes the next value into an addressable value of one type.
type codec struct {
	decode func(d *Decoder, value reflect.Value) error
}

var (
	codecs       sync.Map // reflect.Type → *codec
	codecBuilder sync.Mutex

	rawMessageType  = reflect.TypeFor[json.RawMessage]()
	numberType      = reflect.TypeFor[json.Number]()
	unitType        = reflect.TypeFor[Unit]()
	decodableType   = reflect.TypeFor[decodable]()
	unmarshalerType = reflect.TypeFor[json.Unmarshaler]()
	streamerType    = reflect.TypeFor[jsonv2.UnmarshalerFrom]()
	textType        = reflect.TypeFor[encoding.TextUnmarshaler]()
	modulePath      = strings.TrimSuffix(reflect.TypeFor[Unit]().PkgPath(), "internal/protocol")
)

func codecOf(target reflect.Type) *codec {
	if found, ok := codecs.Load(target); ok {
		return found.(*codec)
	}
	codecBuilder.Lock()
	defer codecBuilder.Unlock()
	building := map[reflect.Type]*codec{}
	result := buildCodec(target, building)
	for built, value := range building {
		codecs.LoadOrStore(built, value)
	}
	return result
}

func buildCodec(target reflect.Type, building map[reflect.Type]*codec) *codec {
	if found, ok := codecs.Load(target); ok {
		return found.(*codec)
	}
	if found, ok := building[target]; ok {
		return found
	}
	built := &codec{}
	building[target] = built
	pointer := reflect.PointerTo(target)
	switch {
	case target == rawMessageType:
		built.decode = decodeRawMessage
	case target == numberType:
		built.decode = func(d *Decoder, value reflect.Value) error {
			raw, err := d.raw()
			if err != nil {
				return err
			}
			if raw[0] != '-' && (raw[0] < '0' || raw[0] > '9') {
				if raw[0] == '"' {
					return checkError("expected numeric token")
				}
				return mismatch(raw, target)
			}
			value.SetString(string(raw))
			return nil
		}
	case target.Kind() == reflect.Interface:
		built.decode = decodeChecked
	case target.Kind() == reflect.Pointer:
		element := buildCodec(target.Elem(), building)
		built.decode = func(d *Decoder, value reflect.Value) error {
			if d.kind() == 'n' {
				if _, err := d.raw(); err != nil {
					return err
				}
				value.SetZero()
				return nil
			}
			if value.IsNil() {
				value.Set(reflect.New(target.Elem()))
			}
			return element.decode(d, value.Elem())
		}
	case pointer.Implements(decodableType):
		built.decode = func(d *Decoder, value reflect.Value) error {
			if d.kind() == 'n' {
				return errNull
			}
			return published(value.Addr().Interface().(decodable).DecodeJSON(d))
		}
	case pointer.Implements(unmarshalerType) && !pointer.Implements(streamerType) && strings.HasPrefix(target.PkgPath(), modulePath):
		built.decode = func(d *Decoder, value reflect.Value) error {
			raw, err := d.raw()
			if err != nil {
				return err
			}
			if err := checkJSONNulls(raw, target); err != nil {
				return err
			}
			return value.Addr().Interface().(json.Unmarshaler).UnmarshalJSON(d.normal(raw))
		}
	case pointer.Implements(unmarshalerType) || pointer.Implements(streamerType) || pointer.Implements(textType):
		// encoding/json has its own rules for types such as time.Time.
		built.decode = decodeChecked
	default:
		built.decode = kindCodec(target, building)
	}
	return built
}

func kindCodec(target reflect.Type, building map[reflect.Type]*codec) func(*Decoder, reflect.Value) error {
	switch target.Kind() {
	case reflect.Bool:
		return func(d *Decoder, value reflect.Value) error {
			raw, err := d.raw()
			if err != nil {
				return err
			}
			if raw[0] != 't' && raw[0] != 'f' {
				return mismatch(raw, target)
			}
			value.SetBool(raw[0] == 't')
			return nil
		}
	case reflect.String:
		return func(d *Decoder, value reflect.Value) error {
			raw, err := d.raw()
			if err != nil {
				return err
			}
			if raw[0] != '"' {
				return mismatch(raw, target)
			}
			value.SetString(unquote(raw))
			return nil
		}
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		bits := target.Bits()
		return func(d *Decoder, value reflect.Value) error {
			raw, err := number(d, target)
			if err != nil {
				return err
			}
			parsed, err := strconv.ParseInt(string(raw), 10, bits)
			if err != nil {
				return &typeError{value: "number " + string(raw), target: target}
			}
			value.SetInt(parsed)
			return nil
		}
	case reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64, reflect.Uintptr:
		bits := target.Bits()
		return func(d *Decoder, value reflect.Value) error {
			raw, err := number(d, target)
			if err != nil {
				return err
			}
			parsed, err := strconv.ParseUint(string(raw), 10, bits)
			if err != nil {
				return &typeError{value: "number " + string(raw), target: target}
			}
			value.SetUint(parsed)
			return nil
		}
	case reflect.Float32, reflect.Float64:
		bits := target.Bits()
		return func(d *Decoder, value reflect.Value) error {
			raw, err := number(d, target)
			if err != nil {
				return err
			}
			parsed, err := strconv.ParseFloat(string(raw), bits)
			if err != nil {
				return &typeError{value: "number " + string(raw), target: target}
			}
			value.SetFloat(parsed)
			return nil
		}
	case reflect.Slice:
		element := buildCodec(target.Elem(), building)
		return func(d *Decoder, value reflect.Value) error {
			if d.kind() == 'n' {
				return errNull
			}
			if d.kind() != '[' || value.Cap() != 0 {
				// Binary strings and merging into existing elements follow
				// encoding/json.
				return decodeChecked(d, value)
			}
			if _, err := d.current.text.ReadToken(); err != nil {
				return err
			}
			count := 0
			for d.kind() != ']' {
				if count == value.Cap() {
					value.Grow(1)
				}
				value.SetLen(count + 1)
				if err := element.decode(d, value.Index(count)); err != nil {
					return err
				}
				count++
			}
			if _, err := d.current.text.ReadToken(); err != nil {
				return err
			}
			if count == 0 {
				value.Set(reflect.MakeSlice(target, 0, 0))
			}
			return nil
		}
	case reflect.Map:
		key := target.Key()
		text := reflect.PointerTo(key).Implements(textType)
		if !text && key.Kind() != reflect.String {
			return decodeChecked
		}
		element := buildCodec(target.Elem(), building)
		return func(d *Decoder, value reflect.Value) error {
			if d.kind() == 'n' {
				return errNull
			}
			if d.kind() != '{' || value.Len() != 0 {
				return decodeChecked(d, value)
			}
			if _, err := d.current.text.ReadToken(); err != nil {
				return err
			}
			source, depth, start := d.current, d.current.text.StackDepth(), int(d.current.text.InputOffset())-1
			created := value.IsNil()
			if created {
				value.Set(reflect.MakeMap(target))
			}
			err := d.entries(value, target, key, text, element)
			if err == nil {
				return nil
			}
			// A member the strict pass refuses may be shadowed by a later
			// duplicate, which the reflective helpers accepted: decide on the
			// whole value the way they did.
			if d.current != source {
				return err
			}
			for source.text.StackDepth() >= depth {
				if _, drained := source.text.ReadToken(); drained != nil {
					return err
				}
			}
			d.resume = resumption{}
			if created {
				value.SetZero()
			} else {
				value.Clear()
			}
			return d.checked(source.data[start:int(source.text.InputOffset())], value)
		}
	}
	return decodeChecked
}

func (d *Decoder) entries(value reflect.Value, target, key reflect.Type, text bool, element *codec) error {
	for d.kind() != '}' {
		raw, err := d.raw()
		if err != nil {
			return err
		}
		var name reflect.Value
		if text {
			name = reflect.New(key)
			if err := name.Interface().(encoding.TextUnmarshaler).UnmarshalText([]byte(unquote(raw))); err != nil {
				return err
			}
			name = name.Elem()
		} else {
			name = reflect.ValueOf(unquote(raw))
			if key != name.Type() {
				name = name.Convert(key)
			}
		}
		entry := reflect.New(target.Elem()).Elem()
		if err := element.decode(d, entry); err != nil {
			return err
		}
		value.SetMapIndex(name, entry)
	}
	_, err := d.current.text.ReadToken()
	return err
}

// number reads a JSON number for a numeric field.
func number(d *Decoder, target reflect.Type) ([]byte, error) {
	raw, err := d.raw()
	if err != nil {
		return nil, err
	}
	if raw[0] != '-' && (raw[0] < '0' || raw[0] > '9') {
		return nil, mismatch(raw, target)
	}
	return raw, nil
}

func decodeRawMessage(d *Decoder, value reflect.Value) error {
	raw, err := d.raw()
	if err != nil {
		return err
	}
	normal := d.normal(raw)
	if len(normal) > 0 && len(raw) > 0 && &normal[0] == &raw[0] {
		normal = bytes.Clone(raw)
	}
	value.SetBytes(normal)
	return nil
}

// decodeChecked hands the next value to encoding/json after the null rules.
func decodeChecked(d *Decoder, value reflect.Value) error {
	raw, err := d.raw()
	if err != nil {
		return err
	}
	return d.checked(raw, value)
}

func (d *Decoder) checked(raw []byte, value reflect.Value) error {
	if err := checkJSONNulls(raw, value.Type()); err != nil {
		return err
	}
	data := d.normal(raw)
	if d.strict {
		decoder := json.NewDecoder(bytes.NewReader(data))
		decoder.DisallowUnknownFields()
		return decoder.Decode(value.Addr().Interface())
	}
	return json.Unmarshal(data, value.Addr().Interface())
}
