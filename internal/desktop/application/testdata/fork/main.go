package main

import (
	"log"
	"os"

	"github.com/sailry/sailry-harness/sdk/desktop"
	"github.com/sailry/sailry-harness/sdk/product"
)

func main() {
	options := desktop.Options{
		NativeName: "ExampleDesktop",
		Identity: product.Desktop{
			Version:        "1.0.0",
			DefaultProfile: "~/.example-desktop",
			HostRelease:    product.CodeHost(),
			Release: product.Release{
				Package: product.Package{Name: "ExampleDesktop", ID: "test.example.desktop", Executable: "example-desktop"},
			},
		},
	}
	if err := desktop.Run(os.Args[1:], options); err != nil {
		log.Fatal(err)
	}
}
