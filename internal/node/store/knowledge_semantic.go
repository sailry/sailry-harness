package store

import (
	"context"
	"database/sql"
	"encoding/binary"
	"encoding/json"
	"errors"
	"math"
	"slices"
	"strings"
	"time"

	"lukechampine.com/blake3"

	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Plugin storage bounds shared with the value writers.
const (
	maxPluginKeyBytes = 128
	maxPluginKeys     = 4096
	maxPluginSearch   = 512 * 1024
)

// SemanticPlan captures what one plugin storage search may rank: the
// package, its automatic collections, the keyword ranking and the keys whose
// text is indexed.
type SemanticPlan struct {
	Package     protocol.PluginReference
	Collections []KnowledgeStored
	Keyword     []PluginValueRank
	Eligible    map[string]bool
	Embedding   protocol.ModelsLocalChoice
}

// SemanticStamp pins a semantic hit to the source revision it was indexed at.
type SemanticStamp struct {
	Collection protocol.KnowledgeID
	Key        string
	Revision   string
}

// SemanticSelection is the fused key order of one search.
type SemanticSelection struct {
	Keys     []string
	Stamps   []SemanticStamp
	Mode     protocol.KnowledgeMode
	Indexing bool
}

func semanticCancelled(ctx context.Context) error {
	if ctx.Err() != nil {
		return fault(protocol.ErrorCodeCancelled, "semantic storage search was cancelled")
	}
	return nil
}

// KnowledgeStorageCommand reports a command that may change the plugin
// values a storage collection indexes.
func KnowledgeStorageCommand(command protocol.Command) bool {
	if input, found := command.Data.(*protocol.CommandPluginTransactionPayload); found {
		return slices.ContainsFunc(input.Operations, func(operation protocol.PluginTransactionOperation) bool {
			switch operation.Kind {
			case "write", "index", "remove", "conversation_write", "conversation_remove", "continue":
				return true
			}
			return false
		})
	}
	switch command.Kind {
	case "write_plugin_value", "write_indexed_plugin_value", "remove_plugin_value",
		"write_plugin_conversation_value", "remove_plugin_conversation_value", "continue_turn":
		return true
	}
	return false
}

// semanticDeclarations lists the Node collections a captured package opts
// into semantic recall for.
func semanticDeclarations(ctx context.Context, db *sql.Conn, reference protocol.PluginReference) ([]protocol.PluginStorageCollection, error) {
	info, err := ResolvePluginInfo(ctx, db, reference)
	if err != nil {
		return nil, err
	}
	if info.Extension == nil || info.Extension.Storage == nil || !slices.Contains(info.Extension.Actions, protocol.PluginActionReadStorage) {
		return nil, nil
	}
	var declarations []protocol.PluginStorageCollection
	for _, collection := range info.Extension.Storage.Collections {
		if collection.Scope == protocol.PluginStorageScopeNode && collection.Index != nil && collection.Index.Semantic {
			declarations = append(declarations, collection)
		}
	}
	return declarations, nil
}

func semanticSelector(collection protocol.PluginStorageCollection) (selector string, exact bool) {
	if collection.Key != nil {
		return *collection.Key, true
	}
	if collection.Prefix != nil {
		return *collection.Prefix, false
	}
	return "", false
}

func semanticMatches(collection protocol.PluginStorageCollection, key string) bool {
	selector, exact := semanticSelector(collection)
	if exact {
		return key == selector
	}
	return collection.Prefix != nil && strings.HasPrefix(key, selector)
}

// semanticIdentifier derives the stable collection ID of one declaration.
func semanticIdentifier(name, selector string, exact bool) protocol.KnowledgeID {
	material := binary.LittleEndian.AppendUint64(nil, uint64(len(name)))
	material = append(material, name...)
	material = append(material, "node"...)
	flag := byte(0)
	if exact {
		flag = 1
	}
	material = append(append(material, flag), selector...)
	var sum [32]byte
	blake3.DeriveKey(sum[:], "Sailry semantic storage collection v1", material)
	var id protocol.KnowledgeID
	copy(id[:], sum[:16])
	id[6] = id[6]&0x0f | 0x80
	id[8] = id[8]&0x3f | 0x80
	return id
}

// knowledgeAutomatic reports a registration derived from a manifest: a
// plugin's Node storage source under the ID of its selector.
func knowledgeAutomatic(stored KnowledgeStored) bool {
	source := ReadKnowledgeSource(stored.Collection.Definition.Source)
	if !stored.Automatic || stored.Collection.Owner.Kind != "plugin" || source.Kind != "storage" || source.Scope != protocol.PluginStorageScopeNode {
		return false
	}
	name := knowledgeOwner(stored.Collection.Owner)
	return semanticIdentifier(name, source.Prefix, false) == stored.Collection.ID || semanticIdentifier(name, source.Prefix, true) == stored.Collection.ID
}

func packageReference(info protocol.PluginInfo) protocol.PluginReference {
	return protocol.PluginReference{Name: info.Summary.Name, Digest: info.Summary.Digest, SettingsRevision: info.Summary.SettingsRevision}
}

// ensureSemantic registers the declared collections of a package that have
// an indexed value pointing at its text. It returns the registered ones.
func ensureSemantic(ctx context.Context, db *sql.Conn, reference protocol.PluginReference) ([]KnowledgeStored, bool, error) {
	declarations, err := semanticDeclarations(ctx, db, reference)
	if err != nil {
		return nil, false, err
	}
	entries, err := loadKnowledge(ctx, db)
	if err != nil {
		return nil, false, err
	}
	var selected []KnowledgeStored
	changed := false
	for _, declaration := range declarations {
		if err := semanticCancelled(ctx); err != nil {
			return nil, false, err
		}
		selector, exact := semanticSelector(declaration)
		var exists bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM plugin_values WHERE name=?1 AND value IS NOT NULL AND index_data IS NOT NULL AND (?2=1 AND key=?3 OR ?2=0 AND substr(key,1,length(?3))=?3) AND json_type(index_data,'$.source')='object')",
			reference.Name, exact, selector).Scan(&exists); err != nil {
			return nil, false, err
		}
		if !exists {
			continue
		}
		id := semanticIdentifier(reference.Name, selector, exact)
		if position := slices.IndexFunc(entries, func(stored KnowledgeStored) bool { return stored.Collection.ID == id }); position >= 0 {
			previous := &entries[position]
			if !knowledgeAutomatic(*previous) {
				return nil, false, fault(protocol.ErrorCodeConflict, "semantic registration identifier is already owned")
			}
			latest, err := ReadPlugin(ctx, db, reference.Name)
			if err != nil {
				return nil, false, err
			}
			// An old admitted turn retains access without replacing a newer package capture.
			if *previous.Capture.Package != reference && latest != nil && packageReference(*latest) == reference {
				previous.Capture.Package = &reference
				if err := advanceKnowledge(previous); err != nil {
					return nil, false, err
				}
				if err := persistKnowledge(ctx, db, *previous); err != nil {
					return nil, false, err
				}
				changed = true
			}
			selected = append(selected, *previous)
			continue
		}
		collection := protocol.DefaultKnowledgeCollection()
		collection.ID, collection.Revision = id, 1
		collection.Owner = protocol.KnowledgeOwner{Kind: "plugin", Data: protocol.KnowledgeOwnerPluginPayload{Name: reference.Name}}
		collection.Definition = protocol.KnowledgeDefinition{
			Name:   reference.Name + " storage",
			Source: protocol.KnowledgeSource{Kind: "storage", Data: protocol.KnowledgeSourceStoragePayload{Scope: protocol.PluginStorageScopeNode, Prefix: selector}},
			Globs:  []string{}, Tags: []string{}, SharedWith: []string{},
		}
		collection.Progress.Generation = 1
		entry := KnowledgeStored{Collection: collection, Capture: KnowledgeCapture{Package: &reference}, Automatic: true}
		if err := persistKnowledge(ctx, db, entry); err != nil {
			return nil, false, err
		}
		selected, entries, changed = append(selected, entry), append(entries, entry), true
	}
	return selected, changed, nil
}

