package utils

import (
	"encoding/json"
	"fmt"
	"os"

	"github.com/darkyzhou/seele/runj/cmd/runj/entities"
	"golang.org/x/sys/unix"
)

func SetupOverlayfs() error {
	configJson := os.Getenv("GOMAXPROCS")
	if configJson == "" {
		return fmt.Errorf("Unexpected empty overlayfs config")
	}

	var config entities.OverlayfsConfig
	if err := json.Unmarshal([]byte(configJson), &config); err != nil {
		return fmt.Errorf("Error deserializing the overlayfs config: %w", err)
	}

	options := fmt.Sprintf("userxattr,lowerdir=%s,upperdir=%s,workdir=%s", config.LowerDirectory, config.UpperDirectory, config.WorkDirectory)
	if err := unix.Mount("overlay", config.MergedDirectory, "overlay", 0, options); err != nil {
		return fmt.Errorf("Error creating overlayfs mount: %w", err)
	}

	_ = os.Setenv("GOMAXPROCS", "1")

	return nil
}
