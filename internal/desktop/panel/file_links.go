package panel

// RevealFileDirectory hands a validated selection to the optional browser.
func (pr *Project) RevealFileDirectory(path string) {
	pr.fileDirectoryTarget = &path
}

func (pr *Project) TakeFileDirectoryReveal() *string {
	path := pr.fileDirectoryTarget
	pr.fileDirectoryTarget = nil
	return path
}
