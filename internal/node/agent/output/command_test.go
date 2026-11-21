package output

import (
	"encoding/json"
	"fmt"
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
		Stdout: protocol.ProcessCapture{Text: "ready\ncurrent stdout\n"},
		Stderr: protocol.ProcessCapture{Text: "current stderr: \ufffd\r\n", InvalidUtf8: true},
		Wait:   protocol.ProcessWaitMatched,
	}
	value := map[string]any{
		"kind": "command_output", "data": snapshot, "sailry_content": map[string]any{"version": 1},
		"sailry_command": map[string]any{
			"id": info.ID,
			"stdout": map[string]any{
				"head": "early stdout\n", "head_bytes": 13, "head_invalid_utf8": false,
				"tail": "current stdout\n", "tail_offset": 65536, "total_bytes": 65551,
				"retained_head_bytes": 64, "retained_tail_offset": 65500, "retained_end": 65551, "retained_bytes": 115,
				"invalid_utf8": false, "transcoded": false,
			},
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
		retained := object(object(decode(source.Result.Value))["sailry_command"])
		if !reflect.DeepEqual(stdout, retained["stdout"]) || !reflect.DeepEqual(stderr, retained["stderr"]) {
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
	const first, last = "first stdout evidence: 世界\r\n", "late stdout evidence\r\n"
	completion := protocol.ProcessCompletion{
		Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(128)},
		Stdout:  protocol.ProcessCapture{Text: first + strings.Repeat("日志\r\n", 8100), Truncated: true},
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
	stdout := object(summary["stdout"])
	shown := text(stdout["text"])
	if len(result.Result.Value) > MaxBytes || commandLines(got) > MaxLines || !utf8.ValidString(shown) || !strings.HasPrefix(shown, first) ||
		!strings.HasPrefix(completion.Stdout.Text, shown) || len(shown) >= len(completion.Stdout.Text) || stdout["truncated"] != true {
		t.Fatal("model bound did not keep an exact, UTF-8 aligned foreground prefix")
	}
	retained := object(object(got["sailry_command"])["stdout"])
	if result.Status != source.Status || got["preview_truncated"] != true || text(summary["sandbox_notice"]) != data["sandbox_notice"] ||
		encoded(summary["outcome"]) != encoded(data["outcome"]) || object(got["sailry_command"])["id"] != commandID ||
		object(summary["stderr"])["text"] != completion.Stderr.Text || retained["tail"] != last || encoded(retained["tail_offset"]) != "70000" {
		t.Fatal("bounded foreground result lost the exit code, command ID, stderr or permission diagnostic")
	}
}

func TestCommandProjectionSharesLineBudget(t *testing.T) {
	info := protocol.DefaultProcessInfo()
	info.ID = protocol.NewRequestID()
	info.Status = protocol.ProcessStatus{Kind: "running"}
	const first, last = "first line\r\n", "latest stdout\r\n"
	head := first + strings.Repeat("row\r\n", 800)
	snapshot := protocol.ProcessSnapshot{
		Info:   info,
		Stdout: protocol.ProcessCapture{Text: head + strings.Repeat("row\r\n", 700) + last, Truncated: true},
		Stderr: protocol.ProcessCapture{Text: "diagnostic \ufffd\r\n", InvalidUtf8: true},
		Wait:   protocol.ProcessWaitOutput,
	}
	record := resultRecord("poll", map[string]any{
		"kind": "command_output", "data": snapshot,
		"sailry_command": map[string]any{"id": info.ID, "stdout": map[string]any{"head": head, "head_bytes": len(head)}},
	}, canonical.JSON)
	source := onlyResponse(&record)
	source.Name = "read_command"
	if len(source.Result.Value) >= MaxBytes || lineCount(snapshot.Stdout.Text) >= MaxLines {
		t.Fatal("fixture must exceed only the shared line budget")
	}
	projected := new(Cache).Project([]canonical.Record{record})
	result := onlyResponse(&projected[0])
	got := object(decode(result.Result.Value))
	data := object(got["data"])
	stdout, stderr := object(data["stdout"]), object(data["stderr"])
	retained := object(object(got["sailry_command"])["stdout"])
	shown, early := text(stdout["text"]), text(retained["head"])
	if lines := commandLines(got); lines > MaxLines || len(result.Result.Value) > MaxBytes || got["preview_truncated"] != true {
		t.Fatalf("expected a bounded line preview, got %d lines and %d bytes", lines, len(result.Result.Value))
	}
	if !strings.HasPrefix(early, first) || strings.Contains(shown, first) || !strings.HasSuffix(shown, last) || !strings.HasSuffix(snapshot.Stdout.Text, shown) {
		t.Fatal("bounded poll lost the retained head or recent stdout")
	}
	if stdout["truncated"] != true || stderr["invalid_utf8"] != true || stderr["text"] != snapshot.Stderr.Text || data["wait"] != string(snapshot.Wait) {
		t.Fatal("bounded poll changed stream flags, stderr or wait reason")
	}
}

func TestCommandProjectionBoundsPage(t *testing.T) {
	for _, test := range []struct {
		name, text string
		lineLimit  bool
	}{
		{"lines", "世\r\n" + strings.Repeat("row\r\n", MaxLines), true},
		{"escaped bytes", "世\r\n" + strings.Repeat("\x1b界", MaxBytes/8), false},
	} {
		t.Run(test.name, func(t *testing.T) {
			info := protocol.DefaultProcessInfo()
			info.ID = protocol.NewRequestID()
			info.Status = protocol.ProcessStatus{Kind: "running"}
			page := protocol.ProcessOutputOutputPage{
				Info: info, Stream: protocol.ProcessOutputOutputStreamStderr,
				Offset: 19, TotalBytes: 120000, NextOffset: new(uint64(90000)), Text: test.text,
				Truncated: true, InvalidUtf8: true, Wait: protocol.ProcessWaitMatched,
			}
			record := resultRecord("page", map[string]any{"kind": "command_output_page", "data": page}, canonical.JSON)
			source := onlyResponse(&record)
			source.Name = "read_command"
			if (lineCount(page.Text) > MaxLines) != test.lineLimit || (len(source.Result.Value) > MaxBytes) == test.lineLimit {
				t.Fatal("fixture must exceed exactly one projection limit")
			}
			projected := new(Cache).Project([]canonical.Record{record})
			result := onlyResponse(&projected[0])
			got := object(decode(result.Result.Value))
			data := object(got["data"])
			shown := text(data["text"])
			if !utf8.ValidString(shown) || !strings.HasPrefix(shown, "世\r\n") || !strings.HasPrefix(page.Text, shown) || shown == page.Text ||
				commandLines(got) > MaxLines || len(result.Result.Value) > MaxBytes || got["preview_truncated"] != true {
				t.Fatal("bounded page did not keep a UTF-8 aligned prefix within both limits")
			}
			if ref := object(got["output"]); ref["call"] != source.ID || ref["revision"] != source.Revision || object(data["info"])["id"] != info.ID.String() {
				t.Fatal("bounded page lost its canonical reference or process ID")
			}
			want := object(decode([]byte(encoded(page))))
			delete(want, "info")
			delete(data, "info")
			data["text"] = page.Text
			if !reflect.DeepEqual(data, want) {
				t.Fatal("bounded page changed the stream, cursor, wait reason or flags")
			}
		})
	}
}

func TestCommandProjectionDeduplicatesOnlyShownRanges(t *testing.T) {
	const first, last = "first\r\n", "last: 世界\r\n"
	info := protocol.DefaultProcessInfo()
	info.ID, info.Status.Kind = protocol.NewRequestID(), "running"
	body := first + strings.Repeat("x\r\n", MaxLines) + last
	snapshot := protocol.ProcessSnapshot{Info: info, Stdout: protocol.ProcessCapture{Text: body}}
	value := map[string]any{"kind": "command_output", "data": snapshot, "sailry_command": map[string]any{
		"id": info.ID, "stdout": map[string]any{
			"head": first, "head_bytes": len(first), "head_invalid_utf8": false, "tail": last,
			"tail_offset": len(body) - len(last), "total_bytes": len(body), "invalid_utf8": false, "transcoded": false,
		},
	}}
	result := projectCommandRecord(t, "read_command", value, canonical.JSON, canonical.ToolSuccess)
	got := object(decode(result.Result.Value))
	retained := object(object(got["sailry_command"])["stdout"])
	shown := text(object(object(got["data"])["stdout"])["text"])
	if strings.Contains(shown, first) || !strings.HasSuffix(shown, last) || !strings.HasSuffix(body, shown) ||
		retained["head"] != first || retained["tail"] != nil || encoded(retained["tail_offset"]) != fmt.Sprint(len(body)-len(last)) {
		t.Fatal("projection did not preserve the disjoint head and merge the identical shown tail range")
	}
}

func TestCommandTextByteClipPreservesUTF8AndCRLF(t *testing.T) {
	for _, test := range []struct {
		body string
		tail bool
	}{
		{"a\r\n界", false},
		{"界\r\na", true},
		{"a界", false},
		{"界a", true},
	} {
		if shown := fitEncodedText(test.body, 3, MaxLines, test.tail); shown != "a" {
			t.Errorf("clipping %q (tail=%t) gave %q, want %q", test.body, test.tail, shown, "a")
		}
	}
}

func TestCommandProjectionBoundsStateAndServices(t *testing.T) {
	const diagnostic = "launch failed: 世界\r\n"
	for _, test := range []struct {
		name, detail string
		services     []protocol.ProcessService
	}{
		{name: "failed status lines", detail: diagnostic + strings.Repeat("e\r\n", MaxLines)},
		{name: "unknown outcome escaped bytes", detail: diagnostic + strings.Repeat("<&>", 10000), services: []protocol.ProcessService{{Port: 8080, Url: strings.Repeat("<", 10000)}}},
	} {
		t.Run(test.name, func(t *testing.T) {
			info := protocol.DefaultProcessInfo()
			info.ID, info.Status = protocol.NewRequestID(), protocol.ProcessStatus{Kind: "failed", Data: test.detail}
			if len(test.services) > 0 {
				info.Status = protocol.ProcessStatus{Kind: "finished", Data: protocol.ProcessOutcome{Kind: "unknown", Data: test.detail}}
				info.Services = test.services
			}
			snapshot := protocol.ProcessSnapshot{Info: info, Stderr: protocol.ProcessCapture{Text: "short stderr\r\n"}, Wait: protocol.ProcessWaitExited}
			result := projectCommandRecord(t, "read_command", map[string]any{"kind": "command_output", "data": snapshot}, canonical.JSON, canonical.ToolErrorStatus)
			got := object(decode(result.Result.Value))
			data := object(got["data"])
			projected := object(data["info"])
			status := object(projected["status"])
			shown := text(status["data"])
			if info.Status.Kind == "finished" {
				outcome := object(status["data"])
				if outcome["kind"] != "unknown" {
					t.Fatal("projection lost the process outcome")
				}
				shown = text(outcome["data"])
			}
			if result.Format != canonical.JSON || got["preview_truncated"] != true || projected["id"] != info.ID.String() || projected["services"] != nil ||
				status["kind"] != info.Status.Kind || !strings.HasPrefix(shown, diagnostic) || !strings.HasPrefix(test.detail, shown) || shown == test.detail ||
				object(data["stderr"])["text"] != snapshot.Stderr.Text || data["wait"] != string(snapshot.Wait) || commandLines(got) > MaxLines {
				t.Fatal("bounded status lost its diagnostic prefix, process identity, wait reason or stderr")
			}
		})
	}
}

func TestCommandProjectionBoundsListPrefix(t *testing.T) {
	items := make([]protocol.ProcessInfo, MaxLines)
	for i := range items {
		items[i] = protocol.DefaultProcessInfo()
		items[i].ID, items[i].Status.Kind, items[i].Command = protocol.NewRequestID(), "running", "first line\r\nsecond line"
	}
	items[0].Services = []protocol.ProcessService{{Port: 8080, Url: strings.Repeat("<", 10000)}}
	result := projectCommandRecord(t, "read_command", map[string]any{"kind": "commands", "data": items}, canonical.JSON, canonical.ToolSuccess)
	got := object(decode(result.Result.Value))
	list := array(got["data"])
	if result.Format != canonical.JSON || len(list) < 2 || len(list) >= len(items) || got["preview_truncated"] != true || commandLines(got) > MaxLines {
		t.Fatalf("expected a bounded prefix of the command list, got %d entries", len(list))
	}
	for i, entry := range list {
		info := object(entry)
		if info["id"] != items[i].ID.String() || info["command_preview"] != "first line…" || object(info["status"])["kind"] != "running" || info["services"] != nil {
			t.Fatalf("command list entry %d changed its identity, label or status", i)
		}
	}
}

func TestCommandProjectionBoundsFallbackAndDiagnostics(t *testing.T) {
	const prefix = "Request failed: 世界\r\n"
	for _, test := range []struct {
		name, body string
		format     canonical.ToolFormat
		diagnostic bool
	}{
		{"unshaped lines", prefix + strings.Repeat("x\r\n", MaxLines), canonical.JSON, false},
		{"unshaped escaped bytes", prefix + strings.Repeat("<&>", 9000), canonical.JSON, false},
		{"returned diagnostic lines", prefix + strings.Repeat("x\r\n", MaxLines), canonical.JSON, true},
		{"runtime fault escaped bytes", prefix + strings.Repeat("<&>", 9000), canonical.MCPError, true},
	} {
		t.Run(test.name, func(t *testing.T) {
			value := map[string]any{"unexpected": test.body}
			status := canonical.ToolSuccess
			if test.diagnostic {
				value, status = map[string]any{"message": test.body, "code": -32000}, canonical.ToolErrorStatus
			}
			result := projectCommandRecord(t, "run_command", value, test.format, status)
			shown := ""
			if test.diagnostic {
				shown = "Tool error: " + protocol.ToolErrorMessage(result.Result.Value)
				if !strings.HasPrefix(shown, "Tool error: "+prefix) || len(result.Result.Value)+len("Tool error: ") > MaxBytes {
					t.Fatal("bounded diagnostic lost its prefix or left no budget for the model's error marker")
				}
			} else {
				content := array(object(decode(result.Result.Value))["content"])
				if result.Format != canonical.MCP || len(content) != 1 {
					t.Fatal("over-limit unrecognized result did not produce a recovery notice")
				}
				shown = text(object(content[0])["text"])
			}
			if len(shown) > MaxBytes || lineCount(shown) > MaxLines || !strings.Contains(shown, "read_tool_output") || !strings.Contains(shown, result.Revision) {
				t.Fatal("model-facing fallback exceeded the limits or lost its canonical reference")
			}
		})
	}
}

func TestCommandProjectionBoundsCanonicalRecoveryPages(t *testing.T) {
	for _, tool := range []string{"read_tool_output", "read_command"} {
		for _, test := range []struct{ name, body string }{
			{"lines", "世\r\n" + strings.Repeat("row\r\n", MaxLines)},
			{"escaped bytes", "世\r\n" + strings.Repeat("<界", 7800)},
		} {
			t.Run(tool+"/"+test.name, func(t *testing.T) {
				const offset = 11
				end := uint64(offset + len(test.body))
				page := protocol.ToolOutputOutputPage{
					Turn: protocol.NewTurnID(), Output: protocol.ToolOutputOutputReference{Call: "original<&", Revision: strings.Repeat("a", 64)},
					Offset: offset, TotalBytes: end + 5, NextOffset: &end, Text: test.body,
				}
				project := func(page protocol.ToolOutputOutputPage) (string, *uint64) {
					var value any = page
					if tool == "read_command" {
						value = map[string]any{"kind": "tool_output", "data": page}
					}
					result := projectCommandRecord(t, tool, value, canonical.JSON, canonical.ToolSuccess)
					parts := array(object(decode(result.Result.Value))["content"])
					if result.Format != canonical.MCP || len(parts) != 1 {
						t.Fatal("canonical recovery did not produce a text page")
					}
					modelText := text(object(parts[0])["text"])
					if lineCount(modelText) > MaxLines || !utf8.ValidString(modelText) {
						t.Fatal("model-facing recovery exceeded the line cap or split UTF-8")
					}
					header, rest, ok := strings.Cut(modelText, "\n")
					sep := strings.LastIndexByte(rest, '\n')
					if !ok || sep < 0 {
						t.Fatal("model-facing recovery lost the page range or continuation")
					}
					shown := rest[:sep]
					var footer struct {
						Output     protocol.ToolOutputOutputReference `json:"output"`
						NextOffset *uint64                            `json:"next_offset"`
					}
					if err := json.Unmarshal([]byte(rest[sep+1:]), &footer); err != nil || footer.Output != page.Output ||
						header != fmt.Sprintf("Result bytes %d–%d of %d", page.Offset, page.Offset+uint64(len(shown)), page.TotalBytes) {
						t.Fatal("canonical recovery changed the source reference or displayed byte range")
					}
					return shown, footer.NextOffset
				}
				shown, next := project(page)
				if len(shown) == 0 || len(shown) >= len(page.Text) || !strings.HasPrefix(page.Text, shown) || next == nil || *next != page.Offset+uint64(len(shown)) || *next >= end {
					t.Fatal("canonical recovery skipped unread bytes after clipping a page")
				}
				page.Offset, page.Text = *next, page.Text[len(shown):]
				suffix, next := project(page)
				if shown+suffix != test.body || next == nil || *next != end {
					t.Fatal("the next page did not recover the remaining bytes and original cursor")
				}
			})
		}
	}
}

func projectCommandRecord(t *testing.T, name string, value any, format canonical.ToolFormat, status canonical.ToolStatus) *canonical.ToolResponse {
	t.Helper()
	record := resultRecord("call", value, format)
	source := onlyResponse(&record)
	source.Name, source.Status = name, status
	before := string(source.Result.Value)
	projected := new(Cache).Project([]canonical.Record{record})
	result := onlyResponse(&projected[0])
	raw, err := json.Marshal(result.Result.Value)
	if err != nil || len(raw) > MaxBytes || result.ID != source.ID || result.Revision != source.Revision || result.Status != status || string(source.Result.Value) != before {
		t.Fatalf("projection changed canonical content or identity, or exceeded the encoded cap (%d bytes)", len(raw))
	}
	return result
}
