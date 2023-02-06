package utils

import (
	"os"

	gonanoid "github.com/matoous/go-nanoid/v2"
)

var RunjInstanceId = gonanoid.MustGenerate("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ", 12)

// Returns true if the specified file exists and is actually a file (not a directory)
func FileExists(path string) bool {
	info, err := os.Stat(path)
	if os.IsNotExist(err) {
		return false
	}

	return !info.IsDir()
}

// Returns true if the specified directory exists and is actually a directory (not a file)
func DirectoryExists(path string) bool {
	info, err := os.Stat(path)
	if os.IsNotExist(err) {
		return false
	}

	return info.IsDir()
}
