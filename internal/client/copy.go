package client

import (
	"reflect"
	"sync"
)

// Protocol values contain nested mutable maps and pointers. Copy them at the
// controller boundary so a presentation cannot mutate the shared projection.
func copyValue[T any](value T) T {
	detachValue(reflect.ValueOf(&value).Elem())
	return value
}

// detachValue starts with an addressable shallow copy. A bulk slice copy, for
// example, already contains all scalar fields of every entry in the transcript.
func detachValue(value reflect.Value) {
	switch value.Kind() {
	case reflect.Pointer, reflect.Interface:
		if value.IsNil() {
			return
		}
		elem := value.Elem()
		if value.Kind() == reflect.Interface && !copyLayoutOf(elem.Type()).mutable {
			return
		}
		copied := reflect.New(elem.Type())
		copied.Elem().Set(elem)
		detachValue(copied.Elem())
		if value.Kind() == reflect.Interface {
			value.Set(copied.Elem())
		} else {
			value.Set(copied)
		}
	case reflect.Slice:
		if value.IsNil() {
			return
		}
		copied := reflect.MakeSlice(value.Type(), value.Len(), value.Len())
		reflect.Copy(copied, value)
		value.Set(copied)
		fallthrough
	case reflect.Array:
		if copyLayoutOf(value.Type().Elem()).mutable {
			for index := range value.Len() {
				detachValue(value.Index(index))
			}
		}
	case reflect.Map:
		if value.IsNil() {
			return
		}
		copied := reflect.MakeMapWithSize(value.Type(), value.Len())
		mutable := copyLayoutOf(value.Type().Elem()).mutable
		for entries := value.MapRange(); entries.Next(); {
			item := entries.Value()
			if mutable {
				cloned := reflect.New(item.Type()).Elem()
				cloned.Set(item)
				detachValue(cloned)
				item = cloned
			}
			copied.SetMapIndex(entries.Key(), item)
		}
		value.Set(copied)
	case reflect.Struct:
		for _, index := range copyLayoutOf(value.Type()).fields {
			detachValue(value.Field(index))
		}
	}
}

type copyLayout struct {
	mutable bool
	fields  []int
}

var copyLayouts sync.Map // reflect.Type to copyLayout

func copyLayoutOf(t reflect.Type) copyLayout {
	if cached, ok := copyLayouts.Load(t); ok {
		return cached.(copyLayout)
	}
	var layout copyLayout
	switch t.Kind() {
	case reflect.Pointer, reflect.Interface, reflect.Map, reflect.Slice:
		layout.mutable = true
	case reflect.Array:
		layout.mutable = copyLayoutOf(t.Elem()).mutable
	case reflect.Struct:
		for index := range t.NumField() {
			field := t.Field(index)
			if field.IsExported() && copyLayoutOf(field.Type).mutable {
				layout.fields = append(layout.fields, index)
			}
		}
		layout.mutable = len(layout.fields) > 0
	}
	copyLayouts.Store(t, layout)
	return layout
}
