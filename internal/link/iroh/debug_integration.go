//go:build integration

package iroh

type Counts struct {
	Endpoints   uintptr `json:"endpoints"`
	Connections uintptr `json:"connections"`
	Sends       uintptr `json:"sends"`
	Receives    uintptr `json:"receives"`
	Operations  uintptr `json:"operations"`
	Callbacks   int     `json:"callbacks"`
}

func DebugCounts() Counts {
	functions := loaded.Load()
	if functions == nil {
		return Counts{}
	}
	counts := Counts{
		Endpoints:   functions.debugCount(0),
		Connections: functions.debugCount(1),
		Sends:       functions.debugCount(2),
		Receives:    functions.debugCount(3),
		Operations:  functions.debugCount(4),
	}
	pendingMutex.Lock()
	counts.Callbacks = len(pending)
	pendingMutex.Unlock()
	return counts
}
