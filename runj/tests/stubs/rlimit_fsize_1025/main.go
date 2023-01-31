package main

import (
	"os"
)

func main() {
	data := [1025]byte{}
	os.Stdout.Write(data[:])
}
