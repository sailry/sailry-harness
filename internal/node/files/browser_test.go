package files

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/shirou/gopsutil/v4/disk"
)

// TestBrowseLocations is not parallel: it browses under a home of its own,
// set in the environment.
func TestBrowseLocations(t *testing.T) {
	ctx := context.Background()
	// The standard folders follow the system's rules, and only those that
	// exist and can be listed are offered.
	t.Run("places", func(t *testing.T) {
		home := t.TempDir()
		none := func(string) string { return "" }
		kinds := func(places []place) (out []string) {
			for _, entry := range places {
				out = append(out, string(entry.kind)+"="+entry.path)
			}
			return out
		}
		if got, want := kinds(userPlaces("darwin", home, none)), []string{"desktop=" + filepath.Join(home, "Desktop"),
			"documents=" + filepath.Join(home, "Documents"), "downloads=" + filepath.Join(home, "Downloads"), "applications=/Applications"}; !slices.Equal(got, want) {
			t.Errorf("the places of macOS are %q, want %q", got, want)
		}
		// Without a user-dirs.dirs file the environment names a folder, and
		// the usual names the rest.
		env := func(name string) string {
			return map[string]string{"XDG_DOWNLOAD_DIR": "/srv/incoming", "XDG_DESKTOP_DIR": "relative"}[name]
		}
		if got, want := kinds(userPlaces("linux", home, env)), []string{"desktop=" + filepath.Join(home, "Desktop"),
			"documents=" + filepath.Join(home, "Documents"), "downloads=/srv/incoming"}; !slices.Equal(got, want) {
			t.Errorf("the places of Linux without a file are %q, want %q", got, want)
		}
		// The file outranks the environment: localized names, an absolute
		// path, an escaped character, and a folder turned off by naming the
		// home. Lines of any other form are skipped.
		config := filepath.Join(home, "settings")
		if err := os.MkdirAll(config, 0o700); err != nil {
			t.Fatal(err)
		}
		dirs := "# written by xdg-user-dirs-update\n\nXDG_DESKTOP_DIR=\"$HOME/Schreibtisch\"\n  XDG_DOCUMENTS_DIR = \"/data/My \\\"Docs\\\"\"\n" +
			"XDG_DOWNLOAD_DIR=\"$HOME/\"\nXDG_MUSIC_DIR=\"$HOMEWORK/Music\"\nXDG_PICTURES_DIR=relative\nOTHER=\"/x\"\n"
		if err := os.WriteFile(filepath.Join(config, "user-dirs.dirs"), []byte(dirs), 0o600); err != nil {
			t.Fatal(err)
		}
		listed := xdgUserDirs(dirs, home)
		if len(listed) != 3 || listed["XDG_DESKTOP_DIR"] != filepath.Join(home, "Schreibtisch") || listed["XDG_DOCUMENTS_DIR"] != `/data/My "Docs"` || listed["XDG_DOWNLOAD_DIR"] != home {
			t.Errorf("the file reads as %q", listed)
		}
		env = func(name string) string {
			return map[string]string{"XDG_CONFIG_HOME": config, "XDG_DESKTOP_DIR": "/elsewhere"}[name]
		}
		if got, want := kinds(userPlaces("linux", home, env)), []string{"desktop=" + filepath.Join(home, "Schreibtisch"),
			`documents=/data/My "Docs"`, "downloads=" + home}; !slices.Equal(got, want) {
			t.Errorf("the places of Linux are %q, want %q", got, want)
		}

		// Browsing lists the home, then the folders that exist: a missing
		// one, a file of the name and one the account cannot list are left
		// out.
		home, err := filepath.EvalSymlinks(home)
		if err != nil {
			t.Fatal(err)
		}
		t.Setenv("HOME", home)
		t.Setenv("USERPROFILE", home)
		t.Setenv("XDG_CONFIG_HOME", filepath.Join(home, "none"))
		if err := os.Mkdir(filepath.Join(home, "Desktop"), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(home, "Documents"), nil, 0o600); err != nil {
			t.Fatal(err)
		}
		locked := filepath.Join(home, "Downloads")
		if err := os.Mkdir(locked, 0); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = os.Chmod(locked, 0o700) })
		listing, err := Browse(ctx, nil, nil)
		if err != nil {
			t.Fatal(err)
		}
		var places, devices []string
		for _, location := range listing.Locations {
			switch location.Kind {
			case protocol.FileBrowserLocationKindVolume, protocol.FileBrowserLocationKindRoot:
				devices = append(devices, location.Path)
			case protocol.FileBrowserLocationKindApplications:
			default:
				places = append(places, string(location.Kind)+"="+location.Path)
			}
		}
		want := []string{"home=" + home, "desktop=" + filepath.Join(home, "Desktop")}
		// A privileged process, and a file system without modes, list the
		// locked folder all the same.
		if _, err := os.ReadDir(locked); err == nil {
			want = append(want, "downloads="+locked)
		}
		if !slices.Equal(places, want) {
			t.Errorf("the places are %q, want %q", places, want)
		}
		if root := filepath.VolumeName(home) + string(filepath.Separator); !slices.Contains(devices, root) {
			t.Errorf("the devices %q lack the root %s", devices, root)
		}
		for i, location := range listing.Locations {
			if i > 0 && location.Kind != protocol.FileBrowserLocationKindVolume && location.Kind != protocol.FileBrowserLocationKindRoot &&
				(listing.Locations[i-1].Kind == protocol.FileBrowserLocationKindVolume || listing.Locations[i-1].Kind == protocol.FileBrowserLocationKindRoot) {
				t.Errorf("the place %+v follows a device", location)
			}
		}
		// A later page of the directory repeats none of them.
		if page, err := Browse(ctx, nil, &protocol.DirectoryCursor{}); err == nil && len(page.Locations) != 0 {
			t.Errorf("a later page lists %+v", page.Locations)
		}
	})

	// The devices are what a file manager shows of the mounts.
	t.Run("volumes", func(t *testing.T) {
		names := func(list []protocol.FileBrowserLocation) (out []string) {
			for _, location := range list {
				medium := "-"
				if location.Medium != nil {
					medium = string(*location.Medium)
				}
				out = append(out, string(location.Kind)+" "+location.Name+" "+location.Path+" "+medium)
			}
			return out
		}
		none := func(string) (string, *protocol.FileBrowserMedium) { return "", nil }
		local := []string{"rw", "local", "journaled"}
		mac := []disk.PartitionStat{
			{Mountpoint: "/dev", Fstype: "devfs", Opts: []string{"rw", "local", "nobrowse"}},
			{Mountpoint: "/System/Volumes/Data", Fstype: "apfs", Opts: append([]string{"nobrowse"}, local...)},
			{Mountpoint: "/Volumes/Work", Fstype: "apfs", Opts: local},
			{Mountpoint: "/", Fstype: "apfs", Opts: local},
			{Mountpoint: "/Volumes/Recovery", Fstype: "apfs", Opts: append([]string{"nobrowse"}, local...)},
			{Mountpoint: "/Volumes/.timemachine", Fstype: "apfs", Opts: local},
			{Mountpoint: "/Volumes/Work/nested", Fstype: "apfs", Opts: local},
			{Mountpoint: "/Volumes/share", Fstype: "smbfs", Opts: []string{"rw"}},
			{Mountpoint: "/Volumes/Work", Fstype: "apfs", Opts: local},
		}
		if got, want := names(volumes("darwin", mac, "me", "Macintosh HD", none)), []string{"root Macintosh HD / internal",
			"volume Work /Volumes/Work -", "volume share /Volumes/share network"}; !slices.Equal(got, want) {
			t.Errorf("the volumes of macOS are %q, want %q", got, want)
		}
		linux := []disk.PartitionStat{
			{Mountpoint: "/boot/efi", Fstype: "vfat"}, {Mountpoint: "/", Fstype: "ext4"}, {Mountpoint: "/home", Fstype: "ext4"},
			{Mountpoint: "/media/me/USB STICK", Fstype: "exfat"}, {Mountpoint: "/media/other/theirs", Fstype: "exfat"},
			{Mountpoint: "/run/media/me/card", Fstype: "vfat"}, {Mountpoint: "/mnt/data", Fstype: "xfs"}, {Mountpoint: "/mnt", Fstype: "ext4"},
			{Mountpoint: "/mnt/data/inner", Fstype: "xfs"}, {Mountpoint: "/snap/core/1", Fstype: "squashfs"},
		}
		if got, want := names(volumes("linux", linux, "me", "", none)), []string{"root  / internal", "volume USB STICK /media/me/USB STICK external",
			"volume card /run/media/me/card external", "volume data /mnt/data -"}; !slices.Equal(got, want) {
			t.Errorf("the volumes of Linux are %q, want %q", got, want)
		}
		external, network := protocol.FileBrowserMediumExternal, protocol.FileBrowserMediumNetwork
		detail := func(root string) (string, *protocol.FileBrowserMedium) {
			switch root {
			case `C:\`:
				return "Windows", nil
			case `E:\`:
				return "", &external
			}
			return "", &network
		}
		drives := []disk.PartitionStat{{Mountpoint: "C:"}, {Mountpoint: ""}, {Mountpoint: `C:\mounted\`}, {Mountpoint: "E:"}, {Mountpoint: "Z:"}}
		if got, want := names(volumes("windows", drives, "me", "", detail)), []string{`volume Windows (C:) C:\ -`, `volume E: E:\ external`,
			`volume Z: Z:\ network`}; !slices.Equal(got, want) {
			t.Errorf("the drives of Windows are %q, want %q", got, want)
		}

		// The startup volume is named by the link to the root kept beside
		// the other volumes.
		directory := t.TempDir()
		if startupName(directory) != "" || startupName(filepath.Join(directory, "gone")) != "" {
			t.Error("a directory without a link names a startup volume")
		}
		if err := os.Mkdir(filepath.Join(directory, "Work"), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.Symlink(filepath.Join(directory, "Work"), filepath.Join(directory, "Alias")); err != nil {
			t.Skip(err)
		}
		if err := os.Symlink("/", filepath.Join(directory, "Macintosh HD")); err != nil {
			t.Fatal(err)
		}
		if name := startupName(directory); name != "Macintosh HD" {
			t.Errorf("the startup volume is %q", name)
		}

		// A location's medium is optional on the wire, and the new kind is
		// one of the enumeration.
		encoded, err := json.Marshal(protocol.FileBrowserLocation{Kind: protocol.FileBrowserLocationKindVolume, Name: "USB", Path: "/Volumes/USB", Medium: &external})
		if want := `{"medium":"external","kind":"volume","name":"USB","path":"/Volumes/USB"}`; err != nil || string(encoded) != want {
			t.Errorf("the volume encodes as %s %v, want %s", encoded, err, want)
		}
		var decoded protocol.FileBrowserLocation
		if err := json.Unmarshal(encoded, &decoded); err != nil || decoded.Medium == nil || *decoded.Medium != external {
			t.Errorf("the volume decodes as %+v %v", decoded, err)
		}
		if err := json.Unmarshal([]byte(`{"kind":"applications","name":"","path":"/Applications"}`), &decoded); err != nil ||
			decoded.Medium != nil || decoded.Kind != protocol.FileBrowserLocationKindApplications {
			t.Errorf("a location without a medium decodes as %+v %v", decoded, err)
		}
		if err := json.Unmarshal([]byte(`{"kind":"volume","name":"","path":"/x","medium":"tape"}`), &decoded); err == nil {
			t.Error("an unknown medium decodes")
		}
		// What this machine reports, for the record of a run.
		home, _ := os.UserHomeDir()
		if listing, err := Browse(ctx, &home, nil); err == nil {
			t.Logf("this machine: %q", names(listing.Locations))
		}
	})
}
