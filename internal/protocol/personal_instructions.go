package protocol

type PersonalInstructions struct {
	Text     string  `json:"text"`
	Revision *string `json:"revision"`
}

func (value *PersonalInstructions) UnmarshalJSON(data []byte) error {
	type plain PersonalInstructions
	return DecodeObject(data, (*plain)(value), []string{"text"}, false)
}

type CommandSavePersonalInstructionsPayload struct {
	Text             string  `json:"text"`
	ExpectedRevision *string `json:"expected_revision"`
}

func (value *CommandSavePersonalInstructionsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSavePersonalInstructionsPayload
	return DecodeObject(data, (*plain)(value), []string{"text"}, true)
}
