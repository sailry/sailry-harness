package icons

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// An avatar's seed always draws the same picture, a random one included;
// an icon that names no seed draws none. The default and lead faces are
// white, for Kit to show on a color.
func TestAvatar(t *testing.T) {
	for range 8 {
		avatar := RandomAvatar()
		if _, ok := protocol.RoleAvatarSeed(avatar); !ok || Avatar(avatar) == nil || Avatar(avatar).Body.A != 255 {
			t.Fatalf("random avatar %q: no picture or body color", avatar)
		}
	}
	fox := Avatar("avatar-fox").Face.Image(64, 64, nil)
	avatars.Lock()
	avatars.pictures = nil
	avatars.Unlock()
	again := Avatar("avatar-fox").Face.Image(64, 64, nil)
	if string(fox.Pix) != string(again.Pix) {
		t.Error("avatar-fox draws differently the second time")
	}
	if string(fox.Pix) == string(Avatar("avatar-owl").Face.Image(64, 64, nil).Pix) {
		t.Error("two seeds draw the same picture")
	}
	for _, icon := range []string{"avatar-", "avatar-Fox", "fox", "code", ""} {
		if Avatar(icon) != nil {
			t.Errorf("%q has a picture", icon)
		}
	}
	// Teams draw from the same seeds, in their own style, and a team
	// without one from its ID.
	if TeamAvatar("avatar-fox") == nil || TeamAvatar("code") != nil || DefaultTeamAvatar("ship") == nil ||
		string(TeamAvatar("avatar-fox").Face.Image(64, 64, nil).Pix) == string(fox.Pix) {
		t.Error("team avatars are not their own pictures")
	}
	if corner := DefaultTeamAvatar("ship").Face.Image(64, 64, nil).RGBAAt(2, 2); corner.A != 255 {
		t.Errorf("a team's picture does not fill its square: %v", corner)
	}
	for _, f := range []struct {
		name string
		draw func() *ui.SVG
	}{{"default", DefaultFace}, {"lead", LeadFace}} {
		if body := f.draw().Image(64, 64, nil).RGBAAt(32, 44); body.R < 240 || body.G < 240 || body.B < 240 {
			t.Errorf("the %s face's body is %v, want white", f.name, body)
		}
	}
}
