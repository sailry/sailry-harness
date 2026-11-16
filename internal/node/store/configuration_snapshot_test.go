package store

import (
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func snapshotFixture(t *testing.T) (*Store, protocol.NodeID, protocol.NodeID, context.Context) {
	t.Helper()
	storage, node, controller, ctx := followFixture(t)
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		user, err := CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		_, err = db.ExecContext(ctx, "UPDATE users SET controller=?2 WHERE id=?1", user.ID.String(), controller[:])
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return storage, node, controller, ctx
}

func snapshotPages(t *testing.T, manifest protocol.ConfigurationManifest) []protocol.ConfigurationSnapshotPage {
	t.Helper()
	body, err := json.Marshal(manifest)
	if err != nil {
		t.Fatal(err)
	}
	return snapshotBytePages(t, manifest, body)
}

func snapshotBytePages(t *testing.T, manifest protocol.ConfigurationManifest, body []byte) []protocol.ConfigurationSnapshotPage {
	t.Helper()
	sum := blake3.Sum256(body)
	page := protocol.ConfigurationSnapshotPage{
		Root: protocol.NewRequestID(), Source: manifest.Source, Category: manifest.Category,
		Revision: manifest.Revision, Bytes: uint64(len(body)), Digest: hex.EncodeToString(sum[:]),
	}
	var pages []protocol.ConfigurationSnapshotPage
	for offset := 0; offset < len(body); offset += protocol.ConfigurationPageBytes {
		page.Index = uint64(len(pages))
		page.Data = append([]byte(nil), body[offset:min(offset+protocol.ConfigurationPageBytes, len(body))]...)
		pages = append(pages, page)
	}
	return pages
}

func stageSnapshot(t *testing.T, storage *Store, ctx context.Context, node, source protocol.NodeID, page protocol.ConfigurationSnapshotPage) (protocol.ConfigurationSnapshotProgress, []protocol.Event, error) {
	t.Helper()
	var progress protocol.ConfigurationSnapshotProgress
	var changes []protocol.Event
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		progress, changes, err = StageConfigurationSnapshot(ctx, db, node, source, protocol.ConfigurationPageRequestID(page.Root, page.Index), page)
		if err != nil {
			return err
		}
		_, err = RecordEvents(ctx, db, node, changes)
		return err
	})
	return progress, changes, err
}

func snapshotCacheRecords(t *testing.T, storage *Store, ctx context.Context, source protocol.NodeID) (uint64, []protocol.ConfigurationFollowRecord) {
	t.Helper()
	var revision uint64
	var records []protocol.ConfigurationFollowRecord
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		manifest, err := ReadConfigurationManifest(ctx, db, source, "providers")
		if err != nil {
			return err
		}
		revision, records = manifest.Revision, manifest.Records
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	return revision, records
}

func largeSnapshotRecords(t *testing.T, count int) []protocol.ConfigurationFollowRecord {
	t.Helper()
	var records []protocol.ConfigurationFollowRecord
	for index := range count {
		provider := providerFixture()
		provider.ID = protocol.NewProviderID()
		provider.Name = fmt.Sprintf("Snapshot provider %d", index)
		provider.Revision = 1
		body, err := json.Marshal(provider)
		if err != nil {
			t.Fatal(err)
		}
		// An ignored body field exercises record-size pagination without
		// creating invalid configuration or oversized user-visible events.
		body = append(body[:len(body)-1], []byte(`,"fixture_padding":"`+strings.Repeat("x", 960*1024)+`"}`)...)
		record := protocol.ConfigurationFollowRecord{ID: provider.ID.String(), Revision: provider.Revision, Body: body}
		record.Digest, err = configurationDigest(record)
		if err != nil {
			t.Fatal(err)
		}
		records = append(records, record)
	}
	return records
}

