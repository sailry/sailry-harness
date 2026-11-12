package icons

import (
	"embed"
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
)

// Role avatars are Sailry's own pictures: 192 px squares, a character on a
// plain color that fills the square.

//go:embed avatars/*.jpg
var avatarFiles embed.FS

// AvatarPrefix starts the identifier of a role avatar: "avatar-fox".
const AvatarPrefix = "avatar-"

// AvatarNames are the role avatars in the order the picker shows them, six
// to a row. An avatar's identifier is AvatarPrefix and its name; the Node
// accepts the same identifiers as a role's icon (store.ValidateRole).
var AvatarNames = strings.Fields("fox panda bear rabbit cat dog penguin owl koala frog lion tiger elephant whale duck pig monkey deer hedgehog seal octopus sheep raccoon dino")

// avatars are the pictures decoded so far by identifier, nil for one there
// is none of, so each is read once.
var avatars sync.Map

// Avatar is the picture of a role avatar's identifier, nil for any other
// icon, one this build does not have too.
func Avatar(id string) *ui.Bitmap {
	if found, cached := avatars.Load(id); cached {
		return found.(*ui.Bitmap)
	}
	var picture *ui.Bitmap
	if name, ok := strings.CutPrefix(id, AvatarPrefix); ok {
		if data, err := avatarFiles.ReadFile("avatars/" + name + ".jpg"); err == nil {
			picture, _ = ui.DecodeBitmap(data)
		}
	}
	avatars.Store(id, picture)
	return picture
}
