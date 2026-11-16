package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"math"
	"slices"
	"time"

	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// PersonalKnowledgeController identifies only a replica's personal namespace.
// Project and conversation searches stay on their container's own Node.
func PersonalKnowledgeController(ctx context.Context, db *sql.Conn, request protocol.Request) (*protocol.NodeID, error) {
	query, ok := request.Command.Data.(*protocol.PluginStorageSearch)
	if !ok || query.Scope != "" && query.Scope != protocol.PluginStorageScopeNode {
		return nil, nil
	}
	user, cached, err := configurationCacheUser(ctx, db)
	if err != nil || !cached {
		return nil, err
	}
	return user.Controller, nil
}

// MaterializePersonalKnowledge merges remote source stamps with the replica's
// current keyword ranking, then reads values and view revisions locally.
func MaterializePersonalKnowledge(ctx context.Context, db *sql.Conn, request protocol.Request, candidates protocol.KnowledgeCandidates) (protocol.PluginStorageSearchPage, error) {
	page := protocol.DefaultPluginStorageSearchPage()
	query, err := semanticQuery(request)
	if err != nil || request.Plugin == nil {
		return page, invalid("personal storage search expected")
	}
	ctx, err = PluginStorageRequestContext(ctx, db, request)
	if err != nil {
		return page, err
	}
	info, err := knowledgePackage(ctx, db, *request.Plugin)
	if err != nil {
		return page, err
	}
	if !knowledgeDeclares(info, protocol.PluginActionReadStorage) {
		return page, knowledgeDenied("plugin does not declare storage access")
	}
	declarations, err := semanticDeclarations(ctx, db, request.Plugin.Package)
	if err != nil {
		return page, err
	}
	keyword, err := rankedPluginValues(ctx, db, request.Plugin.Package.Name, *query)
	if err != nil {
		return page, err
	}
	if len(candidates.Candidates) > 100 {
		return page, invalid("personal knowledge candidate limit exceeded")
	}
	semantic := []string{}
	page.Indexing = candidates.Indexing
	for _, candidate := range candidates.Candidates {
		if !knowledge.Hash(candidate.Revision) || !knowledgeStorageKey(candidate.Key) {
			return page, invalid("invalid personal knowledge candidate")
		}
		record, err := semanticRecord(ctx, db, request.Plugin.Package, declarations, candidate.Key)
		if err != nil {
			return page, err
		}
		if record == nil || record.Identity() != candidate.Revision {
			page.Indexing = true
			continue
		}
		if slices.ContainsFunc(query.All, func(tag string) bool { return !slices.Contains(record.Tags, tag) }) || len(query.Any) != 0 && !slices.ContainsFunc(query.Any, func(tag string) bool { return slices.Contains(record.Tags, tag) }) {
			continue
		}
		if !slices.Contains(semantic, candidate.Key) {
			semantic = append(semantic, candidate.Key)
		}
	}
	keys := []string{}
	for _, rank := range keyword {
		eligible := slices.ContainsFunc(declarations, func(declaration protocol.PluginStorageCollection) bool { return semanticMatches(declaration, rank.Key) })
		if query.Mode != protocol.KnowledgeModeSemantic || page.Indexing || !candidates.Available || len(semantic) == 0 || !eligible {
			keys = append(keys, rank.Key)
		}
	}
	switch {
	case len(semantic) == 0:
		page.Mode = protocol.KnowledgeModeKeyword
	case len(keys) == 0:
		page.Mode, keys = protocol.KnowledgeModeSemantic, semantic
	default:
		page.Mode = protocol.KnowledgeModeHybrid
		fused := knowledge.Fuse(keys, semantic)
		keys = make([]string, len(fused))
		for index, rank := range fused {
			keys[index] = rank.Key
		}
	}
	page.NowMs = uint64(time.Now().UnixMilli())
	for position := int(query.Offset); position < len(keys); position++ {
		if len(page.Entries) == int(min(query.Limit, 100)) {
			next := uint16(min(uint32(position), math.MaxUint16))
			page.Next = &next
			break
		}
		entry, err := ReadPluginValue(ctx, db, request.Plugin.Package, keys[position])
		if err != nil {
			return page, err
		}
		if !entry.Present {
			continue
		}
		page.Entries = append(page.Entries, entry)
		body, err := json.Marshal(page)
		if err != nil {
			return page, err
		}
		if len(body)+len(page.Entries)*2+8 > maxPluginSearch {
			page.Entries = page.Entries[:len(page.Entries)-1]
			next := uint16(min(uint32(position), math.MaxUint16))
			page.Next = &next
			break
		}
	}
	return page, nil
}
