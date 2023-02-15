package main

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/signal"
	"runtime"
	"strings"
	"syscall"

	"github.com/darkyzhou/seele/runj/cmd/runj/entities"
	"github.com/darkyzhou/seele/runj/cmd/runj/execute"
	"github.com/darkyzhou/seele/runj/cmd/runj/utils"
	"github.com/go-playground/validator/v10"
	"github.com/mitchellh/mapstructure"
	"github.com/opencontainers/runc/libcontainer"
	_ "github.com/opencontainers/runc/libcontainer/nsenter"
	"github.com/sirupsen/logrus"
)

func init() {
	runtime.GOMAXPROCS(1)

	if len(os.Args) > 1 && os.Args[1] == "init" {
		runtime.LockOSThread()

		if err := utils.SetupOverlayfs(); err != nil {
			// FIXME: Find a way to pass the error message
			os.Exit(1)
		}

		factory, _ := libcontainer.New("")
		if err := factory.StartInitialization(); err != nil {
			os.Exit(1)
		}

		panic("Libcontainer failed to init")
	}

	if os.Getenv("RUNJ_DEBUG") != "" {
		logrus.SetLevel(logrus.DebugLevel)
		logrus.SetOutput(os.Stdout)
	} else {
		logrus.SetLevel(logrus.FatalLevel)
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

	var config entities.RunjConfig
	if err := mapstructure.Decode(payload, &config); err != nil {
		logrus.WithError(err).Fatal("Error unmarshalling the input")
	}

	validate := validator.New()
	if err := validate.Struct(config); err != nil {
		logrus.WithError(err).Fatal("Invalid config")
	}

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	sigs := make(chan os.Signal, 1)
	signal.Notify(sigs, syscall.SIGINT, syscall.SIGTERM)
	go func() {
		<-sigs
		cancel()
	}()

	report, err := execute.Execute(ctx, &config)
	if err != nil {
		logrus.WithError(err).Fatal("Error executing the container")
	}

	output, err := json.Marshal(report)
	if err != nil {
		logrus.WithError(err).Fatal("Error marshalling the report")
	}
	fmt.Println(string(output))
}
