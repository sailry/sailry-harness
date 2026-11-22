package main

import (
	"log"
	"os"

	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/desktop/application"
	"github.com/sailry/sailry-harness/internal/identity"
	"github.com/sailry/sailry-harness/modules/code-index/node"
)

func main() {
	product := identity.OfficialDesktop()
	product.DefaultProfile = core.Default
	if err := application.Run(os.Args[1:], application.Options{Identity: product, Assembly: desktop.Harness(), CodeIndex: codeindex.NewService}); err != nil {
		log.Fatal(err)
	}
}
