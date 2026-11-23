package memorynode

import (
	"fmt"
	"regexp"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	pageLimit   = 2
	settingsKey = "memory/settings"
	headKey     = "memory/catalog"
	entryPrefix = "memory/entry/"
	maxSafeInt  = (1 << 53) - 1
)

var (
	uuidRE      = regexp.MustCompile(`^[0-9a-fA-F]{8}(-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}$`)
	pageIDRE    = regexp.MustCompile(`^(0|[1-9][0-9]*)$`)
	hash16RE    = regexp.MustCompile(`^[0-9a-f]{16}$`)
	whitespaceS = func(r rune) bool {
		return (r >= 0x0009 && r <= 0x000d) || r == 0x0020 || r == 0x0085 || r == 0x00a0 ||
			r == 0x1680 || (r >= 0x2000 && r <= 0x200a) || r == 0x2028 || r == 0x2029 ||
			r == 0x202f || r == 0x205f || r == 0x3000
	}
)

type memorySettings struct {
	Revision        uint64 `json:"revision"`
	Enabled         bool   `json:"enabled"`
	AutoWrite       bool   `json:"auto_write"`
	ContextBytes    int    `json:"context_bytes"`
	ReviewAfterDays int    `json:"review_after_days"`
}

func defaultMemorySettings() memorySettings {
	return memorySettings{
		Revision:        0,
		Enabled:         true,
		AutoWrite:       true,
		ContextBytes:    8192,
		ReviewAfterDays: 90,
	}
}

type memorySummary struct {
	ID          string  `json:"id"`
	Project     *string `json:"project"`
	Title       string  `json:"title"`
	Kind        string  `json:"kind"`
	Archived    bool    `json:"archived"`
	Revision    int64   `json:"revision"`
	UpdatedAtMs int64   `json:"updated_at_ms"`
}

type memoryEntry struct {
	Summary memorySummary `json:"summary"`
	Body    string        `json:"body"`
}

type storedEntry struct {
	V            int           `json:"v"`
	Summary      memorySummary `json:"summary"`
	Hash         string        `json:"hash"`
	BodyPage     string        `json:"body_page"`
	BodyRevision int64         `json:"body_revision"`
}

type storedBody struct {
	Body     string `json:"body"`
	Revision int64  `json:"revision"`
}

type storedBodyPage struct {
	V      int                   `json:"v"`
	Bodies map[string]storedBody `json:"bodies"`
}

type memoryCatalog struct {
	V        int            `json:"v"`
	Pages    map[string]int `json:"pages"`
	Active   int            `json:"active"`
	Archived int            `json:"archived"`
	Next     int64          `json:"next"`
}

func failFault(code protocol.ErrorCode, message string) error {
	if code == "" {
		code = protocol.ErrorCodeInvalidRequest
	}
	return &protocol.Fault{Code: code, Message: message}
}

func normalizedText(text string) string {
	parts := strings.FieldsFunc(text, whitespaceS)
	return strings.ToLower(strings.Join(parts, " "))
}

func normalizeID(value string) (string, error) {
	if !uuidRE.MatchString(value) {
		return "", failFault(protocol.ErrorCodeInvalidRequest, "invalid memory ID")
	}
	return strings.ToLower(value), nil
}

func validateSummary(s memorySummary) error {
	if normalizedText(s.Title) == "" {
		return failFault(protocol.ErrorCodeInvalidRequest, "invalid memory entry")
	}
	for _, r := range s.Title {
		if unicode.Is(unicode.Cc, r) {
			return failFault(protocol.ErrorCodeInvalidRequest, "invalid memory entry")
		}
	}
	switch s.Kind {
	case "user", "feedback", "project", "reference":
	default:
		return failFault(protocol.ErrorCodeInvalidRequest, "invalid memory entry")
	}
	if s.UpdatedAtMs < 0 || s.UpdatedAtMs > maxSafeInt || s.Revision < 0 || s.Revision > maxSafeInt {
		return failFault(protocol.ErrorCodeInvalidRequest, "invalid memory entry")
	}
	if _, err := normalizeID(s.ID); err != nil {
		return err
	}
	if s.Project != nil {
		if _, err := normalizeID(*s.Project); err != nil {
			return err
		}
	}
	return nil
}

