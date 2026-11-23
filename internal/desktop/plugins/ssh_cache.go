package plugins

import (
	"bytes"
	"encoding/json"
	"sync"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	sshDirectoryCacheMaxEntries = 128
	sshDirectoryCacheMaxBytes   = 4 * 1024 * 1024
	sshDirectoryCacheTTL        = 5 * time.Minute
)

type sshDirectoryKey struct {
	node     protocol.NodeID
	profile  string
	revision uint64
	path     string
	after    string
}

type sshDirectoryEntry struct {
	key      sshDirectoryKey
	body     json.RawMessage
	storedAt time.Time
	used     uint64
}

type sshDirectoryFlight struct {
	done chan struct{}
	body json.RawMessage
	err  error
}

// SshDirectoryCache is a bounded, owner-scoped cache of SSH directory listings
// shared across remounts of a window's SSH plugin view. Entries are evicted by
// revision, TTL, count, total byte budget, explicit refresh, file mutations,
// disconnect, and owner close.
type SshDirectoryCache struct {
	clock    clockwork.Clock
	ttl      time.Duration
	mu       sync.Mutex
	used     uint64
	bytes    int
	entries  map[sshDirectoryKey]*sshDirectoryEntry
	inflight map[sshDirectoryKey]*sshDirectoryFlight
}

func NewSshDirectoryCache(clock clockwork.Clock) *SshDirectoryCache {
	if clock == nil {
		clock = clockwork.NewRealClock()
	}
	return &SshDirectoryCache{
		clock:    clock,
		ttl:      sshDirectoryCacheTTL,
		entries:  map[sshDirectoryKey]*sshDirectoryEntry{},
		inflight: map[sshDirectoryKey]*sshDirectoryFlight{},
	}
}

func (cache *SshDirectoryCache) get(key sshDirectoryKey) (json.RawMessage, bool) {
	if cache == nil {
		return nil, false
	}
	now := cache.clock.Now()
	cache.mu.Lock()
	defer cache.mu.Unlock()
	entry, ok := cache.entries[key]
	if !ok {
		return nil, false
	}
	if cache.ttl > 0 && now.Sub(entry.storedAt) >= cache.ttl {
		cache.deleteLocked(key, entry)
		return nil, false
	}
	cache.used++
	entry.used = cache.used
	return append(json.RawMessage(nil), entry.body...), true
}

func (cache *SshDirectoryCache) put(key sshDirectoryKey, body json.RawMessage, resolvedPath string) {
	if cache == nil || len(body) == 0 || len(body) > sshDirectoryCacheMaxBytes {
		return
	}
	now := cache.clock.Now()
	cache.mu.Lock()
	defer cache.mu.Unlock()
	// Evict any listings kept under an older profile revision.
	for existingKey, existing := range cache.entries {
		if existingKey.node == key.node && existingKey.profile == key.profile && existingKey.revision != key.revision {
			cache.deleteLocked(existingKey, existing)
		}
	}
	cache.storeLocked(key, body, now)
	if key.path == "." && key.after == "" && resolvedPath != "" && resolvedPath != "." {
		resolvedKey := key
		resolvedKey.path = resolvedPath
		cache.storeLocked(resolvedKey, body, now)
	}
	cache.evictLocked(now)
}

func (cache *SshDirectoryCache) storeLocked(key sshDirectoryKey, body json.RawMessage, now time.Time) {
	if existing, ok := cache.entries[key]; ok {
		cache.deleteLocked(key, existing)
	}
	cache.used++
	copied := append(json.RawMessage(nil), body...)
	cache.entries[key] = &sshDirectoryEntry{
		key:      key,
		body:     copied,
		storedAt: now,
		used:     cache.used,
	}
	cache.bytes += len(copied)
}

func (cache *SshDirectoryCache) deleteLocked(key sshDirectoryKey, entry *sshDirectoryEntry) {
	delete(cache.entries, key)
	cache.bytes -= len(entry.body)
}

func (cache *SshDirectoryCache) evictLocked(now time.Time) {
	if cache.ttl > 0 {
		for key, entry := range cache.entries {
			if now.Sub(entry.storedAt) >= cache.ttl {
				cache.deleteLocked(key, entry)
			}
		}
	}
	for len(cache.entries) > sshDirectoryCacheMaxEntries || cache.bytes > sshDirectoryCacheMaxBytes {
		var oldestKey sshDirectoryKey
		var oldest *sshDirectoryEntry
		for key, entry := range cache.entries {
			if oldest == nil || entry.used < oldest.used {
				oldestKey, oldest = key, entry
			}
		}
		if oldest == nil {
			break
		}
		cache.deleteLocked(oldestKey, oldest)
	}
}

