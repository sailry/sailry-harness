package protocol

func DataAs[T any](data any) (T, bool) {
	if value, ok := data.(T); ok {
		return value, true
	}
	if value, ok := data.(*T); ok && value != nil {
		return *value, true
	}
	var zero T
	return zero, false
}
