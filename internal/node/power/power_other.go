//go:build !darwin && !windows

package power

// Other systems have no idle-sleep request, as in the reference.
func native() Backend { return unsupported{} }
