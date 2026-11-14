package icons

import (
	_ "embed"
	"math/rand/v2"
	"sync"

	dicebear "github.com/dicebear/dicebear-go/v10"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Role avatars are DiceBear's Gaze drawn from a seed: a pair of eyes on a
// colored body. Team avatars are its Shape Grid: a few shapes on a colored
// square, a group as a face is one. The same seed always draws the same
// picture, here and in DiceBear's other ports. See avatars/SOURCE.md.

//go:embed avatars/gaze.json
var gazeDefinition []byte

//go:embed avatars/shape-grid.json
var shapeGridDefinition []byte

// parse reads a style definition once, on its first picture.
func parse(name string, definition []byte) func() *dicebear.Style {
	return sync.OnceValue(func() *dicebear.Style {
		style, err := dicebear.NewStyle(definition)
		if err != nil {
			panic("icons: " + name + ": " + err.Error())
		}
		return style
	})
}

var (
	gaze      = parse("gaze.json", gazeDefinition)
	shapeGrid = parse("shape-grid.json", shapeGridDefinition)
)

// faceScale leaves the body room inside its tile, which Kit fills.
const faceScale = 0.82

// Picture is a drawn avatar. A role's face is transparent around its
// body, whose color tints the tile under it; a team's fills its square,
// and Body is unset.
type Picture struct {
	Face *ui.SVG
	Body ui.Color
}

// team draws the Shape Grid of seed, nil when it is invalid.
func team(seed string) *Picture {
	avatar, err := dicebear.NewAvatar(shapeGrid(), map[string]any{"seed": seed})
	if err != nil {
		return nil
	}
	svg, err := ui.ParseSVG([]byte(avatar.SVG()))
	if err != nil {
		return nil
	}
	return &Picture{Face: svg}
}

// face draws the Gaze face of options, nil when they are invalid.
func face(options map[string]any) *Picture {
	options["scale"] = faceScale
	avatar, err := dicebear.NewAvatar(gaze(), options)
	if err != nil {
		return nil
	}
	svg, err := ui.ParseSVG([]byte(avatar.SVG()))
	if err != nil {
		return nil
	}
	picture := &Picture{Face: svg}
	if body, ok := avatar.ResolvedOptions()["bodyColor"].([]string); ok && len(body) > 0 {
		picture.Body = ui.Hex(body[0])
	}
	return picture
}

// DefaultFace is the face of a role without an avatar: a white round face
// that Kit shows on the role's color, so roles differ by color alone.
var DefaultFace = sync.OnceValue(func() *ui.SVG {
	return face(map[string]any{"seed": "sailry", "bodyColor": []string{"#ffffff"}, "shapeVariant": "circle", "eyesVariant": "dots", "spacingVariant": "normal"}).Face
})

// LeadFace is the face of a team's lead: a white smiling pentagon that Kit
// shows on the primary color.
var LeadFace = sync.OnceValue(func() *ui.SVG {
	return face(map[string]any{"seed": "lead", "bodyColor": []string{"#ffffff"}, "shapeVariant": "pentagon", "eyesVariant": "happy", "spacingVariant": "normal"}).Face
})

// AvatarPrefix starts the identifier of a role avatar (protocol.RoleAvatarSeed).
const AvatarPrefix = protocol.RoleAvatarPrefix

// avatarCacheLimit bounds the pictures kept: the role lists' and a few
// picker pages', so shuffling does not grow it without end.
const avatarCacheLimit = 256

// avatars are the pictures drawn so far by identifier, nil for an
// identifier that names none, so each is generated once.
var avatars struct {
	sync.Mutex
	pictures map[string]*Picture
}

// Avatar is the picture of a role avatar's identifier, nil for any other
// icon.
func Avatar(id string) *Picture {
	return cached("role\x00"+id, func() *Picture {
		if seed, ok := protocol.RoleAvatarSeed(id); ok {
			return face(map[string]any{"seed": seed})
		}
		return nil
	})
}

// TeamAvatar is the picture of a team avatar's identifier, which names a
// seed as a role's does, nil for any other icon.
func TeamAvatar(id string) *Picture {
	return cached("team\x00"+id, func() *Picture {
		if seed, ok := protocol.RoleAvatarSeed(id); ok {
			return team(seed)
		}
		return nil
	})
}

// DefaultTeamAvatar is the picture of a team without an avatar: the Shape
// Grid its ID seeds, so each team has its own from the start.
func DefaultTeamAvatar(id string) *Picture {
	return cached("default\x00"+id, func() *Picture { return team(id) })
}

// cached is the picture drawn under key, drawing it the first time.
func cached(key string, draw func() *Picture) *Picture {
	avatars.Lock()
	picture, ok := avatars.pictures[key]
	avatars.Unlock()
	if ok {
		return picture
	}
	picture = draw()
	avatars.Lock()
	if len(avatars.pictures) >= avatarCacheLimit || avatars.pictures == nil {
		avatars.pictures = make(map[string]*Picture, avatarCacheLimit)
	}
	avatars.pictures[key] = picture
	avatars.Unlock()
	return picture
}

// RandomAvatar is the identifier of a new random avatar: AvatarPrefix and
// a seed of 8 letters and digits.
func RandomAvatar() string {
	const alphabet = "abcdefghijklmnopqrstuvwxyz0123456789"
	seed := make([]byte, 8)
	for i := range seed {
		seed[i] = alphabet[rand.IntN(len(alphabet))]
	}
	return AvatarPrefix + string(seed)
}
