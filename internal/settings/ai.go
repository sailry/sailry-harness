package settings

// enabledOpacity fades disabled rows as the providers list does.
func enabledOpacity(on bool) float32 {
	if on {
		return 1
	}
	return 0.45
}
