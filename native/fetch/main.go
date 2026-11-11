// Command fetch stages the pinned native artifacts of one target in
// .runtime/native/<goos>-<goarch>/, the layout the production loaders read
// beside the executable. Run it from the repository root:
//
//	go run ./native/fetch [-target goos/goarch] [-artifacts dir] [-bundle dir] [-host]
//	go run ./native/fetch -seal path [-target goos/goarch] [-host] [-sign identity]
//
// Downloads come only from the official release URLs in the lock files under
// native/ and internal/localmodels/decision, are checked against their pinned
// size and SHA-256, and are cached in .runtime/native/dl. The iroh and capture
// libraries have no upstream binaries: they are taken from the CI native
// job's artifacts (-artifacts, checked against its SHA256SUMS), or from an
// earlier local build already in the output directory. libghostty-vt is not
// handled here; `mygo build` downloads it from the terminal plugin's manifest.
//
// -seal is the step after packaging and signing. It records the shipped
// native libraries in native-seal.json, the lock of the libraries that have
// no upstream pin and, inside a signed macOS application, of the ones
// signing rewrote. For a macOS .app it also rewrites the llama.cpp and
// Python manifests from the signed files and signs the bundle again, so the
// code signature covers the new records. Nested code is not signed again.
package main

import (
	"archive/tar"
	"archive/zip"
	"bytes"
	"compress/bzip2"
	"compress/gzip"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"io/fs"
	"net/http"
	"os"
	"os/exec"
	"path"
	"path/filepath"
	"runtime"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/shipped"
)

// lockFile is the shared shape of the four runtime locks.
type lockFile struct {
	Build         string            `json:"build"`
	Release       string            `json:"release"`
	PythonVersion string            `json:"python_version"`
	Source        string            `json:"source"`
	Targets       map[string]target `json:"targets"`
}

type target struct {
	URL      string   `json:"url"`
	Archive  string   `json:"archive"`
	Size     int64    `json:"size"`
	SHA256   string   `json:"sha256"`
	Platform string   `json:"platform"`
	Files    []pinned `json:"files"`
}

// pinned is one shipped file. The onnxruntime lock leaves size and SHA-256 to
// the loaders' own locks; the archive digest already fixes the bytes.
type pinned struct {
	Name   string `json:"name"`
	Member string `json:"member"`
	Role   string `json:"role,omitempty"`
	Size   int64  `json:"size"`
	SHA256 string `json:"sha256"`
}

type entry struct {
	name string
	mode fs.FileMode
	link string
	body []byte
}

var triples = map[string]string{
	"darwin/arm64":  "aarch64-apple-darwin",
	"darwin/amd64":  "x86_64-apple-darwin",
	"linux/amd64":   "x86_64-unknown-linux-gnu",
	"linux/arm64":   "aarch64-unknown-linux-gnu",
	"windows/amd64": "x86_64-pc-windows-msvc",
	"windows/arm64": "aarch64-pc-windows-msvc",
}

const helperApp = "Sailry Computer Use.app"

// stage is one run: where files go, what ships and what is still missing.
type stage struct {
	goos, triple, output string
	shipped, missing     []string
}

