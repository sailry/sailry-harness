package protocol

// User identifies one local group of controller devices on an execution Node.
// Its identity has no meaning on another Node.
type User struct {
	ID            UserID  `json:"id"`
	Name          string  `json:"name"`
	Administrator bool    `json:"administrator"`
	Local         bool    `json:"local"`
	State         string  `json:"state"`
	MergedInto    *UserID `json:"merged_into,omitempty"`
	Controller    *NodeID `json:"controller,omitempty"`
}

func (value *User) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *User) DecodeJSON(decoder *Decoder) error {
	type plain User
	var decoded plain
	if err := decoder.Object(&decoded, []string{"id", "name", "administrator", "local", "state"}, false); err != nil {
		return err
	}
	*value = User(decoded)
	return nil
}
