package viewmodel

// Role is a subagent profile projected for the accepted settings editor.
type Role struct {
	ID, Key, Name, Description string
	Revision                   uint64
	Model, Effort              string
	// Features are the model's feature options the role sets (as speed:
	// fast): nil follows the lead, and a feature absent from a non-nil
	// map is off.
	Features     map[string]string
	ReadOnly     bool
	Instructions string
	// Avatar is the identifier of the role's picture (icons.Avatar), as
	// "avatar-fox"; "" shows the default face on the color its ID picks.
	Avatar string
}

// Team keeps its stored revision with the editable presentation fields.
type Team struct {
	ID, Name, Description      string
	Revision                   uint64
	Members                    []string
	Lead, Effort, Instructions string
	// Features are the lead model's feature options, as Role.Features;
	// nil follows the conversation.
	Features map[string]string
	// Avatar is the identifier of the team's picture (icons.TeamAvatar);
	// "" shows the one its ID picks.
	Avatar string
}
