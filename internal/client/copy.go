package client

import "reflect"

// Protocol values contain nested mutable maps and pointers. Copy them at the
// controller boundary so a presentation cannot mutate the shared projection.
func copyValue[T any](value T) T {
	return copyReflect(reflect.ValueOf(value)).Interface().(T)
}

func copyReflect(value reflect.Value) reflect.Value {
	switch value.Kind() {
	case reflect.Pointer:
		if value.IsNil() {
			return value
		}
		copied := reflect.New(value.Type().Elem())
		copied.Elem().Set(copyReflect(value.Elem()))
		return copied
	case reflect.Interface:
		if value.IsNil() {
			return value
		}
		copied := reflect.New(value.Type()).Elem()
		copied.Set(copyReflect(value.Elem()))
		return copied
	case reflect.Slice:
		if value.IsNil() {
			return value
		}
		copied := reflect.MakeSlice(value.Type(), value.Len(), value.Len())
		for index := 0; index < value.Len(); index++ {
			copied.Index(index).Set(copyReflect(value.Index(index)))
		}
		return copied
	case reflect.Map:
		if value.IsNil() {
			return value
		}
		copied := reflect.MakeMapWithSize(value.Type(), value.Len())
		for entries := value.MapRange(); entries.Next(); {
			copied.SetMapIndex(entries.Key(), copyReflect(entries.Value()))
		}
		return copied
	case reflect.Struct:
		copied := reflect.New(value.Type()).Elem()
		copied.Set(value)
		for index := 0; index < value.NumField(); index++ {
			if value.Type().Field(index).PkgPath == "" {
				copied.Field(index).Set(copyReflect(value.Field(index)))
			}
		}
		return copied
	default:
		return value
	}
}
