package model

import (
	"encoding/json"

	"charm.land/fantasy"
)

const replayContextKey = "sailry.replayContext"

type replayContext struct{ Prefix string }

func (*replayContext) Options()                            {}
func (replay *replayContext) MarshalJSON() ([]byte, error) { return json.Marshal(replay.Prefix) }
func (replay *replayContext) UnmarshalJSON(raw []byte) error {
	return json.Unmarshal(raw, &replay.Prefix)
}

// ReplayPrefix identifies the projected request that produced the next reply.
func ReplayPrefix(call fantasy.Call) string {
	if replay, ok := call.ProviderOptions[replayContextKey].(*replayContext); ok {
		return replay.Prefix
	}
	return ""
}
