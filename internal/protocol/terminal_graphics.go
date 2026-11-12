// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type TerminalGraphics struct {
	Images     []TerminalGraphicsImage     `json:"images"`
	Placements []TerminalGraphicsPlacement `json:"placements"`
}

func DefaultTerminalGraphics() TerminalGraphics {
	return TerminalGraphics{
		Images:     []TerminalGraphicsImage{},
		Placements: []TerminalGraphicsPlacement{},
	}
}
func (value *TerminalGraphics) UnmarshalJSON(data []byte) error {
	type plain TerminalGraphics
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"images", "placements"}, false); err != nil {
		return err
	}
	*value = TerminalGraphics(decoded)
	return nil
}
func (value TerminalGraphics) MarshalJSON() ([]byte, error) {
	type plain TerminalGraphics
	encoded := plain(value)
	encoded.Images = normalizeCollections(encoded.Images)
	encoded.Placements = normalizeCollections(encoded.Placements)
	return json.Marshal(encoded)
}

type TerminalGraphicsImage struct {
	ID         uint32 `json:"id"`
	Generation uint64 `json:"generation"`
	Width      uint32 `json:"width"`
	Height     uint32 `json:"height"`
	Png        string `json:"png"`
}

func DefaultTerminalGraphicsImage() TerminalGraphicsImage {
	return TerminalGraphicsImage{
		ID:         0,
		Generation: 0,
		Width:      0,
		Height:     0,
		Png:        "",
	}
}
func (value *TerminalGraphicsImage) UnmarshalJSON(data []byte) error {
	type plain TerminalGraphicsImage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "generation", "width", "height", "png"}, false); err != nil {
		return err
	}
	*value = TerminalGraphicsImage(decoded)
	return nil
}
func (value TerminalGraphicsImage) MarshalJSON() ([]byte, error) {
	type plain TerminalGraphicsImage
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalGraphicsPlacement struct {
	Image  uint32     `json:"image"`
	ID     uint32     `json:"id"`
	Column int32      `json:"column"`
	Row    int32      `json:"row"`
	Offset [2]uint32  `json:"offset"`
	Size   [2]uint32  `json:"size"`
	Source [4]uint32  `json:"source"`
	Tile   *[4]uint32 `json:"tile"`
	Cell   [2]uint32  `json:"cell"`
	Z      int32      `json:"z"`
}

func DefaultTerminalGraphicsPlacement() TerminalGraphicsPlacement {
	return TerminalGraphicsPlacement{
		Image:  0,
		ID:     0,
		Column: 0,
		Row:    0,
		Offset: [2]uint32{},
		Size:   [2]uint32{},
		Source: [4]uint32{},
		Tile:   nil,
		Cell:   [2]uint32{},
		Z:      0,
	}
}
func (value *TerminalGraphicsPlacement) UnmarshalJSON(data []byte) error {
	type plain TerminalGraphicsPlacement
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"image", "id", "column", "row", "offset", "size", "source", "cell", "z"}, false); err != nil {
		return err
	}
	*value = TerminalGraphicsPlacement(decoded)
	return nil
}
func (value TerminalGraphicsPlacement) MarshalJSON() ([]byte, error) {
	type plain TerminalGraphicsPlacement
	encoded := plain(value)
	encoded.Offset = normalizeCollections(encoded.Offset)
	encoded.Size = normalizeCollections(encoded.Size)
	encoded.Source = normalizeCollections(encoded.Source)
	encoded.Tile = normalizeCollections(encoded.Tile)
	encoded.Cell = normalizeCollections(encoded.Cell)
	return json.Marshal(encoded)
}
