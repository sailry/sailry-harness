package main

import (
	"log"
	"os"

	"github.com/sailry/sailry-harness/internal/desktop/application"
	"github.com/sailry/sailry-harness/internal/identity"
)

func main() {
	options := application.Options{
		NativeName: "ExampleDesktop",
		Identity: identity.Desktop{
			Version:        "1.0.0",
			DefaultProfile: "~/.example-desktop",
			HostRelease:    identity.OfficialHost(),
			Release: identity.DesktopRelease{
				Package: identity.DesktopPackage{Name: "ExampleDesktop", ID: "test.example.desktop", Executable: "example-desktop"},
			},
		},
	}
	if err := application.Run(os.Args[1:], options); err != nil {
		log.Fatal(err)
	}
}
