package logging

// Windows log directories are validated by their DACL, which no earlier
// release left permissive, so there is nothing to tighten.
func tightenDirectory(string) error { return nil }
