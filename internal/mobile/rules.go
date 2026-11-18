package mobile

import (
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const SessionLanesMethod = "session_lanes"

func Sync(method string, data []byte) ([]byte, bool) {
	var value any
	var err error
	switch method {
	case SessionLanesMethod:
		var input struct {
			Sessions []protocol.Session `json:"sessions"`
		}
		// These sessions are a Node's reported snapshot, with the same tolerance
		// for newer fields and values as the original observation.
		if decodeErr := protocol.DecodeAs(protocol.PeerData, data, &input); decodeErr != nil || input.Sessions == nil {
			err = invalid("invalid session lane arguments")
		} else {
			value = client.ProjectActivity(protocol.ActivityCatalog{Sessions: input.Sessions}, nil).SessionLanes
		}
	case "project_modes", "select_mode", "session_link":
		var input struct {
			Config string `json:"config"`
			Modes  string `json:"modes"`
			Key    string `json:"key"`
			Link   string `json:"link"`
		}
		if err = arguments(data, &input); err != nil {
			break
		}
		switch method {
		case "project_modes":
			var projected json.RawMessage
			projected, err = client.ProjectModesJSON(json.RawMessage(input.Config), json.RawMessage(input.Modes))
			if err == nil {
				value = string(projected)
			}
		case "select_mode":
			var selected json.RawMessage
			selected, err = client.SelectModeJSON(json.RawMessage(input.Config), json.RawMessage(input.Modes), input.Key)
			if err == nil && selected != nil {
				value = string(selected)
			}
		case "session_link":
			if session := client.SessionLink(input.Link); session != nil {
				value = session.String()
			}
		}
	default:
		return nil, false
	}
	encoded, _ := json.Marshal(result(value, err))
	return encoded, true
}