// RegisterKnowledgeSemantic registers the semantic collections of every
// enabled package. It runs when the Node starts and after a package changes.
func RegisterKnowledgeSemantic(ctx context.Context, db *sql.Conn) (bool, error) {
	infos, err := ReadPluginInfos(ctx, db)
	if err != nil {
		return false, err
	}
	changed := false
	for _, info := range infos {
		if err := semanticCancelled(ctx); err != nil {
			return false, err
		}
		if !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" {
			continue
		}
		_, registered, err := ensureSemantic(ctx, db, packageReference(info))
		if err != nil {
			return false, err
		}
		changed = changed || registered
	}
	return changed, nil
}

// KnowledgeStorageChanged follows a committed storage write of a plugin: it
// registers a collection that gained its first source and restarts the
// indexing of the package's automatic collections. It runs in the
// transaction of the write.
func KnowledgeStorageChanged(ctx context.Context, db *sql.Conn, request protocol.Request) (bool, error) {
	if request.Plugin == nil {
		return false, nil
	}
	_, changed, err := ensureSemantic(ctx, db, request.Plugin.Package)
	if err != nil {
		return false, err
	}
	entries, err := loadKnowledge(ctx, db)
	if err != nil {
		return false, err
	}
	for _, stored := range entries {
		if err := semanticCancelled(ctx); err != nil {
			return false, err
		}
		if !knowledgeAutomatic(stored) || stored.Capture.Package.Name != request.Plugin.Package.Name || stored.Collection.Progress.Status == protocol.KnowledgeStatusPending {
			continue
		}
		if err := advanceKnowledge(&stored); err != nil {
			return false, err
		}
		if err := persistKnowledge(ctx, db, stored); err != nil {
			return false, err
		}
		changed = true
	}
	return changed, nil
}

