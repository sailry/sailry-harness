package composer

// ActiveCommand identifies the output currently expanded in the command popover.
func (s *State) ActiveCommand() string {
	if !s.act.commands {
		return ""
	}
	return s.act.command
}
