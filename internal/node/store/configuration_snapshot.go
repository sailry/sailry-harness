package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"math"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type configurationSnapshot struct {
	Source   protocol.NodeID
	Category string
	Revision uint64
	Bytes    uint64
	Digest   string
	Progress protocol.ConfigurationSnapshotProgress
	State    string
}

func configurationPageCount(size uint64) uint64 {
	return (size + protocol.ConfigurationPageBytes - 1) / protocol.ConfigurationPageBytes
}

func validateConfigurationSnapshot(page protocol.ConfigurationSnapshotPage) error {
	kind, category := configurationKind(page.Category)
	category = category && !kind.LocalOnly
	if page.Category == protocol.PersonalKVAcknowledgementCategory {
		category = page.Bytes <= MaxPersonalKVTransferBytes
	}
	digest, err := hex.DecodeString(page.Digest)
	if page.Root == (protocol.RequestID{}) || !category || page.Revision == 0 || page.Revision > math.MaxInt64 ||
		page.Bytes == 0 || page.Bytes > protocol.MaxConfigurationSnapshotBytes || err != nil || len(digest) != 32 {
		return invalid("configuration snapshot header is invalid")
	}
	return nil
}

func readConfigurationSnapshot(ctx context.Context, db *sql.Conn, user protocol.UserID, page protocol.ConfigurationSnapshotPage) (*configurationSnapshot, error) {
	value := &configurationSnapshot{Progress: protocol.ConfigurationSnapshotProgress{Root: page.Root}}
	var source []byte
	err := db.QueryRowContext(ctx, `SELECT source,category,revision,bytes,digest,next_page,state
FROM configuration_snapshots WHERE user=?1 AND root=?2`, user.String(), page.Root.String()).
		Scan(&source, &value.Category, &value.Revision, &value.Bytes, &value.Digest, &value.Progress.NextIndex, &value.State)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	if len(source) != len(value.Source) {
		return nil, errors.New("invalid stored configuration snapshot source")
	}
	copy(value.Source[:], source)
	if value.Source != page.Source || value.Category != page.Category || value.Revision != page.Revision ||
		value.Bytes != page.Bytes || value.Digest != page.Digest {
		return nil, fault(protocol.ErrorCodeRevisionConflict, "configuration snapshot identity was reused for different content")
	}
	if value.State == "superseded" {
		return nil, fault(protocol.ErrorCodeRevisionConflict, "configuration snapshot was superseded")
	}
	value.Progress.Complete = value.State == "complete"
	return value, nil
}

func configurationSnapshotUser(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, page protocol.ConfigurationSnapshotPage) (protocol.User, error) {
	if err := validateConfigurationSnapshot(page); err != nil {
		return protocol.User{}, err
	}
	user, err := configurationReceiver(ctx, db, node, caller, page.Source)
	if err != nil {
		return user, err
	}
	if user.Controller == nil {
		status, err := ReadConfigurationFollow(ctx, db)
		if err != nil {
			return user, err
		}
		if status.Source != nil && *status.Source != caller {
			return user, fault(protocol.ErrorCodePermissionDenied, "configuration belongs to a different source")
		}
	}
	return user, nil
}

// ReadConfigurationSnapshot reports committed progress, including after a
// lost final reply. It never creates or publishes a partial snapshot.
func ReadConfigurationSnapshot(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, page protocol.ConfigurationSnapshotPage) (protocol.ConfigurationSnapshotProgress, error) {
	progress := protocol.ConfigurationSnapshotProgress{Root: page.Root}
	if page.Index != 0 || len(page.Data) != 0 {
		return progress, invalid("configuration snapshot query must contain only its header")
	}
	user, err := configurationSnapshotUser(ctx, db, node, caller, page)
	if err != nil {
		return progress, err
	}
	value, err := readConfigurationSnapshot(ctx, db, user.ID, page)
	if err != nil || value == nil {
		return progress, err
	}
	return value.Progress, nil
}

