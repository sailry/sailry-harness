package plugins

import "context"

// MemoryBytes estimates the JavaScript heap this view retains, including
// values registered through AddLiveRoots. Shared builtins and the host's Go,
// native and image resources are outside this approximate JS-only account.
// Call it on the VM's owner goroutine, between calls. The bool is false if
// execution is in progress or the measurement was interrupted; a view with
// no live engine has a complete zero measurement.
func (mounted *Mounted) MemoryBytes() (uint64, bool) {
	if mounted == nil || mounted.closed || mounted.suspended || mounted.engine == nil {
		return 0, true
	}
	realm := mounted.engine.Realm()
	if mounted.execution != nil || realm.CallDepth() != 0 {
		return 0, false
	}
	var bytes int64
	complete := false
	err := mounted.execute(desktopFrameBudget, func(context.Context) error {
		if realm.Interrupted() {
			return nil
		}
		bytes = realm.MeasureLive(nil)
		// MeasureLive returns a partial count when interrupted, without an
		// error of its own. Do not publish that count as a complete sample.
		// Keep this a sample failure so execute preserves deferred JS jobs.
		complete = !realm.Interrupted()
		return nil
	})
	if err != nil || !complete {
		return 0, false
	}
	return uint64(bytes), true
}
