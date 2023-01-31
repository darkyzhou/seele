package main

import (
	"os"
)

func main() {
	data := [1024]byte{}
	os.Stdout.Write(data[:])
}
