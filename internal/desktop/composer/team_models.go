package composer

import (
	"maps"

	"github.com/egoist/mygo/ui"
)

// MemberModel keeps a member's controls separate from the lead model.
type MemberModel struct {
	Overridden      bool
	Model           string
	Effort          float64
	Features        map[string]string
	InheritFeatures bool
}

func (s *State) memberSelection() *MemberModel {
	if s.member == "" || s.Team == "" {
		return nil
	}
	if s.MemberModels == nil {
		s.MemberModels = map[string]*MemberModel{}
	}
	if s.MemberModels[s.member] == nil {
		s.MemberModels[s.member] = &MemberModel{Model: s.Model, Effort: s.Effort, Features: maps.Clone(s.Features), InheritFeatures: true}
	}
	return s.MemberModels[s.member]
}

func (s *State) controlModelID() string {
	if member := s.memberSelection(); member != nil {
		return member.Model
	}
	return s.Model
}

func (s *State) controlModel(models []Model) Model {
	if member := s.memberSelection(); member != nil {
		copy := *s
		copy.Model = member.Model
		return copy.Chosen(models)
	}
	return s.Chosen(models)
}

func (s *State) controlEffort() *float64 {
	if member := s.memberSelection(); member != nil {
		return &member.Effort
	}
	return &s.Effort
}

func (s *State) controlEffortName(c *ui.Context, model Model) string {
	copy := *s
	copy.Effort = *s.controlEffort()
	return copy.EffortName(c, model)
}

func (s *State) controlFeatures() map[string]string {
	if member := s.memberSelection(); member != nil {
		if member.Features == nil {
			member.Features = map[string]string{}
		}
		return member.Features
	}
	if s.Features == nil {
		s.Features = map[string]string{}
	}
	return s.Features
}

func (s *State) setControlFeatures(features map[string]string) {
	if member := s.memberSelection(); member != nil {
		member.Features = features
		member.InheritFeatures = false
	} else {
		s.Features = features
	}
}

func (s *State) markMemberOverride() {
	if member := s.memberSelection(); member != nil {
		member.Overridden = true
	}
}

func (s *State) markMemberFeatureOverride() {
	if member := s.memberSelection(); member != nil {
		member.Overridden = true
		member.InheritFeatures = false
	}
}
