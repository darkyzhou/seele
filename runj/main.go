package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"runtime"
	"strings"

	"github.com/darkyzhou/seele/runj/run"
	"github.com/darkyzhou/seele/runj/spec"
	"github.com/go-playground/validator/v10"
	"github.com/mitchellh/mapstructure"
	"github.com/opencontainers/runc/libcontainer"
	_ "github.com/opencontainers/runc/libcontainer/nsenter"
	"github.com/sirupsen/logrus"
)

func init() {
	if len(os.Args) > 1 && os.Args[1] == "init" {
		runtime.GOMAXPROCS(1)
		runtime.LockOSThread()
		factory, _ := libcontainer.New("")
		if err := factory.StartInitialization(); err != nil {
			os.Exit(1)
		}

		panic("libcontainer Failed to init")
	} else {
		if os.Getenv("RUNJ_DEBUG") != "" {
			logrus.SetLevel(logrus.DebugLevel)
		} else {
			logrus.SetLevel(logrus.FatalLevel)
		}

		logrus.SetOutput(os.Stderr)
	}
}

func main() {
	var input string

	inputFile := os.Getenv("RUNJ_FILE")
	if inputFile == "" {
		scanner := bufio.NewScanner(os.Stdin)

		builder := strings.Builder{}
		for scanner.Scan() {
			builder.WriteString(scanner.Text())
		}
		if err := scanner.Err(); err != nil {
			logrus.WithError(err).Fatal("Error reading from stdin")
		}

		input = builder.String()
	} else {
		data, err := os.ReadFile(inputFile)
		if err != nil {
			logrus.WithError(err).Fatalf("Error reading input file: %s", inputFile)
		}
		input = string(data)
	}

	var payload map[string]interface{}
	if err := json.Unmarshal([]byte(input), &payload); err != nil {
		logrus.WithError(err).Fatal("Error unmarshalling the input")
	}

	var config spec.RunjConfig
	if err := mapstructure.Decode(payload, &config); err != nil {
		logrus.WithError(err).Fatal("Error unmarshalling the input")
	}

	validate := validator.New()
	if err := validate.Struct(config); err != nil {
		logrus.WithError(err).Fatal("Invalid config")
	}

	report, err := run.RunContainer(&config)
	if err != nil {
		logrus.WithError(err).Fatal("Error running the container")
	}

	output, err := json.Marshal(report)
	if err != nil {
		logrus.WithError(err).Fatal("Error marshalling the report")
	}
	fmt.Println(string(output))
}
