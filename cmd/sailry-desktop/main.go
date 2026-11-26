package main

import (
	"log"
	"os"

	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	hostassembly "github.com/sailry/sailry-harness/internal/assembly/host"
	"github.com/sailry/sailry-harness/internal/identity"
	desktopsdk "github.com/sailry/sailry-harness/sdk/desktop"
)

func main() {
	if err := desktopsdk.Run(os.Args[1:], desktopsdk.Options{Identity: identity.DefaultDesktop(), Assembly: desktop.Harness(), Host: hostassembly.Harness()}); err != nil {
		log.Fatal(err)
	}
}