// semanticDeclaration resolves the declaration an automatic collection was
// registered for in its captured package.
func semanticDeclaration(ctx context.Context, db *sql.Conn, stored KnowledgeStored) (protocol.PluginStorageCollection, error) {
	if !knowledgeAutomatic(stored) {
		return protocol.PluginStorageCollection{}, invalid("automatic storage collection expected")
	}
	declarations, err := semanticDeclarations(ctx, db, *stored.Capture.Package)
	if err != nil {
		return protocol.PluginStorageCollection{}, err
	}
	for _, declaration := range declarations {
		selector, exact := semanticSelector(declaration)
		if semanticIdentifier(stored.Capture.Package.Name, selector, exact) == stored.Collection.ID {
			return declaration, nil
		}
	}
	return protocol.PluginStorageCollection{}, knowledgeDenied("captured package no longer declares this semantic collection")
}

// semanticRecord captures one indexed value whose index points at raw text
// inside the package's semantic collections.
func semanticRecord(ctx context.Context, db *sql.Conn, reference protocol.PluginReference, declarations []protocol.PluginStorageCollection, key string) (*KnowledgeStorageRecord, error) {
	if err := pluginKey(key, false); err != nil {
		return nil, err
	}
	if !slices.ContainsFunc(declarations, func(declaration protocol.PluginStorageCollection) bool { return semanticMatches(declaration, key) }) {
		return nil, nil
	}
	var revision int64
	var body, encoded []byte
	err := db.QueryRowContext(ctx, "SELECT revision,value,index_data FROM plugin_values WHERE name=?1 AND key=?2", reference.Name, key).Scan(&revision, &body, &encoded)
	if errors.Is(err, sql.ErrNoRows) || err == nil && (body == nil || encoded == nil) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	if revision <= 0 {
		return nil, invalid("invalid stored plugin value")
	}
	if len(encoded) > maxPluginIndexBytes {
		return nil, invalid("semantic index exceeds the size limit")
	}
	var index protocol.PluginStorageIndex
	if err := json.Unmarshal(encoded, &index); err != nil {
		return nil, err
	}
	if err := plugins.ValidateStorageIndex(index); err != nil {
		return nil, err
	}
	if index.Source == nil {
		return nil, nil
	}
	original, err := readPluginValue(ctx, db, reference.Name, index.Source.Key)
	if err != nil || !original.Present {
		return nil, err
	}
	text, found := pointedText(original.Value, index.Source.Pointer)
	if !found {
		return nil, invalid("semantic source must point to raw text")
	}
	if len(text) > plugins.MaxValueBytes || strings.ContainsRune(text, 0) {
		return nil, invalid("semantic source text is invalid")
	}
	return &KnowledgeStorageRecord{
		Key: key, Revision: uint64(revision), Hash: knowledgeRaw{body: body, index: encoded}.hash(),
		Title: index.Fields[0], Body: text, Source: *index.Source,
		SourceRevision: original.Revision, SourceHash: blake3.Sum256([]byte(text)), Tags: index.Tags,
	}, nil
}

