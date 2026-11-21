package output

import (
	"reflect"
	"strings"
	"testing"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCommandProjectionPreservesEvidence(t *testing.T) {
	info := protocol.DefaultProcessInfo()
	info.ID = protocol.NewRequestID()
	info.Command = strings.Repeat("sample command\n", 500)
	info.Status = protocol.ProcessStatus{Kind: "running"}
	info.Pty = true
	info.Services = []protocol.ProcessService{{Port: 8080, Url: "http://localhost:8080"}}
	snapshot := protocol.ProcessSnapshot{
		Info:   info,
		Stdout: protocol.ProcessCapture{Text: "ready\ncurrent stdout\n", Truncated: true},
		Stderr: protocol.ProcessCapture{Text: "current stderr: \ufffd\r\n", InvalidUtf8: true},
		Wait:   protocol.ProcessWaitMatched,
	}
	value := map[string]any{
		"kind": "command_output", "data": snapshot, "sailry_content": map[string]any{"version": 1},
		"sailry_command": map[string]any{
			"id": info.ID,
			"stdout": map[string]any{"head": "early stdout\n", "head_bytes": 13, "tail": "current stdout\n", "tail_offset": 65536, "total_bytes": 65551},
			"stderr": map[string]any{"tail": "newer diagnostic\r\n", "tail_offset": 128, "transcoded": true},
		},
	}
	record := resultRecord("poll", value, canonical.JSON)
	source := onlyResponse(&record)
	source.Name = "read_command"
	before := append([]byte(nil), source.Result.Value...)
	var cache Cache
	first := cache.Project([]canonical.Record{record})
	for _, projected := range [][]canonical.Record{first, cache.Project([]canonical.Record{record}), new(Cache).Project(first)} {
		result := onlyResponse(&projected[0])
		if result.Format != canonical.JSON || result.Revision != source.Revision || result.Status != source.Status {
			t.Fatal("projection changed the result identity or status")
		}
		got := object(decode(result.Result.Value))
		data := object(got["data"])
		gotInfo := object(data["info"])
		wantInfo := object(decode([]byte(encoded(map[string]any{"id": info.ID, "access": info.Access, "status": info.Status, "pty": true, "services": info.Services}))))
		if !reflect.DeepEqual(gotInfo, wantInfo) || text(data["wait"]) != string(snapshot.Wait) || got["sailry_content"] != nil {
			t.Fatal("projection lost process state or kept display metadata")
		}
		for name, want := range map[string]protocol.ProcessCapture{"stdout": snapshot.Stdout, "stderr": snapshot.Stderr} {
			if encoded(data[name]) != encoded(decode([]byte(encoded(want)))) {
				t.Errorf("projection changed %s capture", name)
			}
		}
		metadata := object(got["sailry_command"])
		stdout, stderr := object(metadata["stdout"]), object(metadata["stderr"])
		if stdout["head"] != "early stdout\n" || stdout["tail"] != nil || encoded(stdout["tail_offset"]) != "65536" || stderr["tail"] != "newer diagnostic\r\n" || stderr["transcoded"] != true {
			t.Fatal("projection changed retained ranges or hid newer diagnostics")
		}
		if ref := object(got["output"]); ref["call"] != source.ID || ref["revision"] != source.Revision {
			t.Fatal("projection lost its canonical reference")
		}
	}
	if string(source.Result.Value) != string(before) || string(onlyResponse(new(Audience(record))).Result.Value) != string(before) {
		t.Fatal("projection changed canonical or audience-only content")
	}
	source.Name = "unrelated_tool"
	if got := cache.Project([]canonical.Record{record}); string(onlyResponse(&got[0]).Result.Value) != string(before) {
		t.Fatal("previous command projection leaked to an unrelated tool")
	}
}

func TestCommandProjectionBoundsLargeFailure(t *testing.T) {
	last := "late stdout evidence\r\n"
	completion := protocol.ProcessCompletion{
		Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(128)},
		Stdout:  protocol.ProcessCapture{Text: strings.Repeat("日志\n\t", MaxBytes/3) + last, Truncated: true},
		Stderr:  protocol.ProcessCapture{Text: "fatal: permission denied\n"},
	}
	data := object(decode([]byte(encoded(completion))))
	data["sandbox_notice"] = "Inspect partial changes before requesting unrestricted access."
	commandID := protocol.NewRequestID().String()
	record := resultRecord("run", map[string]any{
		"kind": "command_result", "data": data,
		"sailry_command": map[string]any{"id": commandID, "stdout": map[string]any{"tail": last, "tail_offset": 70000, "total_bytes": 70022}},
	}, canonical.JSON)
	source := onlyResponse(&record)
	source.Name, source.Status = "run_command", canonical.ToolErrorStatus
	projected := new(Cache).Project([]canonical.Record{record})
	result := onlyResponse(&projected[0])
	got := object(decode(result.Result.Value))
	summary := object(got["data"])
	stdout := text(object(summary["stdout"])["text"])
	if len(result.Result.Value) > MaxBytes || !utf8.ValidString(stdout) || !strings.HasSuffix(stdout, last) || !strings.HasSuffix(completion.Stdout.Text, stdout) || len(stdout) >= len(completion.Stdout.Text) {
		t.Fatal("model bound did not keep an exact, UTF-8 aligned diagnostic suffix")
	}
	if result.Status != source.Status || got["preview_truncated"] != true || text(summary["sandbox_notice"]) != data["sandbox_notice"] ||
		encoded(summary["outcome"]) != encoded(data["outcome"]) || object(got["sailry_command"])["id"] != commandID ||
		object(summary["stderr"])["text"] != completion.Stderr.Text {
		t.Fatal("bounded foreground result lost the exit code, command ID, stderr or permission diagnostic")
	}
}
