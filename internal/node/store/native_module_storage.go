package store

import (
	"encoding/json"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	// NativeScheduledTasksNamespace is the personal storage and dispatch
	// namespace shared by the native scheduled-tasks module and earlier
	// scheduled-tasks plugin versions.
	NativeScheduledTasksNamespace = "scheduled-tasks"
	nativeScheduledTasksDigest    = "0000000000000000000000000000000000000000000000000000000000000002"

	// NativeRemindersNamespace is the personal storage and dispatch namespace
	// shared by the native reminders module and earlier reminders plugin
	// versions.
	NativeRemindersNamespace = "reminders"
	nativeRemindersDigest    = "0000000000000000000000000000000000000000000000000000000000000003"
)

// NativeScheduledTasksReference identifies the built-in native scheduled-tasks
// storage and dispatch declaration.
func NativeScheduledTasksReference() protocol.PluginReference {
	return protocol.PluginReference{
		Name:   NativeScheduledTasksNamespace,
		Digest: nativeScheduledTasksDigest,
	}
}

// NativeRemindersReference identifies the built-in native reminders storage and
// dispatch declaration.
func NativeRemindersReference() protocol.PluginReference {
	return protocol.PluginReference{
		Name:   NativeRemindersNamespace,
		Digest: nativeRemindersDigest,
	}
}

// NativeStorageReference returns the built-in synthetic package reference for a
// native storage namespace, if one is defined.
func NativeStorageReference(namespace string) (protocol.PluginReference, bool) {
	switch namespace {
	case NativeMemoryNamespace:
		return NativeMemoryReference(), true
	case NativeScheduledTasksNamespace:
		return NativeScheduledTasksReference(), true
	case NativeRemindersNamespace:
		return NativeRemindersReference(), true
	default:
		return protocol.PluginReference{}, false
	}
}

// IsNativeStorageReference reports whether ref is one of the built-in synthetic
// native storage references.
func IsNativeStorageReference(ref protocol.PluginReference) bool {
	expected, ok := NativeStorageReference(ref.Name)
	return ok && ref == expected
}

// NativeStorageInfo returns the built-in storage and dispatch declaration for
// one native module namespace.
func NativeStorageInfo(namespace string) (protocol.PluginInfo, bool) {
	switch namespace {
	case NativeMemoryNamespace:
		return NativeMemoryStorageInfo(), true
	case NativeScheduledTasksNamespace:
		return NativeScheduledTasksStorageInfo(), true
	case NativeRemindersNamespace:
		return NativeRemindersStorageInfo(), true
	default:
		return protocol.PluginInfo{}, false
	}
}

// NativeStorageInfoForReference returns the built-in declaration matching ref
// when ref is a synthetic native storage reference.
func NativeStorageInfoForReference(ref protocol.PluginReference) (protocol.PluginInfo, bool) {
	if !IsNativeStorageReference(ref) {
		return protocol.PluginInfo{}, false
	}
	return NativeStorageInfo(ref.Name)
}

