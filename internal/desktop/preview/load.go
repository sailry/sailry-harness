package preview

import "time"

// LoadAfter is how long a preview load takes: what the loading state
// shows in place of a fetch the preview does not make.
const LoadAfter = 800 * time.Millisecond

// Fetch is a load the preview plays on the frame clock: Loading from Start
// until LoadAfter after the first frame that advances it. It reads and
// writes nothing.
type Fetch struct {
	// Loading is set while the data is on its way.
	Loading bool
	// Loaded is set once a load finished.
	Loaded bool
	start  time.Time
}

// Start begins a load, again if one runs.
func (l *Fetch) Start() { l.Loading, l.start = true, time.Time{} }

// Advance finishes the load once LoadAfter passed, reporting whether it
// still runs: the view then asks for a frame after Tick.
func (l *Fetch) Advance(now time.Time) bool {
	if !l.Loading {
		return false
	}
	if l.start.IsZero() {
		l.start = now
	}
	if now.Sub(l.start) < LoadAfter {
		return true
	}
	l.Loading, l.Loaded = false, true
	return false
}