func TestConfigurationSnapshotPublishesOnlyWhenCompleteAndResumesAfterRestart(t *testing.T) {
	storage, node, source, ctx := snapshotFixture(t)
	oldProvider := providerFixture()
	oldProvider.Revision = 1
	old := followRecord(t, oldProvider.ID.String(), 1, oldProvider, nil)
	if err := applyFollowFixture(t, storage, ctx, node, source, "providers", 1, old); err != nil {
		t.Fatal(err)
	}
	next := protocol.ConfigurationManifest{Source: source, Category: "providers", Revision: 2, Records: largeSnapshotRecords(t, 18)}
	pages := snapshotPages(t, next)
	if pages[0].Bytes <= protocol.MAX_FRAME_BYTES || len(pages) < 2 {
		t.Fatal("fixture does not exceed a wire frame")
	}
	progress, changes, err := stageSnapshot(t, storage, ctx, node, source, pages[0])
	if err != nil || progress.NextIndex != 1 || progress.Complete || len(changes) != 0 {
		t.Fatalf("first page published or did not stage: progress=%+v changes=%d error=%v", progress, len(changes), err)
	}
	if revision, records := snapshotCacheRecords(t, storage, ctx, source); revision != 1 || len(records) != 1 || records[0].ID != old.ID {
		t.Fatal("partial snapshot replaced or deleted the current cache")
	}
	if err := storage.Close(); err != nil {
		t.Fatal(err)
	}
	storage = reopenSchemaFixture(t, storage.Profile, node)
	header := pages[0]
	header.Data = nil
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		resumed, err := ReadConfigurationSnapshot(ctx, db, node, source, header)
		if err != nil {
			return err
		}
		if resumed.Root != pages[0].Root || resumed.NextIndex != 1 || resumed.Complete {
			return fmt.Errorf("snapshot progress was not retained after restart: %+v", resumed)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if _, changes, err := stageSnapshot(t, storage, ctx, node, source, pages[0]); err != nil || len(changes) != 0 {
		t.Fatalf("repeated staged page changed data: %d, %v", len(changes), err)
	}
	for index, page := range pages[1:] {
		progress, changes, err = stageSnapshot(t, storage, ctx, node, source, page)
		if err != nil {
			t.Fatalf("page %d: %v", index+1, err)
		}
		if index != len(pages)-2 && (len(changes) != 0 || progress.Complete) {
			t.Fatal("an intermediate page published changes")
		}
	}
	if !progress.Complete || progress.NextIndex != uint64(len(pages)) || len(changes) == 0 {
		t.Fatalf("complete snapshot was not published: %+v, changes=%d", progress, len(changes))
	}
	if revision, records := snapshotCacheRecords(t, storage, ctx, source); revision != 2 || len(records) != len(next.Records) {
		t.Fatal("complete snapshot did not replace the cache")
	}
	var before, after int
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error { return db.QueryRowContext(ctx, "SELECT count(*) FROM events").Scan(&before) }); err != nil {
		t.Fatal(err)
	}
	for _, page := range []protocol.ConfigurationSnapshotPage{pages[0], pages[len(pages)-1]} {
		replayed, changes, err := stageSnapshot(t, storage, ctx, node, source, page)
		if err != nil || !reflect.DeepEqual(replayed, progress) || len(changes) != 0 {
			t.Fatalf("completed page replay was not idempotent: %+v changes=%d %v", replayed, len(changes), err)
		}
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM events").Scan(&after); err != nil {
			return err
		}
		var retained int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM configuration_snapshot_pages WHERE data IS NOT NULL").Scan(&retained); err != nil {
			return err
		}
		if retained != 0 {
			return fmt.Errorf("completed snapshot retained %d payload pages", retained)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if after != before {
		t.Fatal("completed snapshot emitted duplicate events")
	}
}