func validateEntry(e memoryEntry) error {
	if err := validateSummary(e.Summary); err != nil {
		return err
	}
	if normalizedText(e.Body) == "" || strings.ContainsRune(e.Body, 0) {
		return failFault(protocol.ErrorCodeInvalidRequest, "invalid memory entry")
	}
	return nil
}

func nextEntry(e memoryEntry, nowMs int64) (memoryEntry, error) {
	if err := validateEntry(e); err != nil {
		return memoryEntry{}, err
	}
	if e.Summary.Revision >= maxSafeInt {
		return memoryEntry{}, failFault(protocol.ErrorCodeInvalidRequest, "memory revision exhausted")
	}
	id, err := normalizeID(e.Summary.ID)
	if err != nil {
		return memoryEntry{}, err
	}
	saved := e
	saved.Summary.ID = id
	if saved.Summary.Project != nil {
		p, err := normalizeID(*saved.Summary.Project)
		if err != nil {
			return memoryEntry{}, err
		}
		saved.Summary.Project = &p
	}
	saved.Summary.Revision++
	saved.Summary.UpdatedAtMs = nowMs
	return saved, nil
}

// equivalence matches the dual 32-bit FNV/Murmur hash in model.js verbatim so
// existing stored `equivalent:<hash>` index tags stay 100% compatible.
func equivalenceHash(body string) string {
	var first uint32 = 0x811c9dc5
	var second uint32 = 0x9e3779b9
	for _, r := range normalizedText(body) {
		point := uint32(r)
		first = (first ^ point) * 0x01000193
		second = (second ^ point) * 0x85ebca6b
	}
	return fmt.Sprintf("%08x%08x", first, second)
}

func compareSummaryOrder(a, b memorySummary) int {
	if a.UpdatedAtMs != b.UpdatedAtMs {
		if b.UpdatedAtMs > a.UpdatedAtMs {
			return 1
		}
		return -1
	}
	return strings.Compare(a.ID, b.ID)
}

func sameProject(a, b *string) bool {
	if a == nil && b == nil {
		return true
	}
	if a == nil || b == nil {
		return false
	}
	return *a == *b
}

func visibleInProject(s memorySummary, project *string) bool {
	return s.Project == nil || sameProject(s.Project, project)
}

func scopeTag(project *string) string {
	if project == nil {
		return "scope:global"
	}
	return "scope:" + *project
}

func entryKey(id string) string  { return entryPrefix + strings.ToLower(id) }
func bodyKey(page string) string { return "memory/body/" + page }

func validateCatalog(c *memoryCatalog) (memoryCatalog, error) {
	if c == nil {
		return memoryCatalog{V: 1, Pages: map[string]int{}, Active: 0, Archived: 0, Next: 0}, nil
	}
	if c.V != 1 || c.Pages == nil || c.Next < 0 || c.Active < 0 || c.Archived < 0 {
		return memoryCatalog{}, failFault(protocol.ErrorCodeInvalidRequest, "memory storage is incompatible")
	}
	total := 0
	pages := make(map[string]int, len(c.Pages))
	for k, count := range c.Pages {
		if !pageIDRE.MatchString(k) || count < 1 || count > pageLimit {
			return memoryCatalog{}, failFault(protocol.ErrorCodeInvalidRequest, "memory storage is incompatible")
		}
		pages[k] = count
		total += count
	}
	if total != c.Active+c.Archived {
		return memoryCatalog{}, failFault(protocol.ErrorCodeInvalidRequest, "memory storage is incompatible")
	}
	return memoryCatalog{V: 1, Pages: pages, Active: c.Active, Archived: c.Archived, Next: c.Next}, nil
}

type capacityDelta struct {
	before *memorySummary
	after  *memorySummary
}

func applyCapacity(head *memoryCatalog, changes []capacityDelta) error {
	active, archived := head.Active, head.Archived
	for _, ch := range changes {
		if ch.before != nil {
			if ch.before.Archived {
				archived--
			} else {
				active--
			}
		}
		if ch.after != nil {
			if ch.after.Archived {
				archived++
			} else {
				active++
			}
		}
	}
	if active < 0 || archived < 0 {
		return failFault(protocol.ErrorCodeInvalidRequest, "memory storage is incompatible")
	}
	head.Active, head.Archived = active, archived
	return nil
}
