package protocol

import (
	"encoding/json"
	"reflect"
	"strings"
	"testing"
)

func TestPluginMemory(t *testing.T) {
	t.Parallel()
	t.Run("read command is typed and read-only", func(t *testing.T) {
		value := Command{Kind: "read_plugin_memory", Data: CommandReadPluginMemoryPayload{}}
		body, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		var command Command
		if err := json.Unmarshal(body, &command); err != nil {
			t.Fatal(err)
		}
		if command.Kind != value.Kind || command.Durable() {
			t.Fatalf("invalid diagnostic command: %+v", command)
		}
		if _, ok := command.Data.(*CommandReadPluginMemoryPayload); !ok {
			t.Fatalf("unexpected payload %T", command.Data)
		}
		for _, mode := range []Tolerance{Authored, PeerCommand} {
			err := DecodeAs(mode, []byte(`{"kind":"read_plugin_memory","data":{"mount_inactive":true}}`), &command)
			if (err != nil) != (mode == Authored) {
				t.Fatalf("unknown argument tolerance %v: %v", mode, err)
			}
		}
	})
	t.Run("pending and zero are distinct across typed output", func(t *testing.T) {
		zero := uint64(0)
		value := &PluginMemorySnapshot{CapturedAtMs: 20, Samples: []PluginMemorySample{
			{Package: PluginReference{Name: "fixture", Digest: strings.Repeat("a", 64), SettingsRevision: 9007199254740993}, Kind: "callback", Bytes: nil, Instances: 1, Busy: true},
			{Package: PluginReference{Name: "fixture", Digest: strings.Repeat("b", 64)}, Kind: "model", Bytes: &zero, SampledAtMs: 10, Instances: 2},
		}}
		body, err := json.Marshal(Output{Kind: "plugin_memory", Data: value})
		if err != nil {
			t.Fatal(err)
		}
		var output Output
		if err := json.Unmarshal(body, &output); err != nil {
			t.Fatal(err)
		}
		got, ok := output.Data.(*PluginMemorySnapshot)
		if output.Kind != "plugin_memory" || !ok || !reflect.DeepEqual(got, value) {
			t.Fatalf("diagnostic output changed: %#v", output)
		}
		encoded, err := json.Marshal(DefaultPluginMemorySnapshot())
		if err != nil || string(encoded) != `{"samples":[],"captured_at_ms":0}` {
			t.Fatalf("empty sample encoded as %s: %v", encoded, err)
		}
	})
	t.Run("peer data may add fields", func(t *testing.T) {
		for _, body := range []string{
			`{"samples":[],"captured_at_ms":1,"future_metric":true}`,
			`{"samples":[{"package":{"name":"fixture","digest":"` + strings.Repeat("a", 64) + `","settings_revision":0},"kind":"model","bytes":null,"sampled_at_ms":0,"instances":1,"busy":true,"future_metric":true}],"captured_at_ms":1}`,
		} {
			var value PluginMemorySnapshot
			if err := DecodeAs(PeerData, []byte(body), &value); err != nil {
				t.Fatal(err)
			}
			if err := json.Unmarshal([]byte(body), &value); err == nil {
				t.Fatal("authored data accepted an undeclared field")
			}
		}
	})
}
