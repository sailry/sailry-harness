package remindersnode

import (
	"bytes"
	"encoding/json"
	"regexp"
	"strconv"
	"strings"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	storageNamespace = store.NativeRemindersNamespace
	keyPrefix        = "reminder/"
	maxTitleBytes    = 512
	maxMessageBytes  = 4096
	maxPageItems     = 32
	maxScanKeys      = 128
	maxPageBytes     = 128 * 1024
)

var uuidPattern = regexp.MustCompile(`^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$`)

func fault(code protocol.ErrorCode, msg string) error {
	return &protocol.Fault{Code: code, Message: msg}
}

func newID() string {
	return uuid.Must(uuid.NewV7()).String()
}

func reminderKey(id string) (string, error) {
	if !uuidPattern.MatchString(id) {
		return "", fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder ID")
	}
	return keyPrefix + id, nil
}

func validateNodeID(id *protocol.NodeID) error {
	if id == nil {
		return nil
	}
	if *id == (protocol.NodeID{}) {
		return fault(protocol.ErrorCodeInvalidRequest, "Invalid execution Node")
	}
	return nil
}

func sameNode(a, b protocol.NodeID) bool { return a == b }

func sameProject(a, b *protocol.ProjectID) bool {
	if a == nil && b == nil {
		return true
	}
	if a == nil || b == nil {
		return false
	}
	return *a == *b
}

func sameInt64Ptr(a, b *int64) bool {
	if a == nil && b == nil {
		return true
	}
	if a == nil || b == nil {
		return false
	}
	return *a == *b
}

func normalizeDraft(input protocol.Reminder, localNode protocol.NodeID, capture bool) (protocol.Reminder, error) {
	if input.ID == "" {
		input.ID = newID()
	}
	if _, err := reminderKey(input.ID); err != nil {
		return protocol.Reminder{}, err
	}
	trimmed := strings.TrimSpace(input.Title)
	if trimmed == "" || len(input.Title) > maxTitleBytes || len(input.Message) > maxMessageBytes {
		return protocol.Reminder{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder content or time")
	}
	if input.DueMs != nil && *input.DueMs < 0 {
		return protocol.Reminder{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder content or time")
	}
	if input.NotifiedMs != nil && *input.NotifiedMs < 0 {
		return protocol.Reminder{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder content or time")
	}
	if err := validateNodeID(input.ExecutionNode); err != nil {
		return protocol.Reminder{}, err
	}
	var execNode *protocol.NodeID
	if input.ExecutionNode != nil {
		cp := *input.ExecutionNode
		execNode = &cp
	} else if capture {
		cp := localNode
		execNode = &cp
	}
	return protocol.Reminder{
		ID:            input.ID,
		ExecutionNode: execNode,
		Project:       input.Project,
		Title:         trimmed,
		Message:       input.Message,
		Completed:     input.Completed,
		DueMs:         input.DueMs,
		NotifiedMs:    input.NotifiedMs,
	}, nil
}

func parseStoredReminder(raw json.RawMessage, id string) (protocol.Reminder, error) {
	var stored protocol.Reminder
	if err := json.Unmarshal(raw, &stored); err != nil {
		return protocol.Reminder{}, fault(protocol.ErrorCodeInvalidRequest, "Reminder storage is incompatible")
	}
	if stored.ID != id {
		return protocol.Reminder{}, fault(protocol.ErrorCodeInvalidRequest, "Reminder storage is incompatible")
	}
	normalized, err := normalizeDraft(stored, protocol.NodeID{}, false)
	if err != nil || normalized.Title != stored.Title {
		return protocol.Reminder{}, fault(protocol.ErrorCodeInvalidRequest, "Reminder storage is incompatible")
	}
	return normalized, nil
}

func canonicalSnapshot(item protocol.Reminder) ([]byte, error) {
	return json.Marshal(item)
}

func checkBase(expectedRev uint64, expectedVal *protocol.Reminder, present bool, currentRev uint64, current protocol.Reminder) (uint64, error) {
	if expectedVal != nil {
		if !present {
			return 0, fault(protocol.ErrorCodeRevisionConflict, "Reminder changed")
		}
		actualBytes, err := canonicalSnapshot(current)
		if err != nil {
			return 0, err
		}
		wantBytes, err := canonicalSnapshot(*expectedVal)
		if err != nil {
			return 0, err
		}
		if !bytes.Equal(actualBytes, wantBytes) {
			return 0, fault(protocol.ErrorCodeRevisionConflict, "Reminder changed")
		}
		return currentRev, nil
	}
	if currentRev != expectedRev {
		return 0, fault(protocol.ErrorCodeRevisionConflict, "Reminder changed")
	}
	return currentRev, nil
}

func parseRevisionString(raw string) (uint64, error) {
	if raw == "" {
		return 0, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder revision")
	}
	if len(raw) > 1 && raw[0] == '0' {
		return 0, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder revision")
	}
	v, err := strconv.ParseUint(raw, 10, 63)
	if err != nil {
		return 0, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder revision")
	}
	return v, nil
}

func marshalRaw(v any) (json.RawMessage, error) {
	b, err := json.Marshal(v)
	if err != nil {
		return nil, err
	}
	return json.RawMessage(b), nil
}
