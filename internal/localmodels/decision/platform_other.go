//go:build !darwin && !linux

package decision

// baseline has no locked operating system minimum on this target.
func baseline(*[2]uint32) error { return nil }
