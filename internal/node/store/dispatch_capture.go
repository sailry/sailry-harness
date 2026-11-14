package store

import (
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// dispatchCapture adds Node-owned proof to the private package capture. The
// immutable callback contains the actual authority; legacy captures have no proof.
type dispatchCapture struct {
	Name             string `json:"name"`
	Digest           string `json:"digest"`
	SettingsRevision uint64 `json:"settings_revision"`
	SessionStart     bool   `json:"session_start,omitempty"`
}

func encodeDispatchCapture(reference *protocol.PluginReference, sessionStart bool) ([]byte, error) {
	if reference == nil {
		return encode(nil)
	}
	return encode(dispatchCapture{reference.Name, reference.Digest, reference.SettingsRevision, sessionStart})
}

func dispatchSessionStart(body []byte) (bool, error) {
	var capture dispatchCapture
	err := json.Unmarshal(body, &capture)
	return capture.SessionStart, err
}
