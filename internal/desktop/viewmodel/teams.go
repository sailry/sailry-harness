package viewmodel

// Role is a subagent profile projected for the accepted settings editor.
type Role struct {
	ID, Key, Name, Description string
	Revision                   uint64
	Model, Effort              string
	ReadOnly                   bool
	Instructions               string
	Builtin                    bool
}

// Team keeps its stored revision with the editable presentation fields.
type Team struct {
	ID, Name, Description      string
	Revision                   uint64
	Members                    []string
	Lead, Effort, Instructions string
}
