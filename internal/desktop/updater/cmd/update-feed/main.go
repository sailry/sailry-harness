// Command update-feed signs a release's Desktop update archives and writes
// its signed catalog. The release workflow runs it once per tag:
//
//	go run ./internal/desktop/updater/cmd/update-feed -version 0.1.6 -output <dir> \
//	    aarch64-apple-darwin=<zip> x86_64-apple-darwin=<zip>
//
// Each ZIP holds the sealed, signed and notarized Sailry.app of its target.
// The output directory receives Sailry-<target>.zip, its .update.json and
// update-v1.json; existing files are never replaced. The publisher key is
// the base64 seed in SAILRY_UPDATE_SIGNING_KEY and must belong to
// internal/desktop/updater/update-public-keys.txt.
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