func main() {
	platform := flag.String("target", runtime.GOOS+"/"+runtime.GOARCH, "GOOS/GOARCH to stage")
	artifacts := flag.String("artifacts", "", "directory of the CI native job's libraries and their SHA256SUMS")
	bundle := flag.String("bundle", "", "directory that receives a copy of the shipped files")
	host := flag.Bool("host", false, "stage what sailry-host ships: no speech or capture library")
	sealed := flag.String("seal", "", "packaged directory or signed macOS .app to seal instead of staging")
	identity := flag.String("sign", "-", "with -seal of a macOS .app: the identity that signed it")
	entitlements := flag.String("entitlements", "cmd/sailry-desktop/macos/app.entitlements.plist", "with -seal of a macOS .app: its entitlements")
	flag.Parse()
	triple, supported := triples[*platform]
	if !supported {
		fail(fmt.Errorf("unsupported target %s", *platform))
	}
	if _, err := os.Stat("go.mod"); err != nil {
		fail(errors.New("run from the repository root"))
	}
	goos, goarch, _ := strings.Cut(*platform, "/")
	run := &stage{goos: goos, triple: triple, output: filepath.Join(".runtime", "native", goos+"-"+goarch)}
	if *sealed != "" {
		fail(run.seal(*sealed, *platform, *host, *identity, *entitlements))
		return
	}
	fail(os.MkdirAll(run.output, 0o755))
	names := built(goos)
	fail(run.onnx(*host))
	fail(run.llama())
	fail(run.helper())
	fail(run.python())
	fail(run.built(names[0], "sh native/iroh/build.sh", *artifacts))
	if !*host {
		fail(run.built(names[1], "sh native/capture/build.sh, on Windows see native/capture/README.md", *artifacts))
	}
	if *bundle != "" {
		for _, name := range run.shipped {
			// Trees are replaced whole so no earlier file stays in the package.
			if name == "helpers" || name == "python" {
				fail(os.RemoveAll(filepath.Join(*bundle, name)))
			}
			fail(copyTree(filepath.Join(run.output, name), filepath.Join(*bundle, name)))
		}
		fmt.Printf("bundle  %s\n", *bundle)
	}
	if len(run.missing) > 0 {
		fmt.Fprintf(os.Stderr, "must be built on %s, or passed with -artifacts:\n", *platform)
		for _, line := range run.missing {
			fmt.Fprintln(os.Stderr, "  "+line)
		}
		os.Exit(1)
	}
}

// built names the iroh and capture libraries Sailry builds itself.
func built(goos string) []string {
	switch goos {
	case "darwin":
		return []string{"libsailry_iroh.dylib", "libsailry-capture.dylib"}
	case "windows":
		return []string{"sailry_iroh.dll", "sailry-capture.dll"}
	}
	return []string{"libsailry_iroh.so", "libsailry-capture.so"}
}

func fail(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, "fetch:", err)
		os.Exit(1)
	}
}

