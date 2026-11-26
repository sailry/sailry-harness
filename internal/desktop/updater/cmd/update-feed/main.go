// Command update-feed prepares a signed catalog for historical Harness clients.
// Library release automation does not run this command. For the Code handoff:
//
//	go run ./internal/desktop/updater/cmd/update-feed -version 0.3.0 -output <dir> \
//	    aarch64-apple-darwin=<zip> x86_64-apple-darwin=<zip>
//
// The input ZIPs hold the signed, notarized Mac apps and the portable Windows app.
// The output directory receives Sailry-<target>.zip, its .update.json and
// the macOS and Windows catalogs. The publisher key is
// the base64 seed in SAILRY_UPDATE_SIGNING_KEY and must belong to
// internal/identity/update-public-keys.txt.
package main

import (
	"flag"
	"fmt"
	"os"
	"strings"

	"github.com/MoeForge/moego/base64"
	"github.com/sailry/sailry-harness/internal/desktop/updater"
)

func main() {
	version := flag.String("version", "", "release version, the tag without its v")
	output := flag.String("output", "", "directory that receives the release files")
	flag.Parse()
	unsigned := map[string]string{}
	for _, argument := range flag.Args() {
		target, path, found := strings.Cut(argument, "=")
		if !found || unsigned[target] != "" {
			fail("archives are given once each as <target>=<zip>")
		}
		unsigned[target] = path
	}
	if *version == "" || *output == "" {
		fail("-version and -output are required")
	}
	seed, err := base64.StdEncoding.DecodeString(strings.TrimSpace(os.Getenv("SAILRY_UPDATE_SIGNING_KEY")))
	if err != nil || len(seed) == 0 {
		fail("SAILRY_UPDATE_SIGNING_KEY is not a base64 signing key")
	}
	if err := updater.Publish(seed, *version, *output, unsigned); err != nil {
		fail(err.Error())
	}
}

func fail(message string) {
	fmt.Fprintln(os.Stderr, "update-feed: "+message)
	os.Exit(1)
}
