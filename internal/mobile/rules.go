package mobile

import (
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/client"
)

func Sync(method string, data []byte) ([]byte, bool) {
	switch method {
	case "project_modes", "select_mode", "session_link":
	default:
		return nil, false
	}
	var input struct {
		Config string `json:"config"`
		Modes  string `json:"modes"`
		Key    string `json:"key"`
		Link   string `json:"link"`
	}
	var value any
	err := arguments(data, &input)
	if err == nil {
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
	}
	encoded, _ := json.Marshal(result(value, err))
	return encoded, true
}
