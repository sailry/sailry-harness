package live

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func testApprovalProjection(t *testing.T) {
	session, turn, id := protocol.NewSessionID(), protocol.NewTurnID(), protocol.NewApprovalID()
	destination, prefix := "renamed.txt", "safe command"
	access := protocol.ProcessAccessUnrestricted
	call := protocol.ConversationPartToolCallPayload{Name: "apply_changes", Summary: &protocol.ToolDetailSummary{Resolved: &protocol.ToolDetailResolved{
		Label:    &protocol.ToolResultLocalText{Label: "Apply changes", Locales: map[string]string{"zh-CN": "应用修改"}},
		Input:    protocol.ToolDetailInput{Context: &protocol.ToolDetailText{Text: "src"}, Content: &protocol.ToolDetailText{Text: "generic change prompt", Code: true}},
		Approval: []protocol.ToolResultLocalText{{Label: "Captured notice", Locales: map[string]string{"zh-CN": "已捕获提示"}}, {Label: "Fallback notice"}},
	}}}
	approval := protocol.ConversationApproval{ID: id, Session: session, Turn: turn, Entry: "call", Index: 0, State: protocol.ConversationApprovalStatePending, Prefix: &prefix, CanRemember: true, Access: &access, Changes: &protocol.ChangePreview{Files: []protocol.PreviewFile{
		{Path: "new.txt", Kind: protocol.ChangeKindAdded, Additions: 2},
		{Path: "changed.txt", Kind: protocol.ChangeKindModified, Additions: 4, Deletions: 3, Truncated: true},
		{Path: "old.txt", Destination: &destination, Kind: protocol.ChangeKindRenamed},
		{Path: "deleted.txt", Kind: protocol.ChangeKindDeleted, Deletions: 5},
	}}}
	page := protocol.DefaultConversationPage()
	page.Session = session
	page.Runs = []protocol.ConversationRun{{Turn: turn, Session: session, Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusRunning}}
	page.Entries = []protocol.ConversationEntry{{ID: "call", Turn: turn, Parts: []protocol.ConversationPart{{Kind: "tool_call", Data: call}}}}
	page.Approvals = []protocol.ConversationApproval{approval}
	state := client.ConversationState{Snapshot: protocol.ConversationSnapshot{Page: page}}
	_, _, waiting, _ := Conversation(state, "zh-CN")
	if len(waiting.Approvals) != 1 {
		t.Fatal(waiting)
	}
	got := waiting.Approvals[0]
	if got.ID != id.String() || got.Title != "应用修改" || got.Context != "src" || got.Description != "" || !got.Remember || !got.Unrestricted || got.Prefix != prefix || len(got.Changes) != 4 || len(got.Notices) != 2 || got.Notices[0] != "已捕获提示" || got.Notices[1] != "Fallback notice" {
		t.Fatalf("approval source fields not projected: %+v", got)
	}
	for index, kind := range []composer.ChangeKind{composer.Added, composer.Modified, composer.Renamed, composer.Deleted} {
		if got.Changes[index].Kind != kind {
			t.Fatal("change kind changed", got.Changes)
		}
	}
	if got.Changes[1].Added != 4 || got.Changes[1].Removed != 3 || !got.Changes[1].Truncated || got.Changes[2].Destination != destination {
		t.Fatal("validated change metadata lost", got.Changes)
	}
	_, _, english, _ := Conversation(state)
	if english.Approvals[0].Title != "Apply changes" || english.Approvals[0].Notices[0] != "Captured notice" {
		t.Fatal("locale-free projection lost source fallback", english)
	}
	approval.Changes = nil
	if got := projectApproval(approval, &call, "en"); got.Description != "generic change prompt" || !got.Code {
		t.Fatal("ordinary command approval content lost", got)
	}
}

type pickerViewActions struct {
	composer.Actions
	uploads int
}

func (a *pickerViewActions) Upload(*composer.Attachment) { a.uploads++ }

func testAttachmentPickerAvailability(t *testing.T) {
	for _, fixture := range []struct {
		name                        string
		callbacks                   *composer.AttachmentCallbacks
		preview, readonly, disabled bool
	}{
		{name: "preview chooser", preview: true},
		{name: "native missing", callbacks: &composer.AttachmentCallbacks{}, preview: true, disabled: true},
		{name: "native supplied", callbacks: &composer.AttachmentCallbacks{Pick: func() {}}},
		{name: "readonly", callbacks: &composer.AttachmentCallbacks{Pick: func() {}}, readonly: true, disabled: true},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			comp := composer.New("")
			comp.SetAttachmentCallbacks(fixture.callbacks)
			context := &composer.Context{Readonly: fixture.readonly}
			if fixture.preview {
				context.Files = &composer.Entry{Name: "root", Path: "/"}
			}
			actions := &pickerViewActions{}
			attach := false
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				if attach {
					comp.AttachPaths(c, []string{"/fixture/input.txt"}, actions)
					attach = false
				}
				ui.Column(c).Fill().Children(func() { composer.View(c, comp, context, actions) })
			}, 800, 500)
			tester.SetLocale("en")
			tester.Frame()
			tester.Frame()
			button, ok := tester.AccessByRole(ui.RoleButton, i18n.In("en", "composer_attachment"))
			if !ok || button.Disabled != fixture.disabled {
				t.Fatalf("attach control availability changed: found=%v disabled=%v expected=%v", ok, button.Disabled, fixture.disabled)
			}
			if fixture.name == "native missing" {
				attach = true
				tester.Frame()
				if actions.uploads != 1 || len(comp.Attachments) != 1 {
					t.Fatal("missing chooser disabled independent drop upload")
				}
			}
		})
	}
}