func TestConfigurationSnapshotRejectsChangedIdentityAndPartialOrdering(t *testing.T) {
	storage, node, source, ctx := snapshotFixture(t)
	manifest := protocol.ConfigurationManifest{Source: source, Category: "providers", Revision: 1, Records: largeSnapshotRecords(t, 2)}
	pages := snapshotPages(t, manifest)
	_, _, err := stageSnapshot(t, storage, ctx, node, source, pages[1])
	if !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
		t.Fatalf("out-of-order page was accepted: %v", err)
	}
	if _, _, err := stageSnapshot(t, storage, ctx, node, source, pages[0]); err != nil {
		t.Fatal(err)
	}
	changed := pages[0]
	changed.Data = append([]byte(nil), changed.Data...)
	changed.Data[len(changed.Data)-1] ^= 1
	_, _, err = stageSnapshot(t, storage, ctx, node, source, changed)
	if !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
		t.Fatalf("same page identity accepted different data: %v", err)
	}
	header := pages[0]
	header.Revision++
	header.Data = nil
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		_, err := ReadConfigurationSnapshot(ctx, db, node, source, header)
		return err
	}); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
		t.Fatalf("same root accepted a different header: %v", err)
	}
	if revision, records := snapshotCacheRecords(t, storage, ctx, source); revision != 0 || len(records) != 0 {
		t.Fatal("invalid pages changed the current cache")
	}
	replacement := pages[0]
	replacement.Root = protocol.NewRequestID()
	if _, _, err := stageSnapshot(t, storage, ctx, node, source, replacement); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		var pending, data int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM configuration_snapshots WHERE state='receiving'").Scan(&pending); err != nil {
			return err
		}
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM configuration_snapshot_pages WHERE data IS NOT NULL").Scan(&data); err != nil {
			return err
		}
		if pending != 1 || data != 1 {
			return fmt.Errorf("superseded snapshot payload remains: %d snapshots, %d pages", pending, data)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if _, _, err := stageSnapshot(t, storage, ctx, node, source, pages[0]); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
		t.Fatalf("superseded root resumed over the replacement: %v", err)
	}
}

func TestConfigurationSnapshotValidatesDigestBeforeDeletingCurrentCache(t *testing.T) {
	for _, corrupt := range []string{"digest", "record", "header", "trailing"} {
		t.Run(corrupt, func(t *testing.T) {
			storage, node, source, ctx := snapshotFixture(t)
			provider := providerFixture()
			old := followRecord(t, provider.ID.String(), 1, provider, nil)
			if err := applyFollowFixture(t, storage, ctx, node, source, "providers", 1, old); err != nil {
				t.Fatal(err)
			}
			next := protocol.ConfigurationManifest{Source: source, Category: "providers", Revision: 2, Records: []protocol.ConfigurationFollowRecord{}}
			if corrupt == "record" {
				provider.ID = protocol.NewProviderID()
				record := followRecord(t, provider.ID.String(), 1, provider, nil)
				record.Digest = strings.Repeat("0", 64)
				next.Records = []protocol.ConfigurationFollowRecord{record}
			}
			body, err := json.Marshal(next)
			if err != nil {
				t.Fatal(err)
			}
			if corrupt == "trailing" {
				body = append(body, []byte(`{}`)...)
			}
			pages := snapshotBytePages(t, next, body)
			if corrupt == "digest" {
				pages[0].Digest = strings.Repeat("0", 64)
			}
			if corrupt == "header" {
				pages[0].Revision++
			}
			_, _, err = stageSnapshot(t, storage, ctx, node, source, pages[0])
			var failure *protocol.Fault
			if !errors.As(err, &failure) || failure.Code != protocol.ErrorCodeInvalidRequest {
				t.Fatalf("corrupt %s snapshot accepted: %v", corrupt, err)
			}
			if revision, records := snapshotCacheRecords(t, storage, ctx, source); revision != 1 || len(records) != 1 || records[0].ID != old.ID {
				t.Fatal("invalid complete snapshot deleted current data")
			}
		})
	}
}

