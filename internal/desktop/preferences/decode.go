package preferences

import (
	"errors"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func (v *Selection) UnmarshalJSON(b []byte) error {
	type plain Selection
	var p plain
	if err := protocol.DecodeObject(b, &p, []string{"package", "mode"}, false); err != nil {
		return err
	}
	*v = Selection(p)
	return nil
}
func (v *Terminal) UnmarshalJSON(b []byte) error {
	type plain Terminal
	var p plain
	if err := protocol.DecodeObject(b, &p, []string{"font_size", "font_family", "paste_protection"}, false); err != nil {
		return err
	}
	*v = Terminal(p)
	return nil
}
func (v *Device) UnmarshalJSON(b []byte) error {
	type plain Device
	var p plain
	if err := protocol.DecodeObject(b, &p, []string{"execution"}, false); err != nil {
		return err
	}
	*v = Device(p)
	return nil
}
func (v *Surfaces) UnmarshalJSON(b []byte) error {
	type plain Surfaces
	var p plain
	if err := protocol.DecodeObject(b, &p, []string{"main"}, false); err != nil {
		return err
	}
	*v = Surfaces(p)
	return nil
}
func (v *Composer) UnmarshalJSON(b []byte) error {
	type plain Composer
	var p plain
	if err := protocol.DecodeObject(b, &p, []string{"mode", "permission"}, false); err != nil {
		return err
	}
	*v = Composer(p)
	return nil
}
func (v *Model) UnmarshalJSON(b []byte) error {
	type plain Model
	p := plain{Features: map[string]string{}}
	if err := protocol.DecodeObject(b, &p, []string{"source", "provider", "model", "effort"}, false); err != nil {
		return err
	}
	*v = Model(p)
	return nil
}
func (v *Dictation) UnmarshalJSON(b []byte) error {
	type plain Dictation
	var p plain
	if err := protocol.DecodeObject(b, &p, []string{"language"}, false); err != nil {
		return err
	}
	if !slices.Contains([]string{"auto", "chinese", "english"}, p.Language) {
		return errors.New("invalid dictation language")
	}
	*v = Dictation(p)
	return nil
}
func (v *Pin) UnmarshalJSON(b []byte) error {
	type plain Pin
	var p plain
	if err := protocol.DecodeObject(b, &p, []string{"pinned"}, false); err != nil {
		return err
	}
	*v = Pin(p)
	return nil
}
func (v *Directory) UnmarshalJSON(b []byte) error {
	type plain Directory
	var p plain
	if err := protocol.DecodeObject(b, &p, []string{"digest", "install_revision", "path"}, false); err != nil {
		return err
	}
	*v = Directory(p)
	return nil
}
func (v *FilePicker) UnmarshalJSON(b []byte) error {
	type plain FilePicker
	var p plain
	if err := protocol.DecodeObject(b, &p, []string{"grid", "hidden"}, false); err != nil {
		return err
	}
	*v = FilePicker(p)
	return nil
}
func (v *ArtifactField) UnmarshalJSON(b []byte) error {
	type plain ArtifactField
	var p plain
	if err := protocol.DecodeObject(b, &p, []string{"name", "values"}, true); err != nil {
		return err
	}
	*v = ArtifactField(p)
	return nil
}
func (v *ArtifactDraft) UnmarshalJSON(b []byte) error {
	type plain ArtifactDraft
	var p plain
	if err := protocol.DecodeObject(b, &p, []string{"form", "fields"}, true); err != nil {
		return err
	}
	*v = ArtifactDraft(p)
	return nil
}
