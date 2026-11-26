// Package memory exposes process-wide cache reclamation hooks for modules
// and downstream products built on Sailry Harness.
package memory

import internalmemory "github.com/sailry/sailry-harness/internal/memory"

// Level describes how aggressively registered caches should reclaim memory.
type Level = internalmemory.Level

const (
	// Idle runs on periodic background ticks to evict expired or inactive entries.
	Idle = internalmemory.Idle
	// Pressure runs when Go heap metrics cross high-water thresholds.
	Pressure = internalmemory.Pressure
)

// Register attaches a trim callback to the process-wide memory Governor and
// returns an idempotent unregister function.
func Register(name string, trim func(Level)) func() {
	return internalmemory.Default().Register(name, trim)
}

// Trim invokes all registered callbacks at the requested reclamation level.
func Trim(level Level) {
	internalmemory.Default().Trim(level)
}

// CheckPressure samples Go heap metrics and runs Trim(Pressure) when heap
// usage exceeds high-water thresholds outside the cooldown window.
func CheckPressure() bool {
	return internalmemory.Default().CheckPressure()
}