func TestConfigurationSnapshotEmptyReplacementAndRetainedReceipt(t *testing.T) {
	storage, node, source, ctx := snapshotFixture(t)
	provider := providerFixture()
	old := followRecord(t, provider.ID.String(), 1, provider, nil)
	if err := applyFollowFixture(t, storage, ctx, node, source, "providers", 1, old); err != nil {
		t.Fatal(err)
	}
	pages := snapshotPages(t, protocol.ConfigurationManifest{Source: source, Category: "providers", Revision: 2, Records: []protocol.ConfigurationFollowRecord{}})
	progress, changes, err := stageSnapshot(t, storage, ctx, node, source, pages[0])
	if err != nil || !progress.Complete || len(changes) != 2 {
		t.Fatalf("empty replacement did not remove prior data once: %+v %d %v", progress, len(changes), err)
	}
	if revision, records := snapshotCacheRecords(t, storage, ctx, source); revision != 2 || len(records) != 0 {
		t.Fatal("empty snapshot did not clear absent records")
	}
	next := snapshotPages(t, protocol.ConfigurationManifest{Source: source, Category: "providers", Revision: 3, Records: []protocol.ConfigurationFollowRecord{old}})
	if _, _, err := stageSnapshot(t, storage, ctx, node, source, next[0]); err != nil {
		t.Fatal(err)
	}
	// An old final page remains only a receipt after a newer snapshot.
	if _, changes, err := stageSnapshot(t, storage, ctx, node, source, pages[0]); err != nil || len(changes) != 0 {
		t.Fatalf("old receipt changed a newer cache: %v", err)
	}
	if revision, records := snapshotCacheRecords(t, storage, ctx, source); revision != 3 || len(records) != 1 {
		t.Fatal("old completed root reverted a newer snapshot")
	}
}

func TestConfigurationSnapshotReceiptAndPublicationRollbackTogether(t *testing.T) {
	storage, node, source, ctx := snapshotFixture(t)
	pages := snapshotPages(t, protocol.ConfigurationManifest{Source: source, Category: "providers", Revision: 1, Records: []protocol.ConfigurationFollowRecord{}})
	page := pages[0]
	abort := errors.New("fixture transaction interrupted before commit")
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		progress, changes, err := StageConfigurationSnapshot(ctx, db, node, source, protocol.ConfigurationPageRequestID(page.Root, page.Index), page)
		if err != nil {
			return err
		}
		if !progress.Complete {
			return errors.New("fixture did not reach final publication")
		}
		if _, err := RecordEvents(ctx, db, node, changes); err != nil {
			return err
		}
		return abort
	})
	if !errors.Is(err, abort) {
		t.Fatalf("expected transaction interruption, got %v", err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		var receipts, events int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM configuration_snapshots").Scan(&receipts); err != nil {
			return err
		}
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM events").Scan(&events); err != nil {
			return err
		}
		if receipts != 0 || events != 0 {
			return fmt.Errorf("failed transaction retained %d receipts or %d events", receipts, events)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if revision, records := snapshotCacheRecords(t, storage, ctx, source); revision != 0 || len(records) != 0 {
		t.Fatal("failed transaction published its cache version")
	}
	if progress, changes, err := stageSnapshot(t, storage, ctx, node, source, page); err != nil || !progress.Complete || len(changes) != 1 {
		t.Fatalf("same request could not complete after rollback: %+v changes=%d %v", progress, len(changes), err)
	}
}

func TestConfigurationSnapshotUsesAuthenticatedControllerAndExactPageID(t *testing.T) {
	storage, node, source, ctx := snapshotFixture(t)
	page := snapshotPages(t, protocol.ConfigurationManifest{
		Source: source, Category: "providers", Revision: 1, Records: []protocol.ConfigurationFollowRecord{},
	})[0]
	wrongSource := page
	wrongSource.Source = protocol.NodeID{99}
	if _, _, err := stageSnapshot(t, storage, ctx, node, source, wrongSource); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
		t.Fatalf("page accepted an unauthenticated source: %v", err)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		_, _, err := StageConfigurationSnapshot(ctx, db, node, source, protocol.NewRequestID(), page)
		return err
	}); !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
		t.Fatalf("page accepted a new retry identity: %v", err)
	}
	local, err := func() (protocol.User, error) {
		var user protocol.User
		err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			user, err = LocalUser(ctx, db)
			return err
		})
		return user, err
	}()
	if err != nil {
		t.Fatal(err)
	}
	if _, _, err := stageSnapshot(t, storage, WithUser(ctx, local), node, source, page); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
		t.Fatalf("page escaped its controller user namespace: %v", err)
	}
}
