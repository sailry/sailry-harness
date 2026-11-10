//go:build !darwin

package computer

// Trampoline is a macOS concern: other systems grant nothing per application.
func Trampoline() {}

// Disclaim leaves a helper launch unchanged outside macOS.
func Disclaim(command string, args []string) (string, []string) { return command, args }
