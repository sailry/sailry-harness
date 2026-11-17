package live

import (
	"archive/zip"
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type directoryUpdateTestNode struct {
	terminalTabsTestNode
	stream   protocol.StreamID
	uploaded []byte
}

type directoryUpdateTestUpload struct {
	*bytes.Reader
	node *directoryUpdateTestNode
}

func (node *directoryUpdateTestNode) Open(_ context.Context, _ protocol.NodeID, id protocol.StreamID) (link.Stream, error) {
	if id != node.stream {
		return nil, errors.New("unexpected fixture upload stream")
	}
	return &directoryUpdateTestUpload{Reader: bytes.NewReader([]byte{protocol.FILE_UPLOAD_READY}), node: node}, nil
}

func (stream *directoryUpdateTestUpload) Write(data []byte) (int, error) {
	stream.node.mu.Lock()
	stream.node.uploaded = append(stream.node.uploaded, data...)
	stream.node.mu.Unlock()
	return len(data), nil
}
func (*directoryUpdateTestUpload) CloseWrite() error { return nil }
func (*directoryUpdateTestUpload) Close() error      { return nil }

func TestDirectoryUpdateUsesNodeVerdict(t *testing.T) {
	for _, test := range []struct {
		name, verdict, source string
		inspectFault          bool
		checkFault, badOutput bool
		retain, wantError     bool
		checks                int
	}{
		{name: "Node accepts after earlier compatibility refusal", verdict: "available", retain: true, checks: 1},
		{name: "Node declines newer uploaded bytes", checks: 1},
		{name: "Node blocks earlier compatible inspection", verdict: "blocked", checks: 1},
		{name: "Node preserves a fixed version", verdict: "pinned", checks: 1},
		{name: "Node assessment failure", checkFault: true, wantError: true, checks: 1},
		{name: "unexpected assessment response", badOutput: true, wantError: true, checks: 1},
		{name: "uploaded inspection failure", inspectFault: true, wantError: true},
		{name: "different package identity", source: "other", wantError: true},
		{name: "archive is not a remembered directory", source: "archive", wantError: true},
		{name: "missing directory keeps the existing report", source: "missing"},
	} {
		t.Run(test.name, func(t *testing.T) {
			node := &directoryUpdateTestNode{stream: protocol.StreamID{3}}
			controller := client.New(link.NewLocal(protocol.NodeID{1}, protocol.NodeID{1}, node))
			t.Cleanup(func() { _ = controller.Close() })
			r := &Runtime{ctx: t.Context(), controller: controller, Preferences: preferences.Open(filepath.Join(t.TempDir(), "config", "preferences.json"))}
			directory := t.TempDir()
			manifest := []byte(`{"name":"notes","version":"1.1.0"}`)
			if err := os.WriteFile(filepath.Join(directory, "plugin.json"), manifest, 0o600); err != nil {
				t.Fatal(err)
			}
			selectedPath := directory
			switch test.source {
			case "missing":
				selectedPath = filepath.Join(directory, "removed")
			case "archive":
				var body bytes.Buffer
				archive := zip.NewWriter(&body)
				entry, err := archive.Create("plugin.json")
				if err != nil {
					t.Fatal(err)
				}
				if _, err := entry.Write(manifest); err != nil {
					t.Fatal(err)
				}
				if err := archive.Close(); err != nil {
					t.Fatal(err)
				}
				selectedPath = filepath.Join(t.TempDir(), "notes.zip")
				if err := os.WriteFile(selectedPath, body.Bytes(), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			initial := protocol.DefaultPluginUpdatesReport()
			initial.Current.Name, initial.Current.Version, initial.Current.Revision, initial.Manual = "notes", new("1.0.0"), 7, true
			inspected := protocol.DefaultPluginInfo()
			inspected.Summary.Name, inspected.Summary.Version = "notes", new("1.1.0")
			if test.source == "other" {
				inspected.Summary.Name = "other"
			}
			verdict := initial
			verdict.Manual = false
			switch test.verdict {
			case "available":
				inspected.Summary.Compatibility = protocol.PluginCompatibility{Kind: "requires_newer", Data: protocol.PluginCompatibilityRequiresNewerPayload{Required: "9.0.0"}}
				candidate := inspected
				candidate.Summary.Compatibility = protocol.PluginCompatibility{Kind: "compatible"}
				verdict.Available = &protocol.PluginUpdatesCandidate{Kind: "package", Data: candidate}
			case "blocked":
				verdict.Blocked = &protocol.PluginUpdatesBlocked{Version: "1.1.0", Required: "9.0.0"}
			case "pinned":
				verdict.Current.Pinned = true
			}
			node.respond = func(request protocol.Request) link.Response {
				switch request.Command.Kind {
				case "upload_plugin":
					spec, _ := detail[protocol.PluginUploadSpec](request.Command.Data)
					return link.Response{Output: protocol.Output{Kind: "plugin_upload", Data: protocol.PluginUpload{Stream: node.stream, Spec: spec}}}
				case "inspect_plugin_upload":
					if test.inspectFault {
						return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture inspection failure"}}
					}
					return link.Response{Output: protocol.Output{Kind: "plugin", Data: inspected}}
				case "check_plugin_update":
					if test.checkFault {
						return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture assessment failure"}}
					}
					if test.badOutput {
						return link.Response{Output: protocol.Output{Kind: "plugin", Data: inspected}}
					}
					return link.Response{Output: protocol.Output{Kind: "plugin_update", Data: verdict}}
				case "cancel_file_transfer":
					return link.Response{Output: protocol.Output{Kind: "file_transfer_cancelled"}}
				default:
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "unexpected fixture command"}}
				}
			}
			report := initial
			local, err := r.directoryUpdate(&report, selectedPath)
			if (err != nil) != test.wantError || (local != nil) != test.retain {
				t.Fatalf("retained %v, error %v", local != nil, err)
			}
			expected := initial
			if test.checks == 1 && !test.wantError {
				expected = verdict
			}
			if !reflect.DeepEqual(report, expected) {
				t.Fatalf("Desktop changed the Node verdict: got %+v, want %+v", report, expected)
			}
			checks := node.sent("check_plugin_update")
			if len(checks) != test.checks {
				t.Fatalf("Node assessments: %+v", checks)
			}
			if len(checks) == 1 {
				input, _ := detail[protocol.CommandCheckPluginUpdatePayload](checks[0].Data)
				if input.Name != initial.Current.Name || input.ExpectedRevision != initial.Current.Revision || input.Upload == nil || *input.Upload != node.stream {
					t.Fatalf("assessment lost its installed revision or caller-owned upload: %+v", input)
				}
			}
			if local != nil && (local.directory != directory || local.source != protocol.PluginUploadSourceDirectory || local.stream != node.stream) {
				t.Fatalf("the offered upload lost its local directory source: %+v", local)
			}
			uploads := node.sent("upload_plugin")
			wantUploads, wantCancels := 1, 1
			if test.source == "missing" {
				wantUploads, wantCancels = 0, 0
			} else if test.retain {
				wantCancels = 0
			}
			if len(uploads) != wantUploads || len(node.sent("inspect_plugin_upload")) != wantUploads {
				t.Fatalf("local bytes were not inspected by their Node: uploads %+v", uploads)
			}
			if len(uploads) == 1 {
				spec, _ := detail[protocol.PluginUploadSpec](uploads[0].Data)
				if spec.Size == 0 || spec.Size != uint64(len(node.uploaded)) {
					t.Fatalf("Node did not receive the captured bytes: size %d, uploaded %d", spec.Size, len(node.uploaded))
				}
			}
			cancels := node.sent("cancel_file_transfer")
			if len(cancels) != wantCancels {
				t.Fatalf("unused upload cancellation: %+v", cancels)
			}
			if len(cancels) == 1 {
				input, _ := detail[protocol.CommandCancelFileTransferPayload](cancels[0].Data)
				if input.Stream != node.stream {
					t.Fatalf("cancelled another stream: %+v", input)
				}
			}
		})
	}
}
