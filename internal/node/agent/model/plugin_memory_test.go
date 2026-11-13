package model

import (
	"context"
	"io"
	"net/http"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginModelMemory(t *testing.T) {
	t.Run("stream lifetime uses the captured package", func(t *testing.T) {
		for _, finish := range []string{"complete", "cancel"} {
			t.Run(finish, func(t *testing.T) {
				tracker := plugins.NewMemoryTracker()
				var during *protocol.PluginMemorySnapshot
				model, binding, _ := pluginFixture(t, `
const held=new ArrayBuffer(262144);
export function prepare(input){return {method:"POST",url:input.endpoint+"/messages",body:JSON.stringify({size:held.byteLength})}}
export function decode(input){return {state:null,deltas:input.events.map(event=>JSON.parse(event.data))}}
`, func(*http.Request) (*http.Response, error) {
					during = tracker.Read()
					return &http.Response{StatusCode: 200, Header: http.Header{"Content-Type": []string{"text/event-stream"}},
						Body: io.NopCloser(strings.NewReader("data: {\"kind\":\"text\",\"text\":\"Hello\"}\n\ndata: {\"kind\":\"finish\",\"reason\":\"stop\"}\n\n"))}, nil
				})
				model.capture.Memory = tracker
				(**binding.Provider.Binding).Package.SettingsRevision = 9007199254740993
				expected := *(**binding.Provider.Binding).Package
				if got := tracker.Read(); len(got.Samples) != 0 {
					t.Fatal("capturing a model mounted its JavaScript")
				}
				call, err := BuildCall(binding, Request{System: "captured"})
				if err != nil {
					t.Fatal(err)
				}
				ctx, cancel := context.WithCancel(t.Context())
				defer cancel()
				stream, err := model.Stream(ctx, call)
				if err != nil {
					t.Fatal(err)
				}
				count := 0
				for part := range stream {
					if part.Type == fantasy.StreamPartTypeError {
						t.Fatal(part.Error)
					}
					count++
					if finish == "cancel" {
						cancel()
						break
					}
				}
				if count == 0 || during == nil || len(during.Samples) != 1 {
					t.Fatalf("stream did not publish its active runtime: %d %+v", count, during)
				}
				sample := during.Samples[0]
				if sample.Package != expected || sample.Kind != "model" || sample.Instances != 1 || sample.Busy || sample.Bytes == nil || *sample.Bytes < 262144 || sample.SampledAtMs == 0 {
					t.Fatalf("stream lost its exact package or safe estimate: %+v", sample)
				}
				if got := tracker.Read(); len(got.Samples) != 0 {
					t.Fatalf("closed stream kept its runtime: %+v", got)
				}
			})
		}
	})
	t.Run("discovery releases its captured runtime", func(t *testing.T) {
		tracker := plugins.NewMemoryTracker()
		var during *protocol.PluginMemorySnapshot
		_, binding, capture := pluginFixture(t, `
export function prepare_discovery(input){return {method:"GET",url:input.endpoint+"/models"}}
export function decode_discovery(input){return JSON.parse(input.body)}
`, func(*http.Request) (*http.Response, error) {
			during = tracker.Read()
			return &http.Response{StatusCode: 200, Header: http.Header{}, Body: io.NopCloser(strings.NewReader(`{"endpoint":"https://example.test/v1","models":[{"id":"fixture"}]}`))}, nil
		})
		capture.Memory = tracker
		capture.Declaration.Discovery = true
		(**binding.Provider.Binding).Package.SettingsRevision = 7
		expected := *(**binding.Provider.Binding).Package
		if _, err := DiscoverPlugin(t.Context(), binding, capture); err != nil {
			t.Fatal(err)
		}
		if during == nil || len(during.Samples) != 1 || during.Samples[0].Package != expected || during.Samples[0].Kind != "model" || during.Samples[0].Bytes == nil {
			t.Fatalf("discovery lost its captured runtime: %+v", during)
		}
		if got := tracker.Read(); len(got.Samples) != 0 {
			t.Fatalf("completed discovery kept its runtime: %+v", got)
		}
	})
}