func digest(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

func (run *stage) load(path string) (*lockFile, target, error) {
	source, err := os.ReadFile(path)
	if err != nil {
		return nil, target{}, err
	}
	var lock lockFile
	if err := json.Unmarshal(source, &lock); err != nil {
		return nil, target{}, fmt.Errorf("%s: %w", path, err)
	}
	asset, present := lock.Targets[run.triple]
	if !present {
		return nil, target{}, fmt.Errorf("%s has no target %s", path, run.triple)
	}
	if asset.URL == "" {
		asset.URL = lock.Source + asset.Archive
	}
	return &lock, asset, nil
}

// archive returns the pinned archive, from the cache or its official URL.
func archive(asset target) ([]byte, error) {
	cache := filepath.Join(".runtime", "native", "dl")
	cached := filepath.Join(cache, asset.Archive)
	data, err := os.ReadFile(cached)
	downloaded := errors.Is(err, fs.ErrNotExist)
	if downloaded {
		response, err := http.Get(asset.URL)
		if err != nil {
			return nil, err
		}
		defer response.Body.Close()
		if response.StatusCode != http.StatusOK {
			return nil, fmt.Errorf("%s: %s", asset.URL, response.Status)
		}
		if data, err = io.ReadAll(io.LimitReader(response.Body, asset.Size+1)); err != nil {
			return nil, err
		}
	} else if err != nil {
		return nil, err
	}
	if int64(len(data)) != asset.Size || digest(data) != asset.SHA256 {
		return nil, fmt.Errorf("%s does not match its pinned size and SHA-256", asset.Archive)
	}
	if downloaded {
		if err := os.MkdirAll(cache, 0o755); err != nil {
			return nil, err
		}
		if err := os.WriteFile(cached+".part", data, 0o644); err != nil {
			return nil, err
		}
		if err := os.Rename(cached+".part", cached); err != nil {
			return nil, err
		}
	}
	return data, nil
}

// entries reads every file and link of a .zip, .tar.gz or .tar.bz2 archive.
func entries(name string, data []byte) ([]entry, error) {
	var list []entry
	if strings.HasSuffix(name, ".zip") {
		reader, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
		if err != nil {
			return nil, err
		}
		for _, file := range reader.File {
			if file.FileInfo().IsDir() {
				continue
			}
			source, err := file.Open()
			if err != nil {
				return nil, err
			}
			body, err := io.ReadAll(source)
			source.Close()
			if err != nil {
				return nil, err
			}
			list = append(list, entry{name: path.Clean(file.Name), mode: file.Mode(), body: body})
		}
		return list, nil
	}
	var stream io.Reader = bzip2.NewReader(bytes.NewReader(data))
	if !strings.HasSuffix(name, ".bz2") {
		var err error
		if stream, err = gzip.NewReader(bytes.NewReader(data)); err != nil {
			return nil, err
		}
	}
	reader := tar.NewReader(stream)
	for {
		header, err := reader.Next()
		if err == io.EOF {
			return list, nil
		}
		if err != nil {
			return nil, err
		}
		item := entry{name: path.Clean(header.Name), mode: header.FileInfo().Mode()}
		switch header.Typeflag {
		case tar.TypeDir:
			continue
		case tar.TypeSymlink:
			item.link = header.Linkname
		case tar.TypeReg:
			if item.body, err = io.ReadAll(reader); err != nil {
				return nil, err
			}
		default:
			return nil, fmt.Errorf("%s: unexpected entry %s", name, header.Name)
		}
		list = append(list, item)
	}
}

// members downloads the archive and returns the pinned files by output name.
// With exact, the archive may hold nothing else.
func members(asset target, exact bool) (map[string][]byte, error) {
	data, err := archive(asset)
	if err != nil {
		return nil, err
	}
	list, err := entries(asset.Archive, data)
	if err != nil {
		return nil, err
	}
	if exact && len(list) != len(asset.Files) {
		return nil, fmt.Errorf("%s has unexpected entries", asset.Archive)
	}
	result := map[string][]byte{}
	for _, file := range asset.Files {
		index := slices.IndexFunc(list, func(item entry) bool { return item.name == file.Member && item.link == "" })
		if index < 0 || file.Name != filepath.Base(file.Name) {
			return nil, fmt.Errorf("%s: %s is missing", asset.Archive, file.Member)
		}
		body := list[index].body
		if file.SHA256 != "" && (int64(len(body)) != file.Size || digest(body) != file.SHA256) {
			return nil, fmt.Errorf("%s does not match its pinned size and SHA-256", file.Name)
		}
		result[file.Name] = body
	}
	return result, nil
}

// write publishes a new inode, so a process that mapped the old file keeps it.
func (run *stage) write(name string, body []byte, mode fs.FileMode) error {
	destination := filepath.Join(run.output, filepath.FromSlash(name))
	if err := os.MkdirAll(filepath.Dir(destination), 0o755); err != nil {
		return err
	}
	if err := os.Remove(destination); err != nil && !errors.Is(err, fs.ErrNotExist) {
		return err
	}
	return os.WriteFile(destination, body, mode)
}

func (run *stage) ship(name, version string, body []byte) {
	run.shipped = append(run.shipped, name)
	fmt.Printf("%-34s %-10s %s\n", name, version, digest(body))
}

func (run *stage) onnx(host bool) error {
	lock, asset, err := run.load("native/onnxruntime.lock.json")
	if err != nil {
		return err
	}
	if host {
		asset.Files = slices.DeleteFunc(asset.Files, func(file pinned) bool { return file.Role == "speech" })
	}
	content, err := members(asset, false)
	if err != nil {
		return err
	}
	for _, file := range asset.Files {
		if err := run.write(file.Name, content[file.Name], 0o644); err != nil {
			return err
		}
		run.ship(file.Name, path.Base(strings.TrimSuffix(lock.Source, "/")), content[file.Name])
	}
	return nil
}

func (run *stage) llama() error {
	lock, asset, err := run.load("internal/localmodels/decision/llama-runtime.lock.json")
	if err != nil {
		return err
	}
	content, err := members(asset, false)
	if err != nil {
		return err
	}
	installed := []map[string]any{}
	for _, file := range asset.Files {
		if err := run.write(file.Name, content[file.Name], 0o644); err != nil {
			return err
		}
		run.ship(file.Name, lock.Build, content[file.Name])
		installed = append(installed, map[string]any{"name": file.Name, "size": file.Size, "sha256": file.SHA256})
	}
	manifest, err := json.MarshalIndent(map[string]any{"version": 1, "target": run.triple, "build": lock.Build, "files": installed}, "", "  ")
	if err != nil {
		return err
	}
	run.shipped = append(run.shipped, "llama-runtime.json")
	return run.write("llama-runtime.json", append(manifest, '\n'), 0o644)
}

func (run *stage) helper() error {
	lock, asset, err := run.load("native/agent-desktop.lock.json")
	if err != nil {
		return err
	}
	content, err := members(asset, true)
	if err != nil {
		return err
	}
	license, err := os.ReadFile("native/agent-desktop-LICENSE")
	if err != nil {
		return err
	}
	if err := os.RemoveAll(filepath.Join(run.output, "helpers")); err != nil {
		return err
	}
	// agent-desktop finds its platform helper beside its own executable.
	directory, resources := "helpers/", "helpers/"
	if asset.Platform == "macos" {
		directory = "helpers/" + helperApp + "/Contents/MacOS/"
		resources = "helpers/" + helperApp + "/Contents/Resources/"
		// The identifier owns the Accessibility and Screen Recording grants.
		plist := fmt.Sprintf(`<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key><string>en</string>
	<key>CFBundleDisplayName</key><string>Sailry Computer Use</string>
	<key>CFBundleExecutable</key><string>agent-desktop</string>
	<key>CFBundleIdentifier</key><string>dev.sailry.computer</string>
	<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
	<key>CFBundleName</key><string>Sailry Computer Use</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>CFBundleShortVersionString</key><string>%[1]s</string>
	<key>CFBundleVersion</key><string>%[1]s</string>
	<key>LSMinimumSystemVersion</key><string>13.0</string>
	<key>LSUIElement</key><true/>
	<key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
`, lock.Release)
		if err := run.write("helpers/"+helperApp+"/Contents/Info.plist", []byte(plist), 0o644); err != nil {
			return err
		}
	}
	for _, file := range asset.Files {
		if err := run.write(directory+file.Name, content[file.Name], 0o755); err != nil {
			return err
		}
		fmt.Printf("%-34s %-10s %s\n", file.Name, lock.Release, file.SHA256)
	}
	run.shipped = append(run.shipped, "helpers")
	return run.write(resources+"agent-desktop-LICENSE", license, 0o644)
}

// listed is one entry of the Python receipt: a file or a link.
type listed struct {
	Path   string `json:"path"`
	Size   *int   `json:"size,omitempty"`
	SHA256 string `json:"sha256,omitempty"`
	Link   string `json:"link,omitempty"`
}

// python extracts the standalone runtime, marks it externally managed and
// writes the receipt internal/node/python verifies.
func (run *stage) python() error {
	lock, asset, err := run.load("native/python-runtime.lock.json")
	if err != nil {
		return err
	}
	data, err := archive(asset)
	if err != nil {
		return err
	}
	list, err := entries(asset.Archive, data)
	if err != nil {
		return err
	}
	root := filepath.Join(run.output, "python")
	if err := os.RemoveAll(root); err != nil {
		return err
	}
	library := "python/lib/python3.12/"
	if run.goos == "windows" {
		library = "python/Lib/"
	}
	list = append(list, entry{name: library + "EXTERNALLY-MANAGED",
		body: []byte("[externally-managed]\nError=Use a virtual environment to install dependencies\n")})
	list = slices.DeleteFunc(list, func(item entry) bool { return strings.Contains(item.name, "__pycache__") })
	// Links are published after regular files, so extraction never writes through one.
	slices.SortStableFunc(list, func(a, b entry) int { return len(a.link) - len(b.link) })
	files := []listed{}
	for _, item := range list {
		if !strings.HasPrefix(item.name, "python/") || !filepath.IsLocal(item.name) || strings.Contains(item.name, `\`) {
			return fmt.Errorf("%s: invalid path %s", asset.Archive, item.name)
		}
		destination := filepath.Join(run.output, filepath.FromSlash(item.name))
		relative := strings.TrimPrefix(item.name, "python/")
		if item.link != "" {
			if err := os.MkdirAll(filepath.Dir(destination), 0o755); err != nil {
				return err
			}
			if err := os.Symlink(item.link, destination); err != nil {
				return err
			}
			resolved, err := filepath.EvalSymlinks(destination)
			if inside, _ := filepath.Rel(root, resolved); err != nil || !filepath.IsLocal(inside) {
				return fmt.Errorf("%s: link %s leaves the runtime", asset.Archive, item.name)
			}
			files = append(files, listed{Path: relative, Link: item.link})
			continue
		}
		mode := fs.FileMode(0o644)
		if item.mode&0o111 != 0 {
			mode = 0o755
		}
		if err := run.write(item.name, item.body, mode); err != nil {
			return err
		}
		size := len(item.body)
		files = append(files, listed{Path: relative, Size: &size, SHA256: digest(item.body)})
	}
	slices.SortFunc(files, func(a, b listed) int { return strings.Compare(a.Path, b.Path) })
	receipt, err := json.MarshalIndent(map[string]any{
		"version": 1, "python_version": lock.PythonVersion, "target": run.triple,
		"source": map[string]string{"url": asset.URL, "sha256": asset.SHA256}, "files": files,
	}, "", "  ")
	if err != nil {
		return err
	}
	run.shipped = append(run.shipped, "python")
	fmt.Printf("%-34s %-10s %s\n", "python/ (archive)", lock.PythonVersion, asset.SHA256)
	return run.write("python/python-runtime.json", append(receipt, '\n'), 0o644)
}

// built places a library Sailry builds itself: the CI artifact when given,
// otherwise the local build already in the output directory.
func (run *stage) built(name, build, artifacts string) error {
	destination := filepath.Join(run.output, name)
	if artifacts != "" {
		sums, err := os.ReadFile(filepath.Join(artifacts, "SHA256SUMS"))
		if err != nil {
			return err
		}
		body, err := os.ReadFile(filepath.Join(artifacts, name))
		if err != nil {
			return err
		}
		if !slices.ContainsFunc(strings.Split(string(sums), "\n"), func(line string) bool {
			fields := strings.Fields(line)
			return len(fields) == 2 && fields[0] == digest(body) && strings.TrimPrefix(fields[1], "./") == name
		}) {
			return fmt.Errorf("%s does not match the artifacts' SHA256SUMS", name)
		}
		if err := run.write(name, body, 0o644); err != nil {
			return err
		}
	}
	body, err := os.ReadFile(destination)
	if errors.Is(err, fs.ErrNotExist) {
		run.missing = append(run.missing, name+" ("+build+")")
		return nil
	}
	if err != nil {
		return err
	}
	source := "local"
	if artifacts != "" {
		source = "artifact"
	}
	run.ship(name, source, body)
	return nil
}

// copyTree copies a file or directory, keeping links and permissions.
func copyTree(source, destination string) error {
	return filepath.WalkDir(source, func(current string, item fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		relative, err := filepath.Rel(source, current)
		if err != nil {
			return err
		}
		output := filepath.Join(destination, relative)
		info, err := item.Info()
		if err != nil {
			return err
		}
		switch {
		case item.IsDir():
			return os.MkdirAll(output, 0o755)
		case info.Mode()&fs.ModeSymlink != 0:
			link, err := os.Readlink(current)
			if err != nil {
				return err
			}
			return os.Symlink(link, output)
		}
		body, err := os.ReadFile(current)
		if err != nil {
			return err
		}
		if err := os.MkdirAll(filepath.Dir(output), 0o755); err != nil {
			return err
		}
		if err := os.Remove(output); err != nil && !errors.Is(err, fs.ErrNotExist) {
			return err
		}
		return os.WriteFile(output, body, info.Mode().Perm())
	})
}

// seal records the shipped native libraries of a packaged directory or a
// signed macOS application in the package seal.
func (run *stage) seal(location, platform string, host bool, identity, entitlements string) error {
	directory, app := location, run.goos == "darwin" && strings.HasSuffix(filepath.Clean(location), ".app")
	if app {
		if runtime.GOOS != "darwin" {
			return errors.New("sealing a macOS application needs macOS")
		}
		directory = filepath.Join(location, "Contents", "Resources")
	}
	_, asset, err := run.load("native/onnxruntime.lock.json")
	if err != nil {
		return err
	}
	names := built(run.goos)
	if host {
		names = names[:1]
	}
	for _, file := range asset.Files {
		if !host || file.Role != "speech" {
			names = append(names, file.Name)
		}
	}
	slices.Sort(names)
	record := func() (shipped.Seal, error) {
		seal := shipped.Seal{Version: 1, Target: platform}
		for _, name := range names {
			file, err := shipped.Record(directory, name)
			if err != nil {
				return seal, err
			}
			seal.Files = append(seal.Files, file)
		}
		return seal, nil
	}
	seal, err := record()
	if err != nil {
		return err
	}
	// Only signing a macOS application rewrites shipped files; elsewhere
	// both manifests keep their staged content and the loaders keep the
	// upstream lock.
	if app {
		if err := run.sealLlama(directory); err != nil {
			return err
		}
		if err := sealPython(filepath.Join(directory, "python")); err != nil {
			return err
		}
	}
	if err := writeJSON(filepath.Join(directory, shipped.SealName), seal); err != nil {
		return err
	}
	for _, file := range seal.Files {
		fmt.Printf("%-34s %-10s %s\n", file.Name, "sealed", file.SHA256)
	}
	if !app {
		return nil
	}
	// The bundle is signed again so its signature covers the new records.
	// Nested code keeps the signatures the records describe.
	arguments := []string{"--force", "--sign", identity}
	if identity != "-" {
		arguments = append(arguments, "--options", "runtime", "--timestamp")
	}
	arguments = append(arguments, "--entitlements", entitlements, location)
	for _, arguments := range [][]string{arguments, {"--verify", "--deep", "--strict", location}} {
		if output, err := exec.Command("codesign", arguments...).CombinedOutput(); err != nil {
			return fmt.Errorf("codesign %s: %v\n%s", arguments[0], err, output)
		}
	}
	signed, err := record()
	if err != nil {
		return err
	}
	if !slices.Equal(signed.Files, seal.Files) {
		return errors.New("signing the bundle changed a sealed library")
	}
	fmt.Printf("sealed  %s\n", location)
	return nil
}

// sealLlama rewrites the llama.cpp manifest from the files as shipped.
func (run *stage) sealLlama(directory string) error {
	lock, asset, err := run.load("internal/localmodels/decision/llama-runtime.lock.json")
	if err != nil {
		return err
	}
	installed := []shipped.File{}
	for _, file := range asset.Files {
		record, err := shipped.Record(directory, file.Name)
		if err != nil {
			return err
		}
		installed = append(installed, record)
	}
	return writeJSON(filepath.Join(directory, "llama-runtime.json"),
		map[string]any{"version": 1, "target": run.triple, "build": lock.Build, "files": installed})
}

// sealPython rewrites the file list of the Python receipt from the runtime
// as shipped, keeping its provenance.
func sealPython(root string) error {
	receipt := filepath.Join(root, "python-runtime.json")
	source, err := os.ReadFile(receipt)
	if err != nil {
		return err
	}
	var content map[string]any
	if err := json.Unmarshal(source, &content); err != nil {
		return fmt.Errorf("%s: %w", receipt, err)
	}
	files := []listed{}
	err = filepath.WalkDir(root, func(current string, item fs.DirEntry, err error) error {
		if err != nil || item.IsDir() || current == receipt {
			return err
		}
		relative, err := filepath.Rel(root, current)
		if err != nil {
			return err
		}
		relative = filepath.ToSlash(relative)
		if strings.Contains(relative, "__pycache__") {
			return fmt.Errorf("%s: bytecode cannot be shipped", relative)
		}
		if item.Type()&fs.ModeSymlink != 0 {
			link, err := os.Readlink(current)
			if err != nil {
				return err
			}
			resolved, err := filepath.EvalSymlinks(current)
			if inside, _ := filepath.Rel(root, resolved); err != nil || !filepath.IsLocal(inside) {
				return fmt.Errorf("link %s leaves the runtime", relative)
			}
			files = append(files, listed{Path: relative, Link: link})
			return nil
		}
		record, err := shipped.Record(root, filepath.FromSlash(relative))
		if err != nil {
			return err
		}
		size := int(record.Size)
		files = append(files, listed{Path: relative, Size: &size, SHA256: record.SHA256})
		return nil
	})
	if err != nil {
		return err
	}
	slices.SortFunc(files, func(a, b listed) int { return strings.Compare(a.Path, b.Path) })
	content["files"] = files
	return writeJSON(receipt, content)
}

// writeJSON publishes a new inode, like write.
func writeJSON(path string, value any) error {
	body, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return err
	}
	if err := os.Remove(path); err != nil && !errors.Is(err, fs.ErrNotExist) {
		return err
	}
	return os.WriteFile(path, append(body, '\n'), 0o644)
}
