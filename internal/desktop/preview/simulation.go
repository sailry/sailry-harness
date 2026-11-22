package preview

import (
	"runtime"
	"sync"
	"time"
	"weak"

	"github.com/sailry/sailry-harness/internal/desktop/conversation"
)

type simulationState struct {
	stream             *stream
	older              []conversation.Turn
	paging             time.Time
	queued             int
	statisticsRevision uint64
	// script is the reply a send plays, nil for the preview's (stream.go).
	script *script
}

var simulationMu sync.Mutex
var simulations = map[weak.Pointer[Conversation]]*simulationState{}

// StatisticsRevision identifies the transcript and usage inputs changed by
// the simulator, including in-place changes during a scripted reply.
func StatisticsRevision(c *Conversation) uint64 { return simulation(c).statisticsRevision }

func simulation(c *Conversation) *simulationState {
	simulationMu.Lock()
	defer simulationMu.Unlock()
	key := weak.Make(c)
	state := simulations[key]
	if state == nil {
		state = &simulationState{}
		simulations[key] = state
		runtime.AddCleanup(c, func(key weak.Pointer[Conversation]) {
			simulationMu.Lock()
			delete(simulations, key)
			simulationMu.Unlock()
		}, key)
	}
	runtime.KeepAlive(c)
	return state
}