func (cache *SshDirectoryCache) InvalidateNode(node protocol.NodeID) {
	if cache == nil {
		return
	}
	cache.mu.Lock()
	defer cache.mu.Unlock()
	for key, entry := range cache.entries {
		if key.node == node {
			cache.deleteLocked(key, entry)
		}
	}
	for key := range cache.inflight {
		if key.node == node {
			delete(cache.inflight, key)
		}
	}
}

func (cache *SshDirectoryCache) InvalidateProfile(node protocol.NodeID, profile string) {
	if cache == nil {
		return
	}
	if profile == "" {
		cache.InvalidateNode(node)
		return
	}
	cache.mu.Lock()
	defer cache.mu.Unlock()
	for key, entry := range cache.entries {
		if key.node == node && key.profile == profile {
			cache.deleteLocked(key, entry)
		}
	}
	for key := range cache.inflight {
		if key.node == node && key.profile == profile {
			delete(cache.inflight, key)
		}
	}
}

func (cache *SshDirectoryCache) Clear() {
	if cache == nil {
		return
	}
	cache.mu.Lock()
	clear(cache.entries)
	clear(cache.inflight)
	cache.bytes = 0
	cache.mu.Unlock()
}

func (cache *SshDirectoryCache) Len() int {
	if cache == nil {
		return 0
	}
	cache.mu.Lock()
	defer cache.mu.Unlock()
	return len(cache.entries)
}

func parseSshBrowseKey(node protocol.NodeID, raw json.RawMessage) (sshDirectoryKey, bool) {
	var input struct {
		Profile          string          `json:"profile"`
		ExpectedRevision uint64          `json:"expected_revision"`
		Path             string          `json:"path"`
		After            json.RawMessage `json:"after"`
	}
	if json.Unmarshal(raw, &input) != nil || input.Profile == "" || input.Path == "" {
		return sshDirectoryKey{}, false
	}
	after := ""
	if trimmed := bytes.TrimSpace(input.After); len(trimmed) > 0 && !bytes.Equal(trimmed, []byte("null")) {
		var canonical bytes.Buffer
		if json.Compact(&canonical, trimmed) == nil {
			after = canonical.String()
		} else {
			after = string(trimmed)
		}
	}
	return sshDirectoryKey{
		node:     node,
		profile:  input.Profile,
		revision: input.ExpectedRevision,
		path:     input.Path,
		after:    after,
	}, true
}

func sshDirectoryResponsePath(body json.RawMessage) (string, bool) {
	var outcome struct {
		Kind string `json:"kind"`
		Path string `json:"path"`
	}
	if json.Unmarshal(body, &outcome) != nil || outcome.Kind != "directory" {
		return "", false
	}
	return outcome.Path, true
}

func sshMutationProfile(command protocol.Command) (string, bool) {
	switch command.Kind {
	case "modify_ssh_file", "finish_ssh_upload", "transfer_ssh", "save_ssh", "remove_ssh", "trust_ssh", "run_ssh":
	default:
		return "", false
	}
	body, err := json.Marshal(command.Data)
	if err != nil {
		return "", true
	}
	var payload struct {
		Profile json.RawMessage `json:"profile"`
	}
	if json.Unmarshal(body, &payload) != nil {
		return "", true
	}
	var id string
	if json.Unmarshal(payload.Profile, &id) == nil && id != "" {
		return id, true
	}
	var nested struct {
		ID string `json:"id"`
	}
	if json.Unmarshal(payload.Profile, &nested) == nil && nested.ID != "" {
		return nested.ID, true
	}
	return "", true
}

func sshRefreshEventTarget(name string, value any) (profile string, refresh bool) {
	event, ok := value.(map[string]any)
	if !ok {
		return "", false
	}
	switch name {
	case "control":
		if id, _ := event["id"].(string); id == "ssh-files-refresh" {
			return "", true
		}
	case "context-menu":
		return sshRefreshActionProfile(event["id"])
	case "tree":
		if tree, _ := event["tree"].(string); tree == "ssh-files" {
			if kind, _ := event["kind"].(string); kind == "menu" {
				return sshRefreshActionProfile(event["action"])
			}
		}
	}
	return "", false
}

func sshRefreshActionProfile(raw any) (string, bool) {
	text, ok := raw.(string)
	if !ok || text == "" || text[0] != '{' {
		return "", false
	}
	var action struct {
		Action  string `json:"action"`
		Profile string `json:"profile"`
	}
	if json.Unmarshal([]byte(text), &action) != nil || action.Action != "refresh" {
		return "", false
	}
	return action.Profile, true
}
