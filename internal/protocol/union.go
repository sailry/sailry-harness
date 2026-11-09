package protocol

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"reflect"
	"strings"
)

type variantFactories map[string]func() any

func MarshalUnion(tag, content, kind string, value any, variants map[string]func() any) ([]byte, error) {
	factory, exists := variants[kind]
	if !exists {
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

func UnmarshalUnion(data []byte, tag, content string, variants map[string]func() any, denyUnknown bool) (string, any, error) {
	var fields map[string]json.RawMessage
	var kind string
	var payload json.RawMessage
	if tag == "" && len(bytes.TrimSpace(data)) > 0 && bytes.TrimSpace(data)[0] == '"' {
		if err := json.Unmarshal(data, &kind); err != nil {
			return "", nil, err
		}
	} else {
		entries, decoded, err := readObject(data)
		if err != nil {
			return "", nil, err
		}
		fields = decoded
		if tag == "" {
			if len(entries) != 1 {
				return "", nil, fmt.Errorf("expected one external variant")
			}
			for name, value := range fields {
				kind, payload = name, value
			}
		} else {
			if err := rejectDuplicateFields(entries, map[string]bool{tag: true, content: content != ""}); err != nil {
				return "", nil, err
			}
			encoded, present := fields[tag]
			if !present {
				return "", nil, fmt.Errorf("missing field %q", tag)
			}
			if err := json.Unmarshal(encoded, &kind); err != nil {
				return "", nil, err
			}
			delete(fields, tag)
			if content != "" {
				payload = fields[content]
				delete(fields, content)
				if denyUnknown && len(fields) > 0 {
					return "", nil, fmt.Errorf("unknown enum field")
				}
			} else {
				payload = []byte{'{'}
				for _, entry := range entries {
					if entry.name == tag {
						continue
					}
					if len(payload) > 1 {
						payload = append(payload, ',')
					}
					name, _ := json.Marshal(entry.name)
					payload = append(payload, name...)
					payload = append(payload, ':')
					payload = append(payload, entry.value...)
				}
				payload = append(payload, '}')
			}
		}
	}
	factory, exists := variants[kind]
	if !exists {
		return "", nil, fmt.Errorf("unknown variant %q", kind)
	}
	if factory == nil {
		if tag != "" && content == "" && denyUnknown && len(fields) > 0 {
			return "", nil, fmt.Errorf("unknown unit variant field")
		}
		if (tag == "" || content != "") && len(payload) > 0 && !bytes.Equal(bytes.TrimSpace(payload), []byte("null")) {
			return "", nil, fmt.Errorf("expected unit variant")
		}
		return kind, nil, nil
	}
	if len(payload) == 0 {
		return "", nil, fmt.Errorf("missing variant content")
	}
	value := factory()
	if err := checkJSONNulls(payload, reflect.TypeOf(value).Elem()); err != nil {
		return "", nil, err
	}
	if err := json.Unmarshal(payload, value); err != nil {
		return "", nil, err
	}
	return kind, value, nil
}

func DecodeObject(data []byte, target any, required []string, denyUnknown bool) error {
	entries, fields, err := readObject(data)
	if err != nil {
		return err
	}
	for _, name := range required {
		_, present := fields[name]
		if !present {
			return fmt.Errorf("missing field %q", name)
		}
	}
	targetType := reflect.TypeOf(target).Elem()
	known := make(map[string]bool, targetType.NumField())
	for index := 0; index < targetType.NumField(); index++ {
		field := targetType.Field(index)
		name := strings.Split(field.Tag.Get("json"), ",")[0]
		if name == "-" || field.PkgPath != "" {
			continue
		}
		if name == "" {
			name = field.Name
		}
		known[name] = true
		value, present := fields[name]
		if !present {
			continue
		}
		if err := checkJSONNulls(value, field.Type); err != nil {
			return fmt.Errorf("field %q: %w", name, err)
		}
		if field.Type.Kind() == reflect.Pointer && bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
			for _, key := range required {
				if key != name {
					continue
				}
				return fmt.Errorf("field %q must not be null", name)
			}
		}
	}
	if err := rejectDuplicateFields(entries, known); err != nil {
		return err
	}
	for name := range fields {
		if !known[name] {
			if denyUnknown {
				return fmt.Errorf("unknown field %q", name)
			}
			delete(fields, name)
		}
	}
	filtered, err := json.Marshal(fields)
	if err != nil {
		return err
	}
	if denyUnknown {
		decoder := json.NewDecoder(bytes.NewReader(filtered))
		decoder.DisallowUnknownFields()
		return decoder.Decode(target)
	}
	return json.Unmarshal(filtered, target)
}

func checkJSONNulls(data []byte, target reflect.Type) error {
	if target == reflect.TypeFor[json.RawMessage]() || target == reflect.TypeFor[Unit]() || target.Kind() == reflect.Interface {
		return nil
	}
	if bytes.Equal(bytes.TrimSpace(data), []byte("null")) {
		if target.Kind() == reflect.Pointer {
			return nil
		}
		return fmt.Errorf("value must not be null")
	}
	if target == reflect.TypeFor[json.Number]() {
		if bytes.TrimSpace(data)[0] == '"' {
			return fmt.Errorf("expected numeric token")
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
			return fmt.Errorf("expected array of %d elements", target.Len())
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

type objectField struct {
	name  string
	value json.RawMessage
}

func readObject(data []byte) ([]objectField, map[string]json.RawMessage, error) {
	decoder := json.NewDecoder(bytes.NewReader(data))
	start, err := decoder.Token()
	if err != nil {
		return nil, nil, err
	}
	if start != json.Delim('{') {
		return nil, nil, fmt.Errorf("expected object")
	}
	var entries []objectField
	fields := make(map[string]json.RawMessage)
	for decoder.More() {
		token, err := decoder.Token()
		if err != nil {
			return nil, nil, err
		}
		name, ok := token.(string)
		if !ok {
			return nil, nil, fmt.Errorf("expected field name")
		}
		var value json.RawMessage
		if err := decoder.Decode(&value); err != nil {
			return nil, nil, err
		}
		entries = append(entries, objectField{name, value})
		fields[name] = value
	}
	if _, err := decoder.Token(); err != nil {
		return nil, nil, err
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return nil, nil, fmt.Errorf("unexpected data after object")
	}
	return entries, fields, nil
}

func rejectDuplicateFields(entries []objectField, known map[string]bool) error {
	seen := make(map[string]bool)
	for _, entry := range entries {
		if !known[entry.name] {
			continue
		}
		if seen[entry.name] {
			return fmt.Errorf("duplicate field %q", entry.name)
		}
		seen[entry.name] = true
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
	if kind == "Ok" {
		decoded.Ok = payload.(*Success)
	} else {
		decoded.Err = payload.(*Failure)
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