// semanticSources lists the next keys of an automatic collection and the
// capture of each one.
func semanticSources(ctx context.Context, db *sql.Conn, stored KnowledgeStored, after *string, limit int) ([]string, func(string) (*KnowledgeStorageRecord, error), error) {
	declaration, err := semanticDeclaration(ctx, db, stored)
	if err != nil {
		return nil, nil, err
	}
	reference := *stored.Capture.Package
	declarations, err := semanticDeclarations(ctx, db, reference)
	if err != nil {
		return nil, nil, err
	}
	selector, exact := semanticSelector(declaration)
	keys, err := knowledgeKeys(ctx, db, "SELECT key FROM plugin_values WHERE name=?1 AND value IS NOT NULL AND index_data IS NOT NULL AND (?2=1 AND key=?3 OR ?2=0 AND substr(key,1,length(?3))=?3) AND (?4 IS NULL OR key>?4) ORDER BY key LIMIT ?5",
		reference.Name, exact, selector, after, limit+1)
	return keys, func(key string) (*KnowledgeStorageRecord, error) {
		return semanticRecord(ctx, db, reference, declarations, key)
	}, err
}

// semanticSource captures one key of an automatic collection, or nil when
// the collection no longer covers it.
func semanticSource(ctx context.Context, db *sql.Conn, stored KnowledgeStored, key string) (*KnowledgeStorageRecord, error) {
	declaration, err := semanticDeclaration(ctx, db, stored)
	if err != nil {
		return nil, err
	}
	if !semanticMatches(declaration, key) {
		return nil, nil
	}
	reference := *stored.Capture.Package
	declarations, err := semanticDeclarations(ctx, db, reference)
	if err != nil {
		return nil, err
	}
	return semanticRecord(ctx, db, reference, declarations, key)
}

func semanticQuery(request protocol.Request) (*protocol.PluginStorageSearch, error) {
	query, found := request.Command.Data.(*protocol.PluginStorageSearch)
	if !found {
		return nil, invalid("plugin storage search expected")
	}
	return query, nil
}

// PrepareSemanticSearch captures the plan of one plugin storage search. A
// keyword search, or one without text, selects no collection.
func PrepareSemanticSearch(ctx context.Context, db *sql.Conn, request protocol.Request) (SemanticPlan, error) {
	var plan SemanticPlan
	if err := semanticCancelled(ctx); err != nil {
		return plan, err
	}
	if request.Plugin == nil {
		return plan, knowledgeDenied("plugin provenance is required")
	}
	if _, err := knowledgePackage(ctx, db, *request.Plugin); err != nil {
		return plan, err
	}
	query, err := semanticQuery(request)
	if err != nil {
		return plan, err
	}
	reference := request.Plugin.Package
	plan.Package, plan.Eligible = reference, map[string]bool{}
	var declarations []protocol.PluginStorageCollection
	if query.Mode != protocol.KnowledgeModeKeyword && query.Query != nil && strings.TrimSpace(*query.Query) != "" {
		if declarations, err = semanticDeclarations(ctx, db, reference); err != nil {
			return plan, err
		}
		entries, err := loadKnowledge(ctx, db)
		if err != nil {
			return plan, err
		}
		registered := declarations[:0:0]
		for _, declaration := range declarations {
			if err := semanticCancelled(ctx); err != nil {
				return plan, err
			}
			selector, exact := semanticSelector(declaration)
			id := semanticIdentifier(reference.Name, selector, exact)
			position := slices.IndexFunc(entries, func(stored KnowledgeStored) bool { return stored.Collection.ID == id })
			if position < 0 {
				continue
			}
			if !knowledgeAutomatic(entries[position]) {
				return plan, fault(protocol.ErrorCodeConflict, "semantic registration identifier is already owned")
			}
			plan.Collections, registered = append(plan.Collections, entries[position]), append(registered, declaration)
		}
		declarations = registered
	}
	if plan.Keyword, err = rankedPluginValues(ctx, db, reference.Name, *query); err != nil {
		return plan, err
	}
	if len(plan.Collections) != 0 {
		keys, err := knowledgeKeys(ctx, db, "SELECT v.key FROM plugin_values v WHERE v.name=?1 AND v.value IS NOT NULL AND v.index_data IS NOT NULL AND json_type(v.index_data,'$.source')='object' AND EXISTS(SELECT 1 FROM plugin_values original WHERE original.name=v.name AND original.key=json_extract(v.index_data,'$.source.key') AND original.value IS NOT NULL) ORDER BY v.key LIMIT ?2",
			reference.Name, maxPluginKeys)
		if err != nil {
			return plan, err
		}
		for _, key := range keys {
			if slices.ContainsFunc(declarations, func(declaration protocol.PluginStorageCollection) bool { return semanticMatches(declaration, key) }) {
				plan.Eligible[key] = true
			}
		}
	}
	settings, err := ReadLocalModelSettings(ctx, db)
	if err != nil {
		return plan, err
	}
	plan.Embedding = localmodels.Choice(&settings, protocol.ModelsKindEmbedding)
	return plan, nil
}