// StageConfigurationSnapshot is an internal cache transfer, outside the
// business-effect ledger. Page digests and the completion receipt are persisted
// with their data so retrying the exact page is safe after an unknown commit.
func StageConfigurationSnapshot(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, request protocol.RequestID, page protocol.ConfigurationSnapshotPage) (protocol.ConfigurationSnapshotProgress, []protocol.Event, error) {
	progress := protocol.ConfigurationSnapshotProgress{Root: page.Root}
	user, err := configurationSnapshotUser(ctx, db, node, caller, page)
	if err != nil {
		return progress, nil, err
	}
	count := configurationPageCount(page.Bytes)
	if request != protocol.ConfigurationPageRequestID(page.Root, page.Index) || page.Index >= count {
		return progress, nil, invalid("configuration snapshot page identity is invalid")
	}
	size := uint64(protocol.ConfigurationPageBytes)
	if page.Index == count-1 {
		size = page.Bytes - page.Index*protocol.ConfigurationPageBytes
	}
	if uint64(len(page.Data)) != size {
		return progress, nil, invalid("configuration snapshot page size is invalid")
	}
	value, err := readConfigurationSnapshot(ctx, db, user.ID, page)
	if err != nil {
		return progress, nil, err
	}
	sum := blake3.Sum256(page.Data)
	digest := hex.EncodeToString(sum[:])
	if value != nil {
		progress = value.Progress
		if page.Index < progress.NextIndex {
			var previous string
			if err := db.QueryRowContext(ctx, `SELECT digest FROM configuration_snapshot_pages
WHERE user=?1 AND root=?2 AND page=?3`, user.ID.String(), page.Root.String(), page.Index).Scan(&previous); err != nil {
				return progress, nil, err
			}
			if previous != digest {
				return progress, nil, fault(protocol.ErrorCodeRevisionConflict, "configuration snapshot page content changed")
			}
			return progress, nil, nil
		}
	}
	if page.Index != progress.NextIndex || progress.Complete {
		return progress, nil, fault(protocol.ErrorCodeRevisionConflict, "configuration snapshot page is out of order")
	}
	if value == nil {
		// Superseded roots retain only metadata and page hashes. An old page
		// cannot revive them or reuse their identities for other content.
		if _, err := db.ExecContext(ctx, `UPDATE configuration_snapshot_pages SET data=NULL
WHERE user=?1 AND root IN (SELECT root FROM configuration_snapshots WHERE user=?1 AND source=?2 AND category=?3 AND state='receiving')`,
			user.ID.String(), caller[:], page.Category); err != nil {
			return progress, nil, err
		}
		if _, err := db.ExecContext(ctx, `UPDATE configuration_snapshots SET state='superseded'
WHERE user=?1 AND source=?2 AND category=?3 AND state='receiving'`, user.ID.String(), caller[:], page.Category); err != nil {
			return progress, nil, err
		}
		if _, err := db.ExecContext(ctx, `INSERT INTO configuration_snapshots(user,root,source,category,revision,bytes,digest)
VALUES(?1,?2,?3,?4,?5,?6,?7)`, user.ID.String(), page.Root.String(), caller[:], page.Category, page.Revision, page.Bytes, page.Digest); err != nil {
			return progress, nil, err
		}
	}
	if _, err := db.ExecContext(ctx, `INSERT INTO configuration_snapshot_pages(user,root,page,digest,data)
VALUES(?1,?2,?3,?4,?5)`, user.ID.String(), page.Root.String(), page.Index, digest, page.Data); err != nil {
		return progress, nil, err
	}
	progress.NextIndex++
	var changes []protocol.Event
	if progress.NextIndex == count {
		if page.Category == protocol.PersonalKVAcknowledgementCategory {
			var receipt protocol.PersonalKVReceipt
			receipt, err = assemblePersonalKVAcknowledgement(ctx, db, user.ID, page)
			if err == nil {
				changes, err = AcknowledgePersonalKV(ctx, db, node, caller, receipt)
			}
		} else {
			var manifest protocol.ConfigurationManifest
			manifest, err = assembleConfigurationSnapshot(ctx, db, user.ID, page)
			if err == nil {
				changes, err = ApplyConfigurationManifest(ctx, db, node, caller, manifest)
			}
		}
		if err != nil {
			return progress, nil, err
		}
		progress.Complete = true
		// Only hashes are needed for duplicate-page checks once published.
		if _, err := db.ExecContext(ctx, `UPDATE configuration_snapshot_pages SET data=NULL
WHERE user=?1 AND root=?2`, user.ID.String(), page.Root.String()); err != nil {
			return progress, nil, err
		}
	}
	state := "receiving"
	if progress.Complete {
		state = "complete"
	}
	_, err = db.ExecContext(ctx, `UPDATE configuration_snapshots SET next_page=?3,state=?4
WHERE user=?1 AND root=?2`, user.ID.String(), page.Root.String(), progress.NextIndex, state)
	return progress, changes, err
}

