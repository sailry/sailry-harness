package store

import (
	"encoding/json"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	// NativeMemoryNamespace is the personal storage namespace shared by the
	// compile-time memory module and earlier memory plugin versions.
	NativeMemoryNamespace = "memory"
	nativeMemoryDigest    = "0000000000000000000000000000000000000000000000000000000000000001"
)

// NativeMemoryReference identifies the built-in native memory storage
// declaration so personal memory entries, FTS indexes and automatic semantic
// knowledge collections keep working after plugins/memory is retired.
func NativeMemoryReference() protocol.PluginReference {
	return protocol.PluginReference{
		Name:   NativeMemoryNamespace,
		Digest: nativeMemoryDigest,
	}
}

func isNativeMemoryReference(ref protocol.PluginReference) bool {
	return ref.Name == NativeMemoryNamespace && ref.Digest == nativeMemoryDigest
}

// NativeMemoryStorageInfo returns the built-in storage declaration for the
// native memory module, preserving the exact 4 schemas and semantic index opt-in
// previously declared by plugins/memory/plugin.json.
var NativeMemoryStorageInfo = sync.OnceValue(func() protocol.PluginInfo {
	keySettings := "memory/settings"
	keyCatalog := "memory/catalog"
	prefixEntry := "memory/entry/"
	prefixBody := "memory/body/"
	version := "1.0.0"
	info := protocol.DefaultPluginInfo()
	info.Summary = protocol.DefaultPluginSummary()
	info.Summary.Name = NativeMemoryNamespace
	info.Summary.Version = &version
	info.Summary.Digest = nativeMemoryDigest
	info.Summary.Enabled = true
	info.Summary.Compatibility = protocol.PluginCompatibility{Kind: "compatible"}
	info.Extension = &protocol.PluginExtension{
		ApiVersion: "v1",
		Actions: []protocol.PluginAction{
			protocol.PluginActionReadStorage,
			protocol.PluginActionWriteStorage,
			protocol.PluginActionReadProjects,
		},
		Storage: &protocol.PluginStorageDeclaration{
			Collections: []protocol.PluginStorageCollection{
				{
					Scope: protocol.PluginStorageScopeNode,
					Key:   &keySettings,
					Schema: json.RawMessage(`{
						"type": "object",
						"required": ["v", "settings"],
						"properties": {
							"v": {"const": 1},
							"settings": {
								"type": "object",
								"properties": {
									"enabled": {"type": "boolean", "default": true},
									"auto_write": {"type": "boolean", "default": true},
									"context_bytes": {"type": "integer", "minimum": 2048, "maximum": 65536, "default": 8192},
									"review_after_days": {"type": "integer", "minimum": 1, "maximum": 3650, "default": 90}
								}
							}
						}
					}`),
				},
				{
					Scope: protocol.PluginStorageScopeNode,
					Key:   &keyCatalog,
					Schema: json.RawMessage(`{
						"type": "object",
						"required": ["v", "pages", "active", "archived", "next"],
						"properties": {
							"v": {"const": 1},
							"pages": {
								"type": "object",
								"propertyNames": {"pattern": "^(0|[1-9][0-9]*)$"},
								"additionalProperties": {"type": "integer", "minimum": 1, "maximum": 2}
							},
							"active": {"type": "integer", "minimum": 0},
							"archived": {"type": "integer", "minimum": 0},
							"next": {"type": "integer", "minimum": 0}
						}
					}`),
				},
				{
					Scope:  protocol.PluginStorageScopeNode,
					Prefix: &prefixEntry,
					Index:  &protocol.PluginStorageDeclarationIndex{Semantic: true},
					Schema: json.RawMessage(`{
						"type": "object",
						"required": ["v", "summary", "hash", "body_page", "body_revision"],
						"properties": {
							"v": {"const": 1},
							"hash": {"type": "string", "pattern": "^[0-9a-f]{16}$"},
							"body_page": {"type": "string", "pattern": "^(0|[1-9][0-9]*)$"},
							"body_revision": {"type": "integer", "minimum": 1},
							"summary": {
								"type": "object",
								"required": ["id", "project", "title", "kind", "archived", "revision", "updated_at_ms"],
								"properties": {
									"id": {"type": "string", "pattern": "^[0-9a-fA-F]{8}(-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}$"},
									"project": {"type": ["string", "null"]},
									"title": {"type": "string", "minLength": 1},
									"kind": {"enum": ["user", "feedback", "project", "reference"]},
									"archived": {"type": "boolean"},
									"revision": {"type": "integer", "minimum": 0},
									"updated_at_ms": {"type": "integer", "minimum": 0}
								}
							}
						}
					}`),
				},
				{
					Scope:  protocol.PluginStorageScopeNode,
					Prefix: &prefixBody,
					Schema: json.RawMessage(`{
						"type": "object",
						"required": ["v", "bodies"],
						"properties": {
							"v": {"const": 1},
							"bodies": {
								"type": "object",
								"maxProperties": 2,
								"additionalProperties": {
									"type": "object",
									"required": ["body", "revision"],
									"properties": {
										"body": {"type": "string"},
										"revision": {"type": "integer", "minimum": 1}
									}
								}
							}
						}
					}`),
				},
			},
		},
	}
	return info
})
