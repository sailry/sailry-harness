package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type personalKVFixture struct {
	store      *Store
	node       protocol.NodeID
	controller protocol.NodeID
	ctx        context.Context
	reference  protocol.PluginReference
}

func newPersonalKVFixture(t *testing.T, controller protocol.NodeID) personalKVFixture {
	t.Helper()
	_, node, storage := schemaFixture(t)
	if controller == node {
		node = protocol.NodeID{2}
		if err := storage.Node.Write(t.Context(), func(db *sql.Conn) error {
			_, err := db.ExecContext(t.Context(), "UPDATE node SET identity=?1 WHERE singleton=1", node[:])
			return err
		}); err != nil {
			t.Fatal(err)
		}
	}
	fixture := personalKVFixture{store: storage, node: node, controller: controller, ctx: t.Context()}
	if err := storage.Node.Write(t.Context(), func(db *sql.Conn) error {
		info := pluginTestInfo("personal-fixture", 'e')
		extension := protocol.DefaultPluginExtension()
		extension.Actions = []protocol.PluginAction{protocol.PluginActionReadStorage, protocol.PluginActionWriteStorage}
		info.Extension = &extension
		if controller != (protocol.NodeID{}) {
			if err := PairPeerIdentity(t.Context(), db, node, controller, link.DirectionController, true); err != nil {
				return err
			}
			user, err := CallerUser(t.Context(), db, node, controller)
			if err != nil {
				return err
			}
			fixture.ctx = WithUser(t.Context(), user)
			info.Summary.Enabled, info.Summary.Revision = true, 1
			if err := CacheConfigurationPackage(fixture.ctx, db, info, protocol.PluginMcpConfiguration{}); err != nil {
				return err
			}
			record := followRecord(t, info.Summary.Name, 1, info, nil)
			if _, err := ApplyConfigurationManifest(fixture.ctx, db, node, controller, protocol.ConfigurationManifest{Source: controller, Category: "plugins", Revision: 1, Records: []protocol.ConfigurationFollowRecord{record}}); err != nil {
				return err
			}
			reference := plugins.PackageReference(info)
			settings := pluginFollowSettings{Package: reference, Values: map[string]json.RawMessage{}, MCP: protocol.PluginMcpConfiguration{}}
			record = followRecord(t, pluginSettingsCacheID(reference), 1, settings, nil)
			if _, err := ApplyConfigurationManifest(fixture.ctx, db, node, controller, protocol.ConfigurationManifest{Source: controller, Category: "plugin_settings", Revision: 1, Records: []protocol.ConfigurationFollowRecord{record}}); err != nil {
				return err
			}
		} else {
			var err error
			info, err = InstallPlugin(t.Context(), db, info, 0, true)
			if err != nil {
				return err
			}
		}
		fixture.reference = plugins.PackageReference(info)
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	return fixture
}

func (fixture personalKVFixture) write(t *testing.T, key string, revision uint64, value string) protocol.PersonalKVGroup {
	t.Helper()
	command := protocol.Command{Kind: "write_plugin_value", Data: &protocol.CommandWritePluginValuePayload{Key: key, Value: json.RawMessage(value), ExpectedRevision: revision}}
	if value == "" {
		command = protocol.Command{Kind: "remove_plugin_value", Data: &protocol.CommandRemovePluginValuePayload{Key: key, ExpectedRevision: revision}}
	}
	request := protocol.NewRequest(fixture.node, command)
	request.Plugin = &protocol.PluginContext{Package: fixture.reference}
	var group protocol.PersonalKVGroup
	if err := fixture.store.Node.Write(fixture.ctx, func(db *sql.Conn) error {
		if _, _, err := ExecutePluginStorageCommand(fixture.ctx, db, fixture.node, "", fixture.controller, request); err != nil {
			return err
		}
		if fixture.controller != (protocol.NodeID{}) {
			user, err := CurrentUser(fixture.ctx, db)
			if err != nil {
				return err
			}
			value, err := readBody[protocol.PersonalKVGroup](fixture.ctx, db, "SELECT body FROM personal_kv_outbox WHERE user=?1 AND request=?2", user.ID.String(), request.ID.String())
			if err != nil {
				return err
			}
			if value == nil {
				return errors.New("transaction did not persist its outbox group")
			}
			group = *value
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	return group
}

func (fixture personalKVFixture) read(t *testing.T, key string) protocol.PluginStorageEntry {
	t.Helper()
	var entry protocol.PluginStorageEntry
	if err := fixture.store.Node.Read(fixture.ctx, func(db *sql.Conn) error {
		var err error
		entry, err = ReadPluginValue(fixture.ctx, db, fixture.reference, key)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return entry
}

func TestPersonalKVSequentialWritesRetainViewRevision(t *testing.T) {
	t.Parallel()
	source := newPersonalKVFixture(t, protocol.NodeID{})
	replica := newPersonalKVFixture(t, source.node)
	groups := []protocol.PersonalKVGroup{
		replica.write(t, "note", 0, `{"text":"first"}`),
		replica.write(t, "note", 1, `{"text":"second"}`),
		replica.write(t, "note", 2, ""),
		replica.write(t, "note", 3, `{"text":"recreated"}`),
	}
	if current := replica.read(t, "note"); current.Revision != 4 || string(current.Value) != `{"text":"recreated"}` {
		t.Fatalf("local writes were not visible: %#v", current)
	}
	for index, group := range groups {
		if index > 0 && (group.Writes[0].Predecessor == nil || *group.Writes[0].Predecessor != groups[index-1].Request) {
			t.Fatal("sequential write lost its pending predecessor")
		}
		var receipt protocol.PersonalKVReceipt
		if err := source.store.Node.Write(source.ctx, func(db *sql.Conn) error {
			var err error
			receipt, _, err = ApplyPersonalKVGroup(source.ctx, db, source.node, replica.node, group)
			if err != nil || !receipt.Applied {
				return fmt.Errorf("source rejected sequential write: %v %#v", err, receipt)
			}
			duplicate, _, err := ApplyPersonalKVGroup(source.ctx, db, source.node, replica.node, group)
			if err != nil || duplicate.Records[0].Revision != receipt.Records[0].Revision {
				return errors.New("lost acknowledgement repeated a source effect")
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
		if err := replica.store.Node.Write(replica.ctx, func(db *sql.Conn) error {
			_, err := AcknowledgePersonalKV(replica.ctx, db, replica.node, source.node, receipt)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if current := replica.read(t, "note"); current.Revision != 4 || string(current.Value) != `{"text":"recreated"}` {
			t.Fatalf("acknowledgement changed the visible write revision: %#v", current)
		}
	}
	if current := source.read(t, "note"); current.Revision != 4 || string(current.Value) != `{"text":"recreated"}` {
		t.Fatalf("source did not converge: %#v", current)
	}
}

func TestPersonalKVRejectsWholeConflictingTransaction(t *testing.T) {
	t.Parallel()
	source := newPersonalKVFixture(t, protocol.NodeID{})
	replica := newPersonalKVFixture(t, source.node)
	source.write(t, "one", 0, `{"owner":"controller"}`)
	request := protocol.NewRequest(replica.node, protocol.Command{Kind: "plugin_transaction", Data: &protocol.CommandPluginTransactionPayload{Operations: []protocol.PluginTransactionOperation{
		{Kind: "write", Data: &protocol.CommandWritePluginValuePayload{Key: "one", ExpectedRevision: 0, Value: json.RawMessage(`{"owner":"replica"}`)}},
		{Kind: "write", Data: &protocol.CommandWritePluginValuePayload{Key: "two", ExpectedRevision: 0, Value: json.RawMessage(`{"owner":"replica"}`)}},
	}}})
	request.Plugin = &protocol.PluginContext{Package: replica.reference}
	var group protocol.PersonalKVGroup
	if err := replica.store.Node.Write(replica.ctx, func(db *sql.Conn) error {
		if _, _, err := ExecutePluginStorageCommand(replica.ctx, db, replica.node, "", source.node, request); err != nil {
			return err
		}
		value, err := ReadPersonalKVPending(replica.ctx, db, replica.node, source.node, nil)
		if err != nil {
			return err
		}
		group = *value.Group
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	dependent := replica.write(t, "two", 1, `{"owner":"dependent"}`)
	var receipt protocol.PersonalKVReceipt
	if err := source.store.Node.Write(source.ctx, func(db *sql.Conn) error {
		var err error
		receipt, _, err = ApplyPersonalKVGroup(source.ctx, db, source.node, replica.node, group)
		if err != nil || receipt.Applied {
			return errors.New("conflicting transaction was partially accepted")
		}
		next, _, err := ApplyPersonalKVGroup(source.ctx, db, source.node, replica.node, dependent)
		if err != nil || next.Applied {
			return errors.New("rejected predecessor did not reject its dependent")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if source.read(t, "two").Present {
		t.Fatal("a nonconflicting key escaped the rejected group")
	}
}

func TestPersonalKVSnapshotPublishesOnlyCompletePages(t *testing.T) {
	t.Parallel()
	source := newPersonalKVFixture(t, protocol.NodeID{})
	replica := newPersonalKVFixture(t, source.node)
	for index := range 70 {
		source.write(t, fmt.Sprintf("record-%03d", index), 0, `{"value":true}`)
	}
	var first, last protocol.PersonalKVSnapshot
	if err := source.store.Node.Read(source.ctx, func(db *sql.Conn) error {
		var err error
		first, err = ReadPersonalKVSnapshot(source.ctx, db, nil, nil)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if first.Next == nil {
		t.Fatal("fixture did not produce multiple pages")
	}
	if err := replica.store.Node.Write(replica.ctx, func(db *sql.Conn) error {
		_, err := StagePersonalKVSnapshot(replica.ctx, db, replica.node, source.node, first)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if replica.read(t, "record-000").Present {
		t.Fatal("partial snapshot was published")
	}
	if err := source.store.Node.Read(source.ctx, func(db *sql.Conn) error {
		var err error
		last, err = ReadPersonalKVSnapshot(source.ctx, db, &first.Generation, first.Next)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := replica.store.Node.Write(replica.ctx, func(db *sql.Conn) error {
		_, err := StagePersonalKVSnapshot(replica.ctx, db, replica.node, source.node, last)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if current := replica.read(t, "record-000"); !current.Present || current.Revision != 1 {
		t.Fatalf("complete snapshot was not published: %#v", current)
	}
}

func TestPersonalKVPreservesJSONBytesAcrossDelivery(t *testing.T) {
	t.Parallel()
	for _, value := range []string{`{ "text": "<tag>&value", "items": [1, 2] }`, `null`, `"<>&"`} {
		t.Run(value, func(t *testing.T) {
			source := newPersonalKVFixture(t, protocol.NodeID{})
			replica := newPersonalKVFixture(t, source.node)
			group := replica.write(t, "note", 0, value)
			if string(group.Writes[0].ValueJSON()) != value {
				t.Fatalf("outbox changed JSON bytes: %q", group.Writes[0].ValueJSON())
			}
			var receipt protocol.PersonalKVReceipt
			if err := source.store.Node.Write(source.ctx, func(db *sql.Conn) error {
				var err error
				receipt, _, err = ApplyPersonalKVGroup(source.ctx, db, source.node, replica.node, group)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			raw, err := json.Marshal(receipt)
			if err != nil || json.Unmarshal(raw, &receipt) != nil {
				t.Fatal("receipt did not round trip")
			}
			if err := replica.store.Node.Write(replica.ctx, func(db *sql.Conn) error {
				_, err := AcknowledgePersonalKV(replica.ctx, db, replica.node, source.node, receipt)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			for _, fixture := range []personalKVFixture{source, replica} {
				if entry := fixture.read(t, "note"); string(entry.Value) != value || entry.Revision != 1 {
					t.Fatalf("delivery changed visible bytes or revision: %#v", entry)
				}
			}
		})
	}
}

func TestPersonalKVSnapshotPreservesPendingOverlay(t *testing.T) {
	t.Parallel()
	source := newPersonalKVFixture(t, protocol.NodeID{})
	replica := newPersonalKVFixture(t, source.node)
	group := replica.write(t, "local", 0, `{"local":true}`)
	source.write(t, "remote", 0, `{ "text": "<>&" }`)
	source.write(t, "local", 0, `{"controller":true}`)
	var page protocol.PersonalKVSnapshot
	if err := source.store.Node.Read(source.ctx, func(db *sql.Conn) error {
		var err error
		page, err = ReadPersonalKVSnapshot(source.ctx, db, nil, nil)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	raw, err := json.Marshal(page)
	if err != nil || json.Unmarshal(raw, &page) != nil {
		t.Fatal("snapshot did not round trip")
	}
	if err := replica.store.Node.Write(replica.ctx, func(db *sql.Conn) error {
		_, err := StagePersonalKVSnapshot(replica.ctx, db, replica.node, source.node, page)
		if err != nil {
			return err
		}
		pending, err := ReadPersonalKVPending(replica.ctx, db, replica.node, source.node, nil)
		if err == nil && (pending.Group == nil || pending.Group.Request != group.Request) {
			return errors.New("snapshot discarded a pending transaction")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if entry := replica.read(t, "local"); string(entry.Value) != `{"local":true}` || entry.Revision != 1 {
		t.Fatalf("snapshot overwrote the local overlay: %#v", entry)
	}
	if entry := replica.read(t, "remote"); string(entry.Value) != `{ "text": "<>&" }` {
		t.Fatalf("pending writes prevented a source update: %#v", entry)
	}
}

func TestPersonalKVReceiptsDeduplicateAndIsolateConflicts(t *testing.T) {
	t.Parallel()
	source := newPersonalKVFixture(t, protocol.NodeID{})
	replica := newPersonalKVFixture(t, source.node)
	source.write(t, "conflict", 0, `true`)
	first := replica.write(t, "conflict", 0, `false`)
	dependent := replica.write(t, "conflict", 1, `null`)
	independent := replica.write(t, "other", 0, `"independent"`)
	for i, group := range []protocol.PersonalKVGroup{first, dependent, independent} {
		if err := source.store.Node.Write(source.ctx, func(db *sql.Conn) error {
			receipt, _, err := ApplyPersonalKVGroup(source.ctx, db, source.node, replica.node, group)
			if err != nil {
				return err
			}
			if receipt.Applied != (i == 2) {
				return fmt.Errorf("unexpected group outcome: %#v", receipt)
			}
			duplicate, events, err := ApplyPersonalKVGroup(source.ctx, db, source.node, replica.node, group)
			if err != nil || len(events) != 0 || duplicate.Digest != receipt.Digest {
				return fmt.Errorf("duplicate receipt repeated effects: %v %#v", err, duplicate)
			}
			changed := group
			changed.Writes = append([]protocol.PersonalKVWrite(nil), group.Writes...)
			changed.Writes[0].Value, changed.Writes[0].ValueBytes = nil, []byte(`"changed"`)
			changed.Digest = ""
			changed.Digest, err = configurationDigest(changed)
			if err != nil {
				return err
			}
			_, _, err = ApplyPersonalKVGroup(source.ctx, db, source.node, replica.node, changed)
			if !providerFixtureFault(err, protocol.ErrorCodeConflict) {
				return fmt.Errorf("same identity with another digest was accepted: %v", err)
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	}
	if entry := source.read(t, "other"); !entry.Present {
		t.Fatal("a rejected dependency blocked an unrelated group")
	}
	if err := source.store.Node.Read(source.ctx, func(db *sql.Conn) error {
		notices, err := ReadNotifications(source.ctx, db)
		if err == nil && len(notices) != 2 {
			return fmt.Errorf("conflict notifications were not deduplicated: %d", len(notices))
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
}

func TestPersonalKVPendingQuotaIsPerPluginAndSurvivesRemoval(t *testing.T) {
	t.Parallel()
	source := newPersonalKVFixture(t, protocol.NodeID{})
	replica := newPersonalKVFixture(t, source.node)
	group := replica.write(t, "note", 0, `"pending"`)
	if err := replica.store.Node.Write(replica.ctx, func(db *sql.Conn) error {
		user, err := CurrentUser(replica.ctx, db)
		if err != nil {
			return err
		}
		if err := personalKVOutboxCapacity(replica.ctx, db, user.ID, "another-plugin", personalKVOutboxLimit); err != nil {
			return fmt.Errorf("another plugin used this quota: %w", err)
		}
		if err := personalKVOutboxCapacity(replica.ctx, db, user.ID, group.Package.Name, personalKVOutboxLimit); !providerFixtureFault(err, protocol.ErrorCodeBusy) {
			return fmt.Errorf("pending quota did not return busy: %v", err)
		}
		if _, err := db.ExecContext(replica.ctx, "DELETE FROM configuration_cache WHERE user=?1 AND category IN ('plugins','plugin_settings')", user.ID.String()); err != nil {
			return err
		}
		pending, err := ReadPersonalKVPending(replica.ctx, db, replica.node, source.node, nil)
		if err == nil && (pending.Group == nil || pending.Group.Request != group.Request) {
			return errors.New("removing a package discarded its pending data")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
}

func TestPersonalKVLegacyGroupRetainsDigest(t *testing.T) {
	t.Parallel()
	source := newPersonalKVFixture(t, protocol.NodeID{})
	replica := newPersonalKVFixture(t, source.node)
	group := replica.write(t, "legacy", 0, `{"old":true}`)
	group.Writes[0].Value = json.RawMessage(`{"old":true}`)
	group.Writes[0].ValueBytes = nil
	group.Digest = ""
	group.Digest, _ = configurationDigest(group)
	body, err := json.Marshal(group)
	if err != nil || strings.Contains(string(body), "value_bytes") || json.Unmarshal(body, &group) != nil {
		t.Fatal("legacy group format changed")
	}
	if err := source.store.Node.Write(source.ctx, func(db *sql.Conn) error {
		receipt, _, err := ApplyPersonalKVGroup(source.ctx, db, source.node, replica.node, group)
		if err == nil && !receipt.Applied {
			return errors.New("legacy transaction was rejected")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
}
