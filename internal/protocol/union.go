package protocol

import (
	"bytes"
	"encoding/json"
	"fmt"
	"reflect"
)

type variantFactories map[string]func() any

func MarshalUnion(tag, content, kind string, value any, variants map[string]func() any) ([]byte, error) {
	factory, exists := variants[kind]
	if !exists {
		// A variant a peer reported and this build does not declare is
		// written back as it arrived.
		if unknown, ok := value.(UnknownVariant); ok && json.Valid(unknown) {
			return bytes.Clone(unknown), nil
		}
		return nil, fmt.Errorf("unknown variant %q", kind)
	}
	if factory == nil {
		if tag == "" {
			return json.Marshal(kind)
		}
		return json.Marshal(map[string]string{tag: kind})
	}
	expected := reflect.TypeOf(factory()).Elem()
	actual := reflect.ValueOf(value)
	if !actual.IsValid() {
		if expected.Kind() != reflect.Pointer {
			return nil, fmt.Errorf("missing content for variant %q", kind)
		}
	} else if actual.Type() != expected && actual.Type() != reflect.PointerTo(expected) {
		return nil, fmt.Errorf("invalid content type for variant %q", kind)
	} else if actual.Kind() == reflect.Pointer && actual.IsNil() && expected.Kind() != reflect.Pointer {
		return nil, fmt.Errorf("missing content for variant %q", kind)
	}
	if actual.IsValid() {
		value = normalizeCollectionValue(actual).Interface()
	}
	if tag == "" {
		return json.Marshal(map[string]any{kind: value})
	}
	if content != "" {
		return json.Marshal(map[string]any{tag: kind, content: value})
	}
	data, err := json.Marshal(value)
	if err != nil {
		return nil, err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return nil, err
	}
	if fields == nil {
		fields = map[string]json.RawMessage{}
	}
	encoded, err := json.Marshal(kind)
	if err != nil {
		return nil, err
	}
	fields[tag] = encoded
	return json.Marshal(fields)
}

// checkJSONNulls applies the null, numeric token and fixed-length rules to a
// value that encoding/json decodes, descending by the Go kind of target.
func checkJSONNulls(data []byte, target reflect.Type) error {
	if target == reflect.TypeFor[json.RawMessage]() || target == reflect.TypeFor[Unit]() || target.Kind() == reflect.Interface {
		return nil
	}
	if bytes.Equal(bytes.TrimSpace(data), []byte("null")) {
		if target.Kind() == reflect.Pointer {
			return nil
		}
		return errNull
	}
	if target == reflect.TypeFor[json.Number]() {
		if bytes.TrimSpace(data)[0] == '"' {
			return checkError("expected numeric token")
		}
	}
	switch target.Kind() {
	case reflect.Pointer:
		return checkJSONNulls(data, target.Elem())
	case reflect.Slice, reflect.Array:
		if bytes.TrimSpace(data)[0] != '[' {
			return nil
		}
		var items []json.RawMessage
		if err := json.Unmarshal(data, &items); err != nil {
			return err
		}
		if target.Kind() == reflect.Array && len(items) != target.Len() {
			return checkError(fmt.Sprintf("expected array of %d elements", target.Len()))
		}
		for _, item := range items {
			if err := checkJSONNulls(item, target.Elem()); err != nil {
				return err
			}
		}
	case reflect.Map:
		var items map[string]json.RawMessage
		if err := json.Unmarshal(data, &items); err != nil {
			return err
		}
		for _, item := range items {
			if err := checkJSONNulls(item, target.Elem()); err != nil {
				return err
			}
		}
	}
	return nil
}

func normalizeCollections[T any](value T) T {
	return normalizeCollectionValue(reflect.ValueOf(value)).Interface().(T)
}

func normalizeCollectionValue(value reflect.Value) reflect.Value {
	if value.Type() == reflect.TypeFor[json.RawMessage]() {
		return value
	}
	switch value.Kind() {
	case reflect.Pointer:
		if value.IsNil() {
			return value
		}
		switch value.Type().Elem().Kind() {
		case reflect.Pointer, reflect.Map, reflect.Slice, reflect.Array:
			cloned := reflect.New(value.Type().Elem())
			cloned.Elem().Set(normalizeCollectionValue(value.Elem()))
			return cloned
		}
	case reflect.Slice:
		cloned := reflect.MakeSlice(value.Type(), value.Len(), value.Len())
		for index := range value.Len() {
			cloned.Index(index).Set(normalizeCollectionValue(value.Index(index)))
		}
		return cloned
	case reflect.Array:
		cloned := reflect.New(value.Type()).Elem()
		for index := range value.Len() {
			cloned.Index(index).Set(normalizeCollectionValue(value.Index(index)))
		}
		return cloned
	case reflect.Map:
		cloned := reflect.MakeMapWithSize(value.Type(), value.Len())
		iterator := value.MapRange()
		for iterator.Next() {
			cloned.SetMapIndex(iterator.Key(), normalizeCollectionValue(iterator.Value()))
		}
		return cloned
	}
	return value
}

type Bytes []byte

type Result[Success, Failure any] struct {
	Ok  *Success
	Err *Failure
}

func (value Result[Success, Failure]) MarshalJSON() ([]byte, error) {
	if (value.Ok == nil) == (value.Err == nil) {
		return nil, fmt.Errorf("expected exactly one result variant")
	}
	if value.Ok != nil {
		return json.Marshal(map[string]any{"Ok": value.Ok})
	}
	return json.Marshal(map[string]any{"Err": value.Err})
}

func (value *Result[Success, Failure]) UnmarshalJSON(data []byte) error {
	kind, payload, err := UnmarshalUnion(data, "", "", map[string]func() any{
		"Ok":  func() any { return new(Success) },
		"Err": func() any { return new(Failure) },
	}, false)
	if err != nil {
		return err
	}
	decoded := Result[Success, Failure]{}
	// Tolerant decoding yields an undeclared kind instead of an error; a
	// result has no third variant to carry it.
	switch kind {
	case "Ok":
		decoded.Ok = payload.(*Success)
	case "Err":
		decoded.Err = payload.(*Failure)
	default:
		return fmt.Errorf("unknown result variant %q", kind)
	}
	*value = decoded
	return nil
}

func (value Bytes) MarshalJSON() ([]byte, error) {
	values := make([]uint16, len(value))
	for index, item := range value {
		values[index] = uint16(item)
	}
	return json.Marshal(values)
}

func (value *Bytes) UnmarshalJSON(data []byte) error {
	trimmed := bytes.TrimSpace(data)
	if len(trimmed) == 0 || trimmed[0] != '[' {
		return fmt.Errorf("expected byte array")
	}
	var items []json.RawMessage
	if err := json.Unmarshal(data, &items); err != nil {
		return err
	}
	decoded := make(Bytes, len(items))
	for index, item := range items {
		if bytes.Equal(bytes.TrimSpace(item), []byte("null")) {
			return fmt.Errorf("byte must not be null")
		}
		if err := json.Unmarshal(item, &decoded[index]); err != nil {
			return err
		}
	}
	*value = decoded
	return nil
}
