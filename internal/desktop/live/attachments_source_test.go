package live

import (
	"errors"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestLocalAttachmentSourceRejections(t *testing.T) {
	t.Parallel()
	root := t.TempDir()
	for _, test := range []struct {
		name, path, key string
		code            protocol.ErrorCode
		cause           error
	}{
		{"directory", root, "files_upload_directory", protocol.ErrorCodeInvalidRequest, nil},
		{"relative", "relative.txt", "files_upload_source", protocol.ErrorCodeInvalidRequest, nil},
		{"missing", filepath.Join(root, "missing.txt"), "files_upload_source", "", os.ErrNotExist},
	} {
		t.Run(test.name, func(t *testing.T) {
			source, err := localSource(t.Context(), test.path, nil, "")
			var sourceError *attachmentSourceError
			if !errors.As(err, &sourceError) || sourceError.key != test.key || faultCode(err) != test.code || source.reader != nil {
				t.Fatalf("source rejection lost its outcome: source %+v, error %v", source, err)
			}
			if test.cause != nil && !errors.Is(err, test.cause) {
				t.Fatalf("source rejection lost its I/O cause: %v", err)
			}
		})
	}
}

func TestDraftAttachmentSourceRejections(t *testing.T) {
	root := t.TempDir()
	for _, test := range []struct {
		name, path, key, reason, code string
		level                         slog.Level
	}{
		{"directory", root, "files_upload_directory", "directory", "invalid_request", slog.LevelWarn},
		{"relative", "relative.txt", "files_upload_source", "not_absolute", "invalid_request", slog.LevelError},
		{"missing", filepath.Join(root, "missing.txt"), "files_upload_source", "read_failed", "internal", slog.LevelError},
	} {
		t.Run(test.name, func(t *testing.T) {
			testDraftAttachmentSourceRejection(t, test.path, test.key, test.reason, test.code, test.level)
		})
	}
}

func testDraftAttachmentSourceRejection(t *testing.T, path, key, reason, code string, level slog.Level) {
	t.Helper()
	closeLog, err := logging.Install(filepath.Join(t.TempDir(), "profile"), "desktop")
	if err != nil {
		t.Fatal(err)
	}
	defer closeLog()
	var mu sync.Mutex
	var records []struct {
		level  slog.Level
		fields logging.Fields
	}
	logging.SetReport(func(level slog.Level, fields logging.Fields) {
		operation, _ := fields["operation"].(string)
		if fields["area"] != "desktop" || !strings.HasPrefix(operation, "files_upload_") {
			return
		}
		mu.Lock()
		defer mu.Unlock()
		records = append(records, struct {
			level  slog.Level
			fields logging.Fields
		}{level, fields})
	})
	defer logging.SetReport(nil)
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	transport := &conversationTestTransport{snapshot: snapshot, conversation: protocol.DefaultConversationSnapshot()}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	r, err := NewWithPreferences(t.Context(), controller, preferences.Open(filepath.Join(t.TempDir(), "preferences.json")))
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	r.newDraft(nil)
	draft := r.App.Sidebar.Selected
	cv, comp := r.App.Data.Find(draft), r.App.Composer(draft)
	var view *ui.Context
	var actions *conversationActions
	tester := ui.NewTester(func(c *ui.Context) {
		view = c
		r.drain(c)
		_, action := r.conversation(c, cv, comp)
		actions = action.(*conversationActions)
		r.finishFrame()
	}, 660, 480)
	settle := func() {
		for range 6 {
			r.workers.Wait()
			tester.Frame()
		}
	}
	settle()
	comp.AttachPaths(view, []string{path}, actions)
	if len(comp.Attachments) != 1 || comp.Attachments[0].Status != composer.Preparing {
		t.Fatalf("source was not staged for asynchronous validation: %+v", comp.Attachments)
	}
	settle()
	mu.Lock()
	if len(records) != 1 || records[0].level != level || records[0].fields["operation"] != key || records[0].fields["reason"] != reason || records[0].fields["code"] != code {
		t.Errorf("source refusal diagnostics = %+v; want %v, %s, %s, %s", records, level, key, reason, code)
	} else {
		safe := logging.Remote(records[0].fields)
		if safe["cause_code"] != "attachment_"+reason || safe["operation"] != key || safe["code"] != code || safe["detail"] != nil || safe["reason"] != nil {
			t.Errorf("remote source diagnostics lost their bounded cause or exposed local fields: %v", safe)
		}
	}
	mu.Unlock()
	if len(comp.Attachments) != 1 {
		t.Fatalf("source rejection removed its card: %+v", comp.Attachments)
	}
	attachment := comp.Attachments[0]
	record := r.attachments.items[attachment]
	if attachment.Path != path || attachment.Status != composer.Failed || attachment.Error != key || record == nil || record.running || record.pending != nil || record.ready != nil {
		t.Fatalf("source rejection lost its failed, removable card: %+v, record %+v", attachment, record)
	}
	actions.Discard(attachment)
	settle()
	if len(comp.Attachments) != 0 || r.attachments.items[attachment] != nil {
		t.Fatal("the rejected source card could not be removed")
	}
	transport.mu.Lock()
	defer transport.mu.Unlock()
	for _, request := range transport.requests {
		switch request.Command.Kind {
		case "upload_attachment", "finish_attachment_upload", "discard_attachment":
			t.Errorf("rejected source reached the Node: %s", request.Command.Kind)
		}
	}
}

func TestAttachmentSourceInvalidRequestsRemainErrors(t *testing.T) {
	closeLog, err := logging.Install(filepath.Join(t.TempDir(), "profile"), "desktop")
	if err != nil {
		t.Fatal(err)
	}
	defer closeLog()
	var severity slog.Level
	var fields logging.Fields
	logging.SetReport(func(level slog.Level, values logging.Fields) { severity, fields = level, values })
	defer logging.SetReport(nil)
	_, tooLarge := localSource(t.Context(), "", []byte("over limit"), "", 1)
	for _, test := range []struct {
		name string
		err  error
	}{
		{"upload descriptor", fileActionFault(protocol.ErrorCodeInvalidRequest, "attachment upload descriptor changed")},
		{"unclassified source", &attachmentSourceError{key: "files_upload_source", err: fileActionFault(protocol.ErrorCodeInvalidRequest, "unclassified source failure")}},
		{"too large", tooLarge},
	} {
		t.Run(test.name, func(t *testing.T) {
			fields = nil
			shown := false
			tester := ui.NewTester(func(c *ui.Context) {
				if !shown {
					shown = true
					(&Runtime{}).attachmentOutcome(c, test.err)
				}
			}, 400, 200)
			tester.Frame()
			if fields == nil || severity != slog.LevelError || fields["code"] != "invalid_request" {
				t.Fatalf("an invalid request was incorrectly downgraded: %v, %v", severity, fields)
			}
		})
	}
}
