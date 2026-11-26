// Package memory coordinates process-wide cache trimming and pressure sweeps
// without replacing Go's runtime garbage collection policy or setting GOMEMLIMIT.
package memory

import (
	"runtime/metrics"
	"sync"
	"time"
)

// Level describes how aggressively registered caches should reclaim memory.
type Level int

const (
	// Idle runs on periodic background ticks to evict expired or inactive entries.
	Idle Level = iota
	// Pressure runs when Go heap metrics cross high-water thresholds.
	Pressure
)

const (
	defaultInterval  = 2 * time.Minute
	pressureCooldown = 15 * time.Second
	highLiveBytes    = 384 << 20
	liveGrowthBytes  = 96 << 20
)

// Snapshot reports the latest state observed by a Governor.
type Snapshot struct {
	Registered    int
	LastLevel     Level
	LastTrim      time.Time
	LastHeapBytes uint64
	LastLiveBytes uint64
}

type entry struct {
	name string
	trim func(Level)
}

// Governor tracks process-wide reclaimable caches and coordinates periodic
// idle sweeps alongside heap-pressure trimming.
type Governor struct {
	mu           sync.Mutex
	nextID       uint64
	entries      map[uint64]entry
	refs         int
	stopCh       chan struct{}
	doneCh       chan struct{}
	now          func() time.Time
	interval     time.Duration
	cooldown     time.Duration
	lastPressure time.Time
	pressureLive uint64
	lastLevel    Level
	lastTrim     time.Time
	lastHeap     uint64
	lastLive     uint64
}

var defaultGovernor = NewGovernor()

// Default returns the process-wide memory Governor.
func Default() *Governor {
	return defaultGovernor
}

// NewGovernor constructs an isolated Governor. Tests use dedicated instances to
// exercise registration and pressure triggers without disturbing Default.
func NewGovernor() *Governor {
	return &Governor{
		entries:  map[uint64]entry{},
		now:      time.Now,
		interval: defaultInterval,
		cooldown: pressureCooldown,
	}
}

// Register adds a trim callback to the Governor and returns an idempotent
// unregister function. Callbacks run without holding the Governor lock.
func (g *Governor) Register(name string, trim func(Level)) func() {
	if g == nil || trim == nil {
		return func() {}
	}
	g.mu.Lock()
	g.nextID++
	id := g.nextID
	if g.entries == nil {
		g.entries = map[uint64]entry{}
	}
	g.entries[id] = entry{name: name, trim: trim}
	g.mu.Unlock()

	var once sync.Once
	return func() {
		once.Do(func() {
			g.mu.Lock()
			delete(g.entries, id)
			g.mu.Unlock()
		})
	}
}

// Trim invokes every registered callback at the requested reclamation level.
func (g *Governor) Trim(level Level) {
	if g == nil {
		return
	}
	callbacks := g.snapshotCallbacks(level, g.now())
	for _, fn := range callbacks {
		fn(level)
	}
}

func (g *Governor) snapshotCallbacks(level Level, at time.Time) []func(Level) {
	g.mu.Lock()
	defer g.mu.Unlock()
	g.lastLevel = level
	g.lastTrim = at
	callbacks := make([]func(Level), 0, len(g.entries))
	for _, item := range g.entries {
		callbacks = append(callbacks, item.trim)
	}
	return callbacks
}

// CheckPressure samples Go heap object and live-heap metrics and runs
// Trim(Pressure) when retained memory crosses its high-water mark.
func (g *Governor) CheckPressure() bool {
	if g == nil {
		return false
	}
	samples := [...]metrics.Sample{
		{Name: "/memory/classes/heap/objects:bytes"},
		{Name: "/gc/heap/live:bytes"},
	}
	metrics.Read(samples[:])
	heap := samples[0].Value.Uint64()
	live := samples[1].Value.Uint64()
	return g.checkPressure(heap, live, g.now())
}

func (g *Governor) checkPressure(heap, live uint64, now time.Time) bool {
	g.mu.Lock()
	g.lastHeap = heap
	g.lastLive = live
	// Heap objects include garbage allocated since the last collection. Only
	// the marked live set shows whether retained memory is still growing.
	g.pressureLive = min(g.pressureLive, live)
	triggered := live > highLiveBytes && live-g.pressureLive >= liveGrowthBytes
	if !triggered || (!g.lastPressure.IsZero() && now.Sub(g.lastPressure) < g.cooldown) {
		g.mu.Unlock()
		return false
	}
	g.pressureLive = live
	g.lastPressure = now
	g.lastLevel = Pressure
	g.lastTrim = now
	callbacks := make([]func(Level), 0, len(g.entries))
	for _, item := range g.entries {
		callbacks = append(callbacks, item.trim)
	}
	g.mu.Unlock()

	for _, fn := range callbacks {
		fn(Pressure)
	}
	return true
}

// Start reference-counts the background idle/pressure sweep goroutine and
// returns an idempotent stop function that waits for the worker when the last
// reference is released.
func (g *Governor) Start() func() {
	if g == nil {
		return func() {}
	}
	g.mu.Lock()
	if g.refs == 0 {
		stopCh := make(chan struct{})
		doneCh := make(chan struct{})
		interval := g.interval
		if interval <= 0 {
			interval = defaultInterval
		}
		g.stopCh = stopCh
		g.doneCh = doneCh
		go g.run(interval, stopCh, doneCh)
	}
	g.refs++
	g.mu.Unlock()

	var once sync.Once
	return func() {
		once.Do(func() {
			g.mu.Lock()
			if g.refs == 0 {
				g.mu.Unlock()
				return
			}
			g.refs--
			var stopCh chan struct{}
			var doneCh chan struct{}
			if g.refs == 0 {
				stopCh, doneCh = g.stopCh, g.doneCh
				g.stopCh, g.doneCh = nil, nil
			}
			g.mu.Unlock()
			if stopCh != nil {
				close(stopCh)
				<-doneCh
			}
		})
	}
}

func (g *Governor) run(interval time.Duration, stopCh <-chan struct{}, doneCh chan<- struct{}) {
	defer close(doneCh)
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-stopCh:
			return
		case <-ticker.C:
			if !g.CheckPressure() {
				g.Trim(Idle)
			}
		}
	}
}

// Snapshot returns a copy of the Governor's current diagnostics.
func (g *Governor) Snapshot() Snapshot {
	if g == nil {
		return Snapshot{}
	}
	g.mu.Lock()
	defer g.mu.Unlock()
	return Snapshot{
		Registered:    len(g.entries),
		LastLevel:     g.lastLevel,
		LastTrim:      g.lastTrim,
		LastHeapBytes: g.lastHeap,
		LastLiveBytes: g.lastLive,
	}
}
