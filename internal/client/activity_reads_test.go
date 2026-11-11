package client

import (
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func testActivityReads(t *testing.T) {
	transport := &clientTransport{}
	controller := New(transport)
	t.Cleanup(func() { _ = controller.Close() })
	scope := protocol.DefaultPluginContext()
	scope.Package = protocol.PluginReference{Name: "activity-fixture", Digest: strings.Repeat("a", 64), SettingsRevision: 7}
	scope.Surface.Kind = "workspace"
	captured := copyValue(scope)
	session := protocol.NewSessionID()
	catalog := protocol.DefaultActivityCatalog()
	catalog.Node, catalog.Cursor = controller.Target(), 11
	preview := protocol.ActivityPreview{Session: session, Text: new("Captured activity")}
	fail := false
	transport.dispatch = func(request protocol.Request) (link.Admission, error) {
		if !reflect.DeepEqual(request.Plugin, &captured) || request.Target != controller.Target() || request.Command.Durable() {
			t.Fatal("activity read changed captured package provenance, target or durability")
		}
		completed := make(chan link.Response, 1)
		output := protocol.Output{}
		switch request.Command.Kind {
		case "read_activity_catalog":
			output = protocol.Output{Kind: "activity_catalog", Data: catalog}
		case "read_activity":
			input, err := payload[protocol.CommandReadActivityPayload](request.Command.Data)
			if err != nil || !reflect.DeepEqual(input.Sessions, []protocol.SessionID{session}) {
				t.Fatal("preview read changed the captured session list")
			}
			output = protocol.Output{Kind: "activity", Data: []protocol.ActivityPreview{preview}}
		default:
			t.Fatalf("unexpected activity command: %s", request.Command.Kind)
		}
		if fail {
			completed <- link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "plugin package changed"}}
		} else {
			completed <- link.Response{Output: output}
		}
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completed}, nil
	}
	if actual, err := controller.ReadActivityCatalog(t.Context(), scope); err != nil || !reflect.DeepEqual(actual, catalog) {
		t.Fatalf("captured activity catalog: %+v, %v", actual, err)
	}
	scope.Package.SettingsRevision++
	if transport.requests[0].Plugin.Package != captured.Package {
		t.Fatal("editing a caller's scope changed the retained catalog request")
	}
	scope = captured
	if values, err := controller.ReadActivityPreviews(t.Context(), scope, []protocol.SessionID{session}); err != nil || !reflect.DeepEqual(values, []protocol.ActivityPreview{preview}) {
		t.Fatalf("captured activity previews: %+v, %v", values, err)
	}
	before := len(transport.requests)
	if _, err := controller.ReadActivityPreviews(t.Context(), scope, make([]protocol.SessionID, 9)); faultCode(err) != protocol.ErrorCodeInvalidRequest || len(transport.requests) != before {
		t.Fatal("oversized desktop preview batch reached the Node")
	}
	fail = true
	if _, err := controller.ReadActivityCatalog(t.Context(), scope); faultCode(err) != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("catalog lost the Node's package conflict: %v", err)
	}
	if _, err := controller.ReadActivityPreviews(t.Context(), scope, []protocol.SessionID{session}); faultCode(err) != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("preview lost the Node's package conflict: %v", err)
	}

	// Read-aloud synthesis is a controller command without a package scope.
	transport.dispatch = func(request protocol.Request) (link.Admission, error) {
		input, err := payload[protocol.CommandSynthesizeSpeechPayload](request.Command.Data)
		if err != nil || request.Command.Kind != "synthesize_speech" || request.Plugin != nil || request.Command.Durable() || input.Text != "Read this" || input.Voice != nil {
			t.Fatalf("speech request = %+v, %v", request.Command, err)
		}
		completed := make(chan link.Response, 1)
		if fail {
			completed <- link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "speech model is not assigned"}}
		} else {
			completed <- link.Response{Output: protocol.Output{Kind: "speech", Data: protocol.SpeechAudio{MediaType: "audio/mpeg", DataBase64: "SUQz", Provider: protocol.NewProviderID(), Model: "voice"}}}
		}
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completed}, nil
	}
	if _, _, err := controller.SynthesizeSpeech(t.Context(), "Read this", nil); faultCode(err) != protocol.ErrorCodeNotConfigured {
		t.Fatalf("speech lost the Node's fault: %v", err)
	}
	fail = false
	if mediaType, audio, err := controller.SynthesizeSpeech(t.Context(), "Read this", nil); err != nil || mediaType != "audio/mpeg" || string(audio) != "ID3" {
		t.Fatalf("speech audio = %s, %q, %v", mediaType, audio, err)
	}
}
