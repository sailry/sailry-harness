package protocol

// CommandResumeControllerLoginPayload replaces an expired authorization only for
// the exact model step waiting on it. Waiting is the executor's opaque state
// token; it is not a new turn or a replay of the original user message.
type CommandResumeControllerLoginPayload struct {
	Session     SessionID     `json:"session"`
	Turn        TurnID        `json:"turn"`
	Waiting     RequestID     `json:"waiting"`
	Previous    CredentialRef `json:"previous"`
	Replacement CredentialRef `json:"replacement"`
}

func (value *CommandResumeControllerLoginPayload) UnmarshalJSON(data []byte) error {
	type plain CommandResumeControllerLoginPayload
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"session", "turn", "waiting", "previous", "replacement"}, false); err != nil {
		return err
	}
	*value = CommandResumeControllerLoginPayload(decoded)
	return nil
}

type FaultDetailsControllerLoginPayload struct {
	Reason string `json:"reason"`
}

func (value *FaultDetailsControllerLoginPayload) UnmarshalJSON(data []byte) error {
	type plain FaultDetailsControllerLoginPayload
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"reason"}, false); err != nil {
		return err
	}
	*value = FaultDetailsControllerLoginPayload(decoded)
	return nil
}
