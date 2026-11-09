// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ProjectsCatalog struct {
	Projects  []Project  `json:"projects"`
	Worktrees []Worktree `json:"worktrees"`
}

func DefaultProjectsCatalog() ProjectsCatalog {
	return ProjectsCatalog{
		Projects:  []Project{},
		Worktrees: []Worktree{},
	}
}
func (value *ProjectsCatalog) UnmarshalJSON(data []byte) error {
	type plain ProjectsCatalog
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"projects", "worktrees"}, false); err != nil {
		return err
	}
	*value = ProjectsCatalog(decoded)
	return nil
}
func (value ProjectsCatalog) MarshalJSON() ([]byte, error) {
	type plain ProjectsCatalog
	encoded := plain(value)
	encoded.Projects = normalizeCollections(encoded.Projects)
	encoded.Worktrees = normalizeCollections(encoded.Worktrees)
	return json.Marshal(encoded)
}

type ProjectsAppearance struct {
	Icon  string `json:"icon"`
	Color string `json:"color"`
}

func DefaultProjectsAppearance() ProjectsAppearance {
	return ProjectsAppearance{
		Icon:  "folder",
		Color: "none",
	}
}
func (value *ProjectsAppearance) UnmarshalJSON(data []byte) error {
	type plain ProjectsAppearance
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"icon", "color"}, false); err != nil {
		return err
	}
	*value = ProjectsAppearance(decoded)
	return nil
}
func (value ProjectsAppearance) MarshalJSON() ([]byte, error) {
	type plain ProjectsAppearance
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ProjectsSource struct {
	Kind string
	Data any
}
type ProjectsSourceClonePayload struct {
	Url    string  `json:"url"`
	Branch *string `json:"branch"`
}

func DefaultProjectsSourceClonePayload() ProjectsSourceClonePayload {
	return ProjectsSourceClonePayload{
		Url:    "",
		Branch: nil,
	}
}
func (value *ProjectsSourceClonePayload) UnmarshalJSON(data []byte) error {
	type plain ProjectsSourceClonePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"url"}, false); err != nil {
		return err
	}
	*value = ProjectsSourceClonePayload(decoded)
	return nil
}
func (value ProjectsSourceClonePayload) MarshalJSON() ([]byte, error) {
	type plain ProjectsSourceClonePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var projectsSourceVariants = variantFactories{"local": nil,
	"clone": func() any { return new(ProjectsSourceClonePayload) }}

func (value ProjectsSource) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, projectsSourceVariants)
}
func (value *ProjectsSource) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", projectsSourceVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ProjectsDraft struct {
	Name       string             `json:"name"`
	Path       string             `json:"path"`
	Appearance ProjectsAppearance `json:"appearance"`
	Source     ProjectsSource     `json:"source"`
}

func DefaultProjectsDraft() ProjectsDraft {
	return ProjectsDraft{
		Name:       "",
		Path:       "",
		Appearance: DefaultProjectsAppearance(),
		Source:     ProjectsSource{},
	}
}
func (value *ProjectsDraft) UnmarshalJSON(data []byte) error {
	type plain ProjectsDraft
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "path", "appearance", "source"}, false); err != nil {
		return err
	}
	*value = ProjectsDraft(decoded)
	return nil
}
func (value ProjectsDraft) MarshalJSON() ([]byte, error) {
	type plain ProjectsDraft
	encoded := plain(value)
	return json.Marshal(encoded)
}