func sameModelChoice(left, right protocol.ModelsLocalChoice) bool {
	return left.Enabled == right.Enabled && (left.Model == nil) == (right.Model == nil) && (left.Model == nil || *left.Model == *right.Model)
}

// MaterializeSemanticSearch reads the selected values of a plan. A result
// whose source, tags, grants or embedding choice moved since the plan falls
// back to the keyword page and reports it stale.
func MaterializeSemanticSearch(ctx context.Context, db *sql.Conn, request protocol.Request, plan SemanticPlan, selection SemanticSelection) (page protocol.PluginStorageSearchPage, stale bool, err error) {
	page = protocol.DefaultPluginStorageSearchPage()
	if err := semanticCancelled(ctx); err != nil {
		return page, false, err
	}
	if request.Plugin == nil {
		return page, false, knowledgeDenied("plugin provenance is required")
	}
	if _, err := knowledgePackage(ctx, db, *request.Plugin); err != nil {
		return page, false, err
	}
	reference := request.Plugin.Package
	if reference != plan.Package {
		return page, false, knowledgeDenied("semantic plan belongs to another capture")
	}
	query, err := semanticQuery(request)
	if err != nil {
		return page, false, err
	}
	fallback := func() (protocol.PluginStorageSearchPage, bool, error) {
		page, err := SearchPluginValues(ctx, db, reference, *query)
		page.Mode, page.Indexing = protocol.KnowledgeModeKeyword, true
		return page, true, err
	}
	if selection.Mode != protocol.KnowledgeModeKeyword {
		settings, err := ReadLocalModelSettings(ctx, db)
		if err != nil {
			return page, false, err
		}
		if !sameModelChoice(localmodels.Choice(&settings, protocol.ModelsKindEmbedding), plan.Embedding) {
			return fallback()
		}
	}
	keyword := map[string]uint64{}
	for _, rank := range plan.Keyword {
		keyword[rank.Key] = rank.Revision
	}
	stamps := map[string]SemanticStamp{}
	for _, stamp := range selection.Stamps {
		stamps[stamp.Key] = stamp
	}
	page.NowMs, page.Mode, page.Indexing = uint64(time.Now().UnixMilli()), selection.Mode, selection.Indexing
	limit := int(min(query.Limit, 100))
	next := func() {
		value := uint16(min(uint32(query.Offset)+uint32(len(page.Entries)), math.MaxUint16))
		page.Next = &value
	}
	for position := int(query.Offset); position < len(selection.Keys); position++ {
		key := selection.Keys[position]
		if err := semanticCancelled(ctx); err != nil {
			return page, false, err
		}
		if len(page.Entries) == limit {
			next()
			break
		}
		var semantic *uint64
		if stamp, found := stamps[key]; found {
			collection := slices.IndexFunc(plan.Collections, func(stored KnowledgeStored) bool { return stored.Collection.ID == stamp.Collection })
			if collection < 0 {
				return page, false, knowledgeDenied("semantic result is outside the captured collections")
			}
			record, err := ReadKnowledgeStorageRecord(ctx, db, plan.Collections[collection], key)
			if err != nil {
				var failure *protocol.Fault
				if errors.As(err, &failure) && (failure.Code == protocol.ErrorCodeRevisionConflict || failure.Code == protocol.ErrorCodeInvalidRequest) {
					return fallback()
				}
				return page, false, err
			}
			if record == nil || record.Identity() != stamp.Revision {
				return fallback()
			}
			if slices.ContainsFunc(query.All, func(tag string) bool { return !slices.Contains(record.Tags, tag) }) ||
				len(query.Any) != 0 && !slices.ContainsFunc(query.Any, func(tag string) bool { return slices.Contains(record.Tags, tag) }) {
				return fallback()
			}
			semantic = &record.Revision
		} else if _, ranked := keyword[key]; !ranked {
			return page, false, knowledgeDenied("semantic result has no captured ranking")
		}
		entry, err := ReadPluginValue(ctx, db, reference, key)
		if err != nil {
			return page, false, err
		}
		revision, ranked := keyword[key]
		if !entry.Present || semantic != nil && *semantic != entry.Revision || ranked && revision != entry.Revision {
			return fallback()
		}
		page.Entries = append(page.Entries, entry)
		body, err := json.Marshal(page)
		if err != nil {
			return page, false, err
		}
		if len(body)+len(page.Entries)*2+8 > maxPluginSearch {
			page.Entries = page.Entries[:len(page.Entries)-1]
			next()
			break
		}
	}
	return page, false, nil
}
