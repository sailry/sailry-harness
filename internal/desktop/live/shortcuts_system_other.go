//go:build !darwin

package live

// The UI Automation and X11 selection readers are not ported.
func readSelection() (string, string) { return "", "selection_unavailable" }
