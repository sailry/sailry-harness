package store

import (
	"database/sql"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func testPluginActivityReads(t *testing.T) {
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	text := strings.Repeat("Activity 中文 🙂 ", 80)
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		info := pluginTestInfo("activity-fixture", 'a')
		info.Summary.Enabled = true
		info.Extension = new(protocol.DefaultPluginExtension())
		info.Extension.Actions = []protocol.PluginAction{protocol.PluginActionReadActivity}
		scope := protocol.DefaultPluginContext()
		scope.Package = plugins.PackageReference(info)
		scope.Surface.Kind = "workspace"
		for _, kind := range []string{"read_activity_catalog", "read_activity"} {
			for _, scenario := range []struct {
				name   string
				change func(*protocol.PluginContext, *protocol.PluginInfo)
				code   protocol.ErrorCode
			}{
				{"Node-wide declared action", nil, ""},
				{"missing activity action", func(_ *protocol.PluginContext, value *protocol.PluginInfo) { value.Extension.Actions = nil }, protocol.ErrorCodePermissionDenied},
				{"disabled package", func(_ *protocol.PluginContext, value *protocol.PluginInfo) { value.Summary.Enabled = false }, protocol.ErrorCodeNotConfigured},
				{"captured worktree", func(value *protocol.PluginContext, _ *protocol.PluginInfo) { value.Worktree = &session.Worktree }, protocol.ErrorCodePermissionDenied},
				{"captured session", func(value *protocol.PluginContext, _ *protocol.PluginInfo) { value.Session = &session.ID }, protocol.ErrorCodePermissionDenied},
				{"captured turn", func(value *protocol.PluginContext, _ *protocol.PluginInfo) { value.Turn = &turn }, protocol.ErrorCodePermissionDenied},
				{"captured invocation", func(value *protocol.PluginContext, _ *protocol.PluginInfo) {
					value.Invocation = new(protocol.NewRequestID())
				}, protocol.ErrorCodePermissionDenied},
			} {
				captured, current := scope, info
				extension := *info.Extension
				current.Extension = &extension
				if scenario.change != nil {
					scenario.change(&captured, &current)
				}
				err := CheckPluginRequest(ctx, db, protocol.Request{Plugin: &captured, Command: protocol.Command{Kind: kind}}, current)
				var failure *protocol.Fault
				if scenario.code == "" && err != nil || scenario.code != "" && (!errors.As(err, &failure) || failure.Code != scenario.code) {
					return fmt.Errorf("%s %s: %w", kind, scenario.name, err)
				}
			}
		}
		for _, author := range []canonical.Role{canonical.Assistant, canonical.User} {
			content := "Private user input"
			if author == canonical.Assistant {
				content = text
			}
			record := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(author, canonical.Text(0, content))), now)
			record.Provenance.Author = string(author)
			if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
				return err
			}
		}
		for _, index := range []byte{2, 1} {
			terminal := protocol.DefaultTerminalInfo()
			terminal.ID, terminal.Worktree, terminal.Revision = protocol.TerminalID{index}, &session.Worktree, 1
			terminal.Status.Kind = "stopped"
			if _, err := SaveTerminal(ctx, db, terminal, false); err != nil {
				return err
			}
		}
		_, err := RecordEvents(ctx, db, node, []protocol.Event{{Kind: "session_changed", Data: session}})
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	err = storage.Node.Read(ctx, func(db *sql.Conn) error {
		catalog, err := ReadActivityCatalog(ctx, db, node)
		if err != nil {
			return err
		}
		snapshot, err := Snapshot(ctx, db, node)
		if err != nil {
			return err
		}
		if catalog.Node != snapshot.Node || catalog.Cursor != snapshot.Cursor || !reflect.DeepEqual(catalog.Projects, snapshot.Projects) ||
			!reflect.DeepEqual(catalog.Worktrees, snapshot.Worktrees) || !reflect.DeepEqual(catalog.Sessions, snapshot.Sessions) ||
			len(catalog.Terminals) != 2 || catalog.Terminals[0].ID != (protocol.TerminalID{1}) || catalog.Terminals[1].ID != (protocol.TerminalID{2}) {
			return errors.New("activity catalog did not preserve canonical resources and terminal identity order")
		}
		previews, err := ReadActivityPreviews(ctx, db, []protocol.SessionID{session.ID, session.ID})
		if err != nil {
			return err
		}
		want := protocol.ActivityPreview{Session: session.ID, Turn: &turn, Text: new(string([]rune(text)[:512]))}
		if !reflect.DeepEqual(previews, []protocol.ActivityPreview{want, want}) {
			return fmt.Errorf("bounded canonical activity previews: %+v", previews)
		}
		for _, scenario := range []struct {
			sessions []protocol.SessionID
			code     protocol.ErrorCode
		}{
			{make([]protocol.SessionID, 33), protocol.ErrorCodeInvalidRequest},
			{[]protocol.SessionID{protocol.NewSessionID()}, protocol.ErrorCodeNotFound},
		} {
			_, err := ReadActivityPreviews(ctx, db, scenario.sessions)
			var failure *protocol.Fault
			if !errors.As(err, &failure) || failure.Code != scenario.code {
				return fmt.Errorf("invalid activity preview batch: %w", err)
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	err = storage.Node.Write(ctx, func(db *sql.Conn) error {
		terminal, err := ReadTerminal(ctx, db, protocol.TerminalID{1})
		if err != nil {
			return err
		}
		terminal.Title, terminal.Revision = new(strings.Repeat("a", 8*1024*1024)), terminal.Revision+1
		_, err = SaveTerminal(ctx, db, terminal, false)
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	err = storage.Node.Read(ctx, func(db *sql.Conn) error {
		_, err := ReadActivityCatalog(ctx, db, node)
		var failure *protocol.Fault
		if !errors.As(err, &failure) || failure.Code != protocol.ErrorCodeInvalidRequest || failure.Message != "activity catalog exceeds the response limit" {
			return fmt.Errorf("oversized activity catalog: %w", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
