package utils

import (
	"fmt"
	"io"
	"io/fs"
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

func CheckPermission(path string, bits fs.FileMode) error {
	info, err := os.Stat(path)
	if err != nil {
		return err
	}

	mode := info.Mode()
	if mode&bits != bits {
		return fmt.Errorf("Insufficient permissions, want at least %b", bits)
	}

	return nil
}

func DirectoryEmpty(path string) (bool, error) {
	dir, err := os.Open(path)
	if err != nil {
		return false, err
	}
	defer dir.Close()

	_, err = dir.Readdirnames(1)
	if err == io.EOF {
		return true, nil
	}
	return false, err
}
