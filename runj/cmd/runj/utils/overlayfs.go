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

	// * `userxattr` is mandatory when using overlayfs with user namespace.
	//    Note that when using tmpfs directories as upperdirs, overlayfs will complain 'failed to set xattr on upper'
	//    because tmpfs does NOT support user extended attributes(user xattrs).
	// * `xino=off` is used to prevent overlayfs from complaining that some filesystem 'does not support file handles'
	//    which seems to because runj lacks CAP_DAC_READ_SEARCH.
	// * `index=off` is used as Moby also sets it.
	options := fmt.Sprintf("userxattr,xino=off,index=off,lowerdir=%s,upperdir=%s,workdir=%s", config.LowerDirectory, config.UpperDirectory, config.WorkDirectory)
	if err := unix.Mount("overlay", config.MergedDirectory, "overlay", 0, options); err != nil {
		return fmt.Errorf("Error creating overlayfs mount: %w", err)
	}

	_ = os.Setenv("GOMAXPROCS", "1")

	return nil
}