// configurationSnapshotReader streams the durable chunks through the JSON
// decoder without assembling another whole-snapshot byte buffer.
type configurationSnapshotReader struct {
	ctx   context.Context
	db    *sql.Conn
	user  protocol.UserID
	root  protocol.RequestID
	count uint64
	next  uint64
	part  *bytes.Reader
}

func (reader *configurationSnapshotReader) Read(target []byte) (int, error) {
	for {
		if reader.part != nil {
			n, err := reader.part.Read(target)
			if err != io.EOF {
				return n, err
			}
			reader.part = nil
		}
		if reader.next == reader.count {
			return 0, io.EOF
		}
		var data []byte
		if err := reader.db.QueryRowContext(reader.ctx, `SELECT data FROM configuration_snapshot_pages
WHERE user=?1 AND root=?2 AND page=?3`, reader.user.String(), reader.root.String(), reader.next).Scan(&data); err != nil {
			return 0, err
		}
		reader.next++
		reader.part = bytes.NewReader(data)
	}
}

func assembleConfigurationSnapshot(ctx context.Context, db *sql.Conn, user protocol.UserID, page protocol.ConfigurationSnapshotPage) (protocol.ConfigurationManifest, error) {
	var manifest protocol.ConfigurationManifest
	reader := &configurationSnapshotReader{ctx: ctx, db: db, user: user, root: page.Root, count: configurationPageCount(page.Bytes)}
	digest := blake3.New(32, nil)
	decoder := json.NewDecoder(io.TeeReader(reader, digest))
	// Decode records individually: encoding/json otherwise buffers the entire
	// top-level object before decoding a potentially multi-GiB record list.
	token, err := decoder.Token()
	if err != nil || token != json.Delim('{') {
		return manifest, invalid("configuration snapshot JSON is invalid")
	}
	seen := map[string]bool{}
	for decoder.More() {
		key, err := decoder.Token()
		if err != nil {
			return manifest, err
		}
		name, ok := key.(string)
		if !ok || seen[name] {
			return manifest, invalid("configuration snapshot repeats a field")
		}
		seen[name] = true
		switch name {
		case "source":
			err = decoder.Decode(&manifest.Source)
		case "category":
			err = decoder.Decode(&manifest.Category)
		case "revision":
			err = decoder.Decode(&manifest.Revision)
		case "records":
			token, err = decoder.Token()
			if err == nil && token == nil {
				manifest.Records = nil
				break
			}
			if err == nil && token != json.Delim('[') {
				err = invalid("configuration snapshot records must be an array")
			}
			for err == nil && decoder.More() {
				if len(manifest.Records) >= 4096 {
					return manifest, invalid("configuration snapshot has too many records")
				}
				var record protocol.ConfigurationFollowRecord
				if err = decoder.Decode(&record); err == nil {
					manifest.Records = append(manifest.Records, record)
				}
			}
			if err == nil {
				_, err = decoder.Token()
			}
		default:
			err = invalid("configuration snapshot contains an unsupported field")
		}
		if err != nil {
			return manifest, err
		}
	}
	if _, err := decoder.Token(); err != nil {
		return manifest, err
	}
	var extra any
	if err := decoder.Decode(&extra); err != io.EOF {
		return manifest, invalid("configuration snapshot contains trailing data")
	}
	if len(seen) != 4 || manifest.Source != page.Source || manifest.Category != page.Category || manifest.Revision != page.Revision ||
		hex.EncodeToString(digest.Sum(nil)) != page.Digest {
		return manifest, invalid("configuration snapshot digest or header does not match")
	}
	return manifest, validateConfigurationManifest(manifest)
}
