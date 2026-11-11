package store

import (
	"context"
	"database/sql"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/kaptinlin/jsonpointer"
	"lukechampine.com/blake3"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// KnowledgeStoragePageLimit bounds the plugin values of one source page.
const KnowledgeStoragePageLimit = 16

const maxPluginIndexBytes = 64 * 1024

// KnowledgeStorageRecord is one plugin value captured as a knowledge source:
// the indexed entry and the value its text was read from.
type KnowledgeStorageRecord struct {
	Key            string
	Revision       uint64
	Hash           [32]byte
	Title, Body    string
	Source         protocol.PluginStorageSource
	SourceRevision uint64
	SourceHash     [32]byte
	Tags           []string
}

// Identity is the source revision the index and citations carry. It changes
// with the entry, its index data and the text it points to.
func (record KnowledgeStorageRecord) Identity() string {
	sum := blake3.Sum256(append(record.Hash[:], record.SourceHash[:]...))
	return hex.EncodeToString(sum[:])
}

// KnowledgeStoragePage is one page of captured plugin values. After continues
// the scan; skipped keys still advance it.
type KnowledgeStoragePage struct {
	Records []KnowledgeStorageRecord
	After   *string
}

type knowledgeRaw struct {
	revision uint64
	body     []byte
	index    []byte
}

func (raw knowledgeRaw) hash() [32]byte {
	hasher := blake3.New(32, nil)
	var size [8]byte
	binary.LittleEndian.PutUint64(size[:], uint64(len(raw.body)))
	hasher.Write(size[:])
	hasher.Write(raw.body)
	hasher.Write(raw.index)
	var sum [32]byte
	hasher.Sum(sum[:0])
	return sum
}

func knowledgeStorageCancelled(ctx context.Context) error {
	if ctx.Err() != nil {
		return fault(protocol.ErrorCodeCancelled, "knowledge source read was cancelled")
	}
	return nil
}

func knowledgeStorageKey(key string) bool {
	return key != "" && len(key) <= maxPluginKeyBytes && !strings.ContainsRune(key, 0)
}

// knowledgeCurrent reports whether the captured collection is still the
// stored one.
func knowledgeCurrent(ctx context.Context, db *sql.Conn, stored KnowledgeStored) (bool, error) {
	entries, err := loadKnowledge(ctx, db)
	if err != nil {
		return false, err
	}
	return slices.ContainsFunc(entries, func(current KnowledgeStored) bool { return SameKnowledgeStored(current, stored) }), nil
}

func knowledgeNamespace(stored KnowledgeStored) (string, KnowledgeSource, error) {
	source := ReadKnowledgeSource(stored.Collection.Definition.Source)
	if source.Kind != "storage" {
		return "", source, invalid("knowledge storage source expected")
	}
	if stored.Capture.Package == nil {
		return "", source, invalid("knowledge storage owner is missing")
	}
	if source.Scope == protocol.PluginStorageScopeConversation && stored.Capture.Session == nil {
		return "", source, invalid("knowledge storage session is missing")
	}
	return stored.Capture.Package.Name, source, nil
}

func readKnowledgeRaw(ctx context.Context, db *sql.Conn, stored KnowledgeStored, key string) (*knowledgeRaw, error) {
	if !knowledgeStorageKey(key) {
		return nil, invalid("invalid knowledge storage key")
	}
	name, source, err := knowledgeNamespace(stored)
	if err != nil {
		return nil, err
	}
	var raw knowledgeRaw
	var revision int64
	if source.Scope == protocol.PluginStorageScopeNode {
		err = db.QueryRowContext(ctx, "SELECT revision,value,index_data FROM plugin_values WHERE name=?1 AND key=?2 AND value IS NOT NULL", name, key).
			Scan(&revision, &raw.body, &raw.index)
	} else {
		err = db.QueryRowContext(ctx, "SELECT v.revision,r.value FROM plugin_conversation_values v JOIN plugin_conversation_records r ON r.sequence=v.record WHERE v.name=?1 AND v.session=?2 AND v.key=?3 AND r.value IS NOT NULL",
			name, stored.Capture.Session.String(), key).Scan(&revision, &raw.body)
	}
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	if len(raw.body) > plugins.MaxValueBytes || len(raw.index) > maxPluginIndexBytes {
		return nil, invalid("knowledge storage source exceeds its size limit")
	}
	if revision <= 0 {
		return nil, errors.New("invalid plugin value revision")
	}
	raw.revision = uint64(revision)
	return &raw, nil
}

// pointedText follows a JSON pointer to raw text.
func pointedText(body []byte, pointer string) (string, bool) {
	var document any
	if json.Unmarshal(body, &document) != nil {
		return "", false
	}
	if pointer != "" {
		value, err := jsonpointer.Value(document, pointer)
		if err != nil {
			return "", false
		}
		document = value
	}
	text, found := document.(string)
	return text, found
}

func captureKnowledgeValue(ctx context.Context, db *sql.Conn, stored KnowledgeStored, key string) (*KnowledgeStorageRecord, error) {
	raw, err := readKnowledgeRaw(ctx, db, stored, key)
	if err != nil || raw == nil {
		return nil, err
	}
	var value any
	if err := json.Unmarshal(raw.body, &value); err != nil {
		return nil, err
	}
	var index *protocol.PluginStorageIndex
	if raw.index != nil {
		index = new(protocol.PluginStorageIndex)
		if err := json.Unmarshal(raw.index, index); err != nil {
			return nil, err
		}
		if err := plugins.ValidateStorageIndex(*index); err != nil {
			return nil, err
		}
	}
	source := protocol.PluginStorageSource{Key: key}
	pointed := index != nil && index.Source != nil
	if pointed {
		source = *index.Source
	}
	original, err := readKnowledgeRaw(ctx, db, stored, source.Key)
	if err != nil || original == nil {
		return nil, err
	}
	var body string
	if pointed {
		text, found := pointedText(original.body, source.Pointer)
		if !found {
			return nil, invalid("knowledge source pointer must select raw text")
		}
		body = text
	} else if text, found := value.(string); found {
		body = text
	} else {
		if !utf8.Valid(raw.body) {
			return nil, errors.New("plugin value is not UTF-8")
		}
		body = string(raw.body)
	}
	if len(body) > plugins.MaxValueBytes || strings.ContainsRune(body, 0) {
		return nil, invalid("knowledge source text exceeds its size limit")
	}
	title := key
	if object, found := value.(map[string]any); found {
		if text, found := object["title"].(string); found {
			title = text
		} else if _, titled := object["title"]; !titled {
			if text, found := object["name"].(string); found {
				title = text
			}
		}
	}
	record := &KnowledgeStorageRecord{
		Key: key, Revision: raw.revision, Hash: raw.hash(), Title: title, Body: body,
		Source: source, SourceRevision: original.revision, SourceHash: blake3.Sum256(original.body),
	}
	if index != nil {
		record.Tags = index.Tags
	}
	return record, nil
}

// ReadKnowledgeStoragePage reads one page of the plugin values a storage
// collection indexes, in key order.
func ReadKnowledgeStoragePage(ctx context.Context, db *sql.Conn, stored KnowledgeStored, after *string, limit int) (KnowledgeStoragePage, error) {
	var page KnowledgeStoragePage
	if err := knowledgeStorageCancelled(ctx); err != nil {
		return page, err
	}
	if limit < 1 || limit > KnowledgeStoragePageLimit || after != nil && !knowledgeStorageKey(*after) {
		return page, invalid("invalid knowledge source page")
	}
	current, err := knowledgeCurrent(ctx, db, stored)
	if err != nil {
		return page, err
	}
	if !current {
		return page, fault(protocol.ErrorCodeRevisionConflict, "knowledge collection changed")
	}
	var keys []string
	var capture func(key string) (*KnowledgeStorageRecord, error)
	if stored.Automatic {
		if keys, capture, err = semanticSources(ctx, db, stored, after, limit); err != nil {
			return page, err
		}
	} else {
		name, source, err := knowledgeNamespace(stored)
		if err != nil {
			return page, err
		}
		query := "SELECT key FROM plugin_values WHERE name=?1 AND value IS NOT NULL AND substr(key,1,length(?2))=?2 AND (?3 IS NULL OR key>?3) ORDER BY key LIMIT ?4"
		arguments := []any{name, source.Prefix, after, limit + 1}
		if source.Scope == protocol.PluginStorageScopeConversation {
			query = "SELECT v.key FROM plugin_conversation_values v JOIN plugin_conversation_records r ON r.sequence=v.record WHERE v.name=?1 AND r.value IS NOT NULL AND substr(v.key,1,length(?2))=?2 AND (?3 IS NULL OR v.key>?3) AND v.session=?5 ORDER BY v.key LIMIT ?4"
			arguments = append(arguments, stored.Capture.Session.String())
		}
		if keys, err = knowledgeKeys(ctx, db, query, arguments...); err != nil {
			return page, err
		}
		capture = func(key string) (*KnowledgeStorageRecord, error) { return captureKnowledgeValue(ctx, db, stored, key) }
	}
	for position, key := range keys {
		if err := knowledgeStorageCancelled(ctx); err != nil {
			return page, err
		}
		if position == limit {
			page.After = &keys[position-1]
			break
		}
		record, err := capture(key)
		if err != nil {
			return page, err
		}
		if record != nil {
			page.Records = append(page.Records, *record)
		}
	}
	return page, nil
}

func knowledgeKeys(ctx context.Context, db *sql.Conn, query string, arguments ...any) ([]string, error) {
	rows, err := db.QueryContext(ctx, query, arguments...)
	if err != nil {
		return nil, err
	}
	var keys []string
	for rows.Next() {
		var key string
		if err := rows.Scan(&key); err != nil {
			rows.Close()
			return nil, err
		}
		keys = append(keys, key)
	}
	return keys, errors.Join(rows.Err(), rows.Close())
}

// KnowledgeStorageUnchanged rechecks a captured record before its chunks are
// published or its citation is returned.
func KnowledgeStorageUnchanged(ctx context.Context, db *sql.Conn, stored KnowledgeStored, record KnowledgeStorageRecord) (bool, error) {
	if err := knowledgeStorageCancelled(ctx); err != nil {
		return false, err
	}
	current, err := knowledgeCurrent(ctx, db, stored)
	if err != nil || !current {
		return false, err
	}
	if stored.Automatic {
		latest, err := semanticSource(ctx, db, stored, record.Key)
		if err != nil || latest == nil {
			return false, err
		}
		return latest.Revision == record.Revision && latest.Hash == record.Hash && latest.Source == record.Source &&
			latest.SourceRevision == record.SourceRevision && latest.SourceHash == record.SourceHash &&
			latest.Title == record.Title && latest.Body == record.Body && slices.Equal(latest.Tags, record.Tags), nil
	}
	_, source, err := knowledgeNamespace(stored)
	if err != nil {
		return false, err
	}
	if !strings.HasPrefix(record.Key, source.Prefix) {
		return false, nil
	}
	metadata, err := readKnowledgeRaw(ctx, db, stored, record.Key)
	if err != nil || metadata == nil {
		return false, err
	}
	original, err := readKnowledgeRaw(ctx, db, stored, record.Source.Key)
	if err != nil || original == nil {
		return false, err
	}
	return metadata.revision == record.Revision && metadata.hash() == record.Hash &&
		original.revision == record.SourceRevision && blake3.Sum256(original.body) == record.SourceHash, nil
}

// ReadKnowledgeStorageRecord captures one plugin value of a storage
// collection, or nil when it no longer exists.
func ReadKnowledgeStorageRecord(ctx context.Context, db *sql.Conn, stored KnowledgeStored, key string) (*KnowledgeStorageRecord, error) {
	if err := knowledgeStorageCancelled(ctx); err != nil {
		return nil, err
	}
	current, err := knowledgeCurrent(ctx, db, stored)
	if err != nil {
		return nil, err
	}
	if !current {
		return nil, fault(protocol.ErrorCodeRevisionConflict, "knowledge collection changed")
	}
	if stored.Automatic {
		return semanticSource(ctx, db, stored, key)
	}
	_, source, err := knowledgeNamespace(stored)
	if err != nil {
		return nil, err
	}
	if !knowledgeStorageKey(key) || !strings.HasPrefix(key, source.Prefix) {
		return nil, invalid("knowledge key is outside its source")
	}
	return captureKnowledgeValue(ctx, db, stored, key)
}
