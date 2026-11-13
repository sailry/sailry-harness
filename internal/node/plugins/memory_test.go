package plugins

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/MoeForge/moejs"
	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestScriptMemory(t *testing.T) {
	t.Parallel()
	t.Run("idle model samples are opt-in cached and released", func(t *testing.T) {
		clock := clockwork.NewFakeClockAt(time.Date(2026, 11, 14, 0, 0, 0, 0, time.UTC))
		tracker := NewMemoryTracker()
		tracker.now = clock.Now
		pkg := scriptTestEnvironment().Context.Package
		ctx := tracker.Context(t.Context(), pkg, "model")
		session, err := NewTranslation(ctx, scriptTestBundle(`let held=new ArrayBuffer(262144);export function grow(){held=new ArrayBuffer(1048576);return held.byteLength}`))
		if err != nil {
			t.Fatal(err)
		}
		defer session.Close()
		entry := session.runtime.memory
		if entry.bytes != nil {
			t.Fatal("a runtime was measured before diagnostic demand")
		}
		stats := session.runtime.vm.Stats()
		first := tracker.Read()
		if len(first.Samples) != 1 || first.Samples[0].Package != pkg || first.Samples[0].Kind != "model" || first.Samples[0].Instances != 1 || first.Samples[0].Busy || first.Samples[0].Bytes == nil || *first.Samples[0].Bytes < 262144 {
			t.Fatalf("idle model estimate: %+v", first)
		}
		if session.runtime.vm.Realm().MemoryLimit() != ScriptMemoryBytes || !reflect.DeepEqual(stats, session.runtime.vm.Stats()) {
			t.Fatal("diagnostic sampling changed script budgets or allocation counters")
		}
		if _, err := session.Call("grow", nil, ScriptMaxDataBytes); err != nil {
			t.Fatal(err)
		}
		cached := tracker.Read().Samples[0]
		if *cached.Bytes != *first.Samples[0].Bytes || cached.SampledAtMs != first.Samples[0].SampledAtMs {
			t.Fatal("a repeated read walked the runtime before its cache expired")
		}
		clock.Advance(memoryInterval)
		grown := tracker.Read().Samples[0]
		if *grown.Bytes < 1048576 || grown.SampledAtMs <= cached.SampledAtMs {
			t.Fatal("an idle model did not refresh after its cache expired")
		}
		clock.Advance(memoryLease + time.Second)
		at := entry.at
		if _, err := session.Call("grow", nil, ScriptMaxDataBytes); err != nil {
			t.Fatal(err)
		}
		if entry.at != at {
			t.Fatal("a closed diagnostic lease still sampled owner boundaries")
		}
		session.Close()
		if got := tracker.Read(); len(got.Samples) != 0 {
			t.Fatalf("closed runtime remained: %+v", got)
		}
	})
	t.Run("busy tools and callbacks keep their safe sample without blocking reads", func(t *testing.T) {
		for _, kind := range []string{"tool", "callback"} {
			t.Run(kind, func(t *testing.T) {
				clock := clockwork.NewFakeClockAt(time.Date(2026, 11, 14, 0, 0, 0, 0, time.UTC))
				tracker := NewMemoryTracker()
				tracker.now = clock.Now
				tracker.Read()
				environment := scriptTestEnvironment()
				entered, release := make(chan struct{}), make(chan struct{})
				done := make(chan error, 1)
				ctx := tracker.Context(t.Context(), environment.Context.Package, kind)
				go func() {
					bundle := scriptTestBundle(`import {getValue} from 'sailry/sdk';const held=new ArrayBuffer(262144);export async function run(){return {size:held.byteLength,result:await getValue('pause')}}`)
					dispatch := func(context.Context, protocol.Request) (protocol.Output, error) {
						close(entered)
						<-release
						return protocol.Output{Kind: "plugin_value", Data: protocol.PluginStorageEntry{Key: "pause", Present: true, Revision: 1, Value: json.RawMessage("null")}}, nil
					}
					var err error
					if kind == "tool" {
						_, err = RunTool(ctx, bundle, "run", nil, environment, dispatch, &ScriptExecution{Mode: protocol.WorkModeCode})
					} else {
						_, err = RunCallback(ctx, bundle, "run", nil, environment, false, dispatch)
					}
					done <- err
				}()
				select {
				case <-entered:
				case err := <-done:
					t.Fatalf("callback ended before its SDK boundary: %v", err)
				}
				first := tracker.Read()
				clock.Advance(memoryInterval)
				second := tracker.Read()
				close(release)
				if err := <-done; err != nil {
					t.Fatal(err)
				}
				if len(first.Samples) != 1 || len(second.Samples) != 1 {
					t.Fatalf("busy runtime disappeared: %+v %+v", first, second)
				}
				a, b := first.Samples[0], second.Samples[0]
				if a.Package != environment.Context.Package || a.Kind != kind || !a.Busy || a.Bytes == nil || *a.Bytes < 262144 || !reflect.DeepEqual(a, b) {
					t.Fatalf("busy runtime sample was lost or walked concurrently: %+v %+v", a, b)
				}
				if len(tracker.Read().Samples) != 0 {
					t.Fatal("completed callback remained active")
				}
			})
		}
	})
	t.Run("busy models and captured revisions stay distinct", func(t *testing.T) {
		tracker, other := NewMemoryTracker(), NewMemoryTracker()
		pkg := scriptTestEnvironment().Context.Package
		var sessions []*Translation
		for _, digest := range []string{strings.Repeat("b", 64), strings.Repeat("a", 64)} {
			pkg.Digest = digest
			for range 2 {
				session, err := NewTranslation(tracker.Context(t.Context(), pkg, "model"), scriptTestBundle(`export function run(){return null}`))
				if err != nil {
					t.Fatal(err)
				}
				defer session.Close()
				sessions = append(sessions, session)
			}
		}
		locked := sessions[0]
		locked.mu.Lock()
		locked.runtime.memory.executing(true)
		first := tracker.Read()
		locked.runtime.memory.executing(false)
		locked.mu.Unlock()
		if len(first.Samples) != 2 || first.Samples[0].Package.Digest != strings.Repeat("a", 64) || first.Samples[0].Instances != 2 || first.Samples[0].Bytes == nil || first.Samples[1].Instances != 2 || first.Samples[1].Bytes != nil || !first.Samples[1].Busy || first.Samples[1].SampledAtMs != 0 {
			t.Fatalf("captured groups or pending state changed: %+v", first)
		}
		if len(other.Read().Samples) != 0 {
			t.Fatal("per-Node trackers shared instances")
		}
		second := tracker.Read()
		if second.Samples[1].Bytes == nil || second.Samples[1].Busy {
			t.Fatal("an idle model kept a busy or unknown sample")
		}
	})
	t.Run("failed creation and interrupted walks do not publish partial data", func(t *testing.T) {
		clock := clockwork.NewFakeClockAt(time.Date(2026, 11, 14, 0, 0, 0, 0, time.UTC))
		tracker := NewMemoryTracker()
		tracker.now = clock.Now
		tracker.Read()
		ctx := tracker.Context(t.Context(), scriptTestEnvironment().Context.Package, "model")
		if _, err := NewTranslation(ctx, scriptTestBundle(`throw new Error('Fixture failure')`)); err == nil {
			t.Fatal("expected initialization failure")
		}
		if len(tracker.Read().Samples) != 0 {
			t.Fatal("failed construction leaked a runtime")
		}
		session, err := NewTranslation(ctx, scriptTestBundle(`export function run(){return null}`))
		if err != nil {
			t.Fatal(err)
		}
		defer session.Close()
		entry := session.runtime.memory
		previous, at := *entry.bytes, entry.at
		clock.Advance(memoryInterval)
		remove := session.runtime.vm.AddLiveRoots(func(func(moejs.Value)) { session.runtime.vm.Interrupt(errors.New("Fixture interruption")) })
		session.runtime.measureMemory()
		remove()
		if *entry.bytes != previous || entry.at != at {
			t.Fatal("an interrupted walk replaced the last complete sample")
		}
		session.runtime.vm.ClearInterrupt()
		session.runtime.measureMemory()
		if !entry.at.After(at) {
			t.Fatal("sampling did not resume after interruption cleared")
		}
	})
}
