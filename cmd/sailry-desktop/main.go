package main

import (
	"log"
	"os"

	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/desktop/application"
	"github.com/sailry/sailry-harness/internal/identity"
)

func main() {
	product := identity.OfficialDesktop()
	product.DefaultProfile = core.Default
	if err := application.Run(os.Args[1:], application.Options{Identity: product, Assembly: desktop.Harness()}); err != nil {
		log.Fatal(err)
	}
}
