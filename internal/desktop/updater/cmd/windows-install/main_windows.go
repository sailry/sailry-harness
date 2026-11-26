package main

import (
	"log"
	"os"

	"github.com/sailry/sailry-harness/internal/desktop/updater"
)

func main() {
	if err := updater.FinishWindowsInstall(os.Args[1:]); err != nil {
		log.Fatal(err)
	}
}
