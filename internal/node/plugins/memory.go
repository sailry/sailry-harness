package plugins

import (
	"cmp"
	"context"
	"slices"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	memoryInterval = 5 * time.Second
	// A diagnostic reader renews this lease. No timer or background walk
	// remains when it leaves; owner boundaries stop sampling after it ends.
	memoryLease = 2 * memoryInterval
)

// MemoryTracker belongs to one Node. It tracks real JavaScript lifetimes,
// not installed packages, and publishes retained-heap estimates separately
// from Go/native memory. Only a VM's owner, or an idle Translation holding
// its existing mutex, may walk that VM; readers never inspect a busy VM.
type MemoryTracker struct {
	mu        sync.Mutex
	now       func() time.Time
	until     time.Time
	instances map[*memoryInstance]struct{}
}

type memoryScope struct {
	tracker    *MemoryTracker
	packageRef protocol.PluginReference
	kind       string
}

type memoryContextKey struct{}

type memoryInstance struct {
	tracker    *MemoryTracker
	packageRef protocol.PluginReference
	kind       string
	// These fields are protected by tracker.mu. idle takes a Translation's
	// mutex with TryLock before it reads the engine, never waiting for JS.
	idle  func()
	bytes *uint64
	at    time.Time
	busy  bool
}

func NewMemoryTracker() *MemoryTracker {
	return &MemoryTracker{now: time.Now, instances: map[*memoryInstance]struct{}{}}
}

// Context carries the captured package identity to a VM constructor. It
// changes neither the script's authority nor its cancellation and budgets.
func (tracker *MemoryTracker) Context(ctx context.Context, pkg protocol.PluginReference, kind string) context.Context {
	if tracker == nil {
		return ctx
	}
	return context.WithValue(ctx, memoryContextKey{}, memoryScope{tracker: tracker, packageRef: pkg, kind: kind})
}

func (runtime *scriptVM) attachMemory() {
	scope, exists := runtime.ctx.Value(memoryContextKey{}).(memoryScope)
	if !exists {
		return
	}
	entry := &memoryInstance{tracker: scope.tracker, packageRef: scope.packageRef, kind: scope.kind, busy: true}
	scope.tracker.mu.Lock()
	scope.tracker.instances[entry] = struct{}{}
	scope.tracker.mu.Unlock()
	runtime.memory = entry
}

func (entry *memoryInstance) executing(busy bool) {
	if entry == nil {
		return
	}
	entry.tracker.mu.Lock()
	entry.busy = busy
	entry.tracker.mu.Unlock()
}

func (runtime *scriptVM) memoryIdle() {
	entry := runtime.memory
	if entry == nil {
		return
	}
	entry.executing(false)
	runtime.measureMemory()
}

// measureMemory runs between calls on the VM's owner. MeasureLive changes
// no allocation budget and includes our registered roots. An interruption
// returns a partial walk, so it must not replace the last complete sample.
func (runtime *scriptVM) measureMemory() {
	entry := runtime.memory
	if entry == nil || runtime.vm == nil || runtime.ctx.Err() != nil {
		return
	}
	tracker := entry.tracker
	tracker.mu.Lock()
	now := tracker.now()
	_, alive := tracker.instances[entry]
	due := alive && now.Before(tracker.until) && (entry.bytes == nil || now.Sub(entry.at) >= memoryInterval)
	tracker.mu.Unlock()
	if !due || runtime.vm.Realm().Interrupted() {
		return
	}
	bytes := runtime.vm.Realm().MeasureLive(nil)
	if runtime.ctx.Err() != nil || runtime.vm.Realm().Interrupted() {
		return
	}
	value := uint64(bytes)
	tracker.mu.Lock()
	if _, alive := tracker.instances[entry]; alive {
		entry.bytes, entry.at = &value, tracker.now()
	}
	tracker.mu.Unlock()
}

func (runtime *scriptVM) detachMemory() {
	if entry := runtime.memory; entry != nil {
		entry.tracker.mu.Lock()
		delete(entry.tracker.instances, entry)
		entry.idle = nil
		entry.tracker.mu.Unlock()
		runtime.memory = nil
	}
}

// Read renews diagnostic demand and samples idle translations without
// waiting for executing JavaScript. A busy instance retains its older
// estimate or nil; a call starting during a walk uses the existing mutex.
func (tracker *MemoryTracker) Read() *protocol.PluginMemorySnapshot {
	tracker.mu.Lock()
	tracker.until = tracker.now().Add(memoryLease)
	idle := make([]func(), 0, len(tracker.instances))
	for entry := range tracker.instances {
		if entry.idle != nil {
			idle = append(idle, entry.idle)
		}
	}
	tracker.mu.Unlock()
	for _, sample := range idle {
		sample()
	}

	tracker.mu.Lock()
	defer tracker.mu.Unlock()
	type key struct {
		pkg  protocol.PluginReference
		kind string
	}
	groups := map[key]*protocol.PluginMemorySample{}
	for entry := range tracker.instances {
		id := key{entry.packageRef, entry.kind}
		sample := groups[id]
		if sample == nil {
			sample = &protocol.PluginMemorySample{Package: entry.packageRef, Kind: entry.kind, Bytes: new(uint64)}
			groups[id] = sample
		}
		sample.Instances++
		sample.Busy = sample.Busy || entry.busy
		if entry.bytes == nil {
			sample.Bytes = nil
		} else if sample.Bytes != nil {
			*sample.Bytes += *entry.bytes
		}
		at := uint64(max(0, entry.at.UnixMilli()))
		if sample.Instances == 1 || at < sample.SampledAtMs {
			sample.SampledAtMs = at
		}
	}
	result := &protocol.PluginMemorySnapshot{Samples: make([]protocol.PluginMemorySample, 0, len(groups)), CapturedAtMs: uint64(max(0, tracker.now().UnixMilli()))}
	for _, sample := range groups {
		result.Samples = append(result.Samples, *sample)
	}
	slices.SortFunc(result.Samples, func(a, b protocol.PluginMemorySample) int {
		if order := cmp.Compare(a.Package.Name, b.Package.Name); order != 0 {
			return order
		}
		if order := cmp.Compare(a.Package.Digest, b.Package.Digest); order != 0 {
			return order
		}
		if order := cmp.Compare(a.Package.SettingsRevision, b.Package.SettingsRevision); order != 0 {
			return order
		}
		return cmp.Compare(a.Kind, b.Kind)
	})
	return result
}