// NativeScheduledTasksStorageInfo preserves the exact "task/" personal storage
// schema and dispatch/session-start capabilities previously declared by
// plugins/scheduled-tasks/plugin.json.
var NativeScheduledTasksStorageInfo = sync.OnceValue(func() protocol.PluginInfo {
	prefixTask := "task/"
	version := "1.0.0"
	info := protocol.DefaultPluginInfo()
	info.Summary = protocol.DefaultPluginSummary()
	info.Summary.Name = NativeScheduledTasksNamespace
	info.Summary.Version = &version
	info.Summary.Digest = nativeScheduledTasksDigest
	info.Summary.Enabled = true
	info.Summary.Compatibility = protocol.PluginCompatibility{Kind: "compatible"}
	info.Extension = &protocol.PluginExtension{
		ApiVersion: "v1",
		Actions: []protocol.PluginAction{
			protocol.PluginActionReadStorage,
			protocol.PluginActionWriteStorage,
			protocol.PluginActionDispatch,
			protocol.PluginActionStartSessions,
			protocol.PluginActionReadProjects,
			protocol.PluginActionReadModels,
			protocol.PluginActionReadTeams,
			protocol.PluginActionReadConversation,
		},
		Storage: &protocol.PluginStorageDeclaration{
			Collections: []protocol.PluginStorageCollection{
				{
					Scope:  protocol.PluginStorageScopeNode,
					Prefix: &prefixTask,
					Schema: json.RawMessage(`{
						"type": "object",
						"required": ["id", "name", "prompt", "config", "timing", "queue"],
						"properties": {
							"id": {"type": "string", "pattern": "^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$"},
							"name": {"type": "string", "minLength": 1, "maxLength": 256},
							"prompt": {"type": "string", "minLength": 1, "maxLength": 32768},
							"project": {"type": ["string", "null"], "default": null},
							"worktree": {"type": ["string", "null"], "default": null},
							"config": {"type": "object"},
							"enabled": {"type": "boolean", "default": true},
							"queue": {"type": "string", "pattern": "^[a-zA-Z0-9._-]{1,128}$"},
							"timing": {
								"oneOf": [
									{
										"type": "object",
										"required": ["kind", "data"],
										"properties": {
											"kind": {"const": "once"},
											"data": {
												"type": "object",
												"required": ["at_ms"],
												"properties": {
													"at_ms": {"type": "integer", "minimum": 0}
												}
											}
										}
									},
									{
										"type": "object",
										"required": ["kind", "data"],
										"properties": {
											"kind": {"const": "every"},
											"data": {
												"type": "object",
												"required": ["anchor_ms", "interval_ms"],
												"properties": {
													"anchor_ms": {"type": "integer", "minimum": 0},
													"interval_ms": {"type": "integer", "minimum": 1}
												}
											}
										}
									},
									{
										"type": "object",
										"required": ["kind", "data"],
										"properties": {
											"kind": {"const": "weekly"},
											"data": {
												"type": "object",
												"required": ["timezone", "weekdays", "hour", "minute"],
												"properties": {
													"timezone": {"type": "string", "minLength": 1},
													"weekdays": {
														"type": "array",
														"minItems": 1,
														"maxItems": 7,
														"uniqueItems": true,
														"items": {"type": "integer", "minimum": 1, "maximum": 7}
													},
													"hour": {"type": "integer", "minimum": 0, "maximum": 23},
													"minute": {"type": "integer", "minimum": 0, "maximum": 59}
												}
											}
										}
									}
								]
							},
							"execution": {"type": "string", "enum": ["model", "team"], "default": "team"},
							"team": {
								"type": ["object", "null"],
								"required": ["id"],
								"properties": {
									"id": {"type": "string", "pattern": "^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$"},
									"revision": {"type": ["string", "null"], "pattern": "^[1-9][0-9]*$", "default": null}
								},
								"default": null
							},
							"models": {"type": "object", "default": {}},
							"execution_node": {
								"type": ["array", "null"],
								"minItems": 32,
								"maxItems": 32,
								"items": {"type": "integer", "minimum": 0, "maximum": 255}
							}
						}
					}`),
				},
			},
		},
	}
	return info
})

// NativeRemindersStorageInfo preserves the exact "reminder/" personal storage
// schema and dispatch/notification capabilities previously declared by
// plugins/reminders/plugin.json.
var NativeRemindersStorageInfo = sync.OnceValue(func() protocol.PluginInfo {
	prefixReminder := "reminder/"
	version := "1.0.0"
	info := protocol.DefaultPluginInfo()
	info.Summary = protocol.DefaultPluginSummary()
	info.Summary.Name = NativeRemindersNamespace
	info.Summary.Version = &version
	info.Summary.Digest = nativeRemindersDigest
	info.Summary.Enabled = true
	info.Summary.Compatibility = protocol.PluginCompatibility{Kind: "compatible"}
	info.Extension = &protocol.PluginExtension{
		ApiVersion: "v1",
		Actions: []protocol.PluginAction{
			protocol.PluginActionReadStorage,
			protocol.PluginActionWriteStorage,
			protocol.PluginActionDispatch,
			protocol.PluginActionNotify,
			protocol.PluginActionReadProjects,
		},
		Storage: &protocol.PluginStorageDeclaration{
			Collections: []protocol.PluginStorageCollection{
				{
					Scope:  protocol.PluginStorageScopeNode,
					Prefix: &prefixReminder,
					Schema: json.RawMessage(`{
						"type": "object",
						"required": ["id", "project", "title"],
						"properties": {
							"id": {"type": "string", "pattern": "^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$"},
							"project": {"type": ["string", "null"], "default": null, "pattern": "^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$"},
							"title": {"type": "string", "minLength": 1, "maxLength": 512},
							"message": {"type": "string", "default": "", "maxLength": 4096},
							"completed": {"type": "boolean", "default": false},
							"due_ms": {"type": ["integer", "null"], "default": null, "minimum": 0},
							"notified_ms": {"type": ["integer", "null"], "default": null, "minimum": 0},
							"execution_node": {
								"type": ["array", "null"],
								"minItems": 32,
								"maxItems": 32,
								"items": {"type": "integer", "minimum": 0, "maximum": 255}
							}
						}
					}`),
				},
			},
		},
	}
	return info
})
