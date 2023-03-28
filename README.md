<p align="center"><img alt="Banner" src="docs/public/logo.svg"></p>
<h1 align="center">Seele</h1>

<p align="center">
  <img src="https://github.com/darkyzhou/seele/actions/workflows/build.yml/badge.svg">
  <img src="https://img.shields.io/github/v/release/darkyzhou/seele?include_prereleases&label=version&style=flat-square">
  <img src="https://img.shields.io/github/license/darkyzhou/seele?color=FF5531&style=flat-square">
</p>

Seele 是一款面向云原生的在线评测（Online Judge）系统，主要面向计算机相关的在线课程系统、程序设计竞赛等场景。 它作为评测服务接收用户提交的代码，在安全沙箱中运行并返回评测报告。

Seele 的诞生是为了解决当前一些流行的开源在线评测系统在伸缩性、扩展性和观测性上存在的不足。 同时，它的安全沙箱基于著名的容器运行时 runc，并使用 Rootless Containers 技术带来额外的安全性。 目前，Seele 服务于华南某高校的在线课程系统，承接各类实验课程和机试的需求，覆盖来自不同学院的数以千计的师生。

本项目是作者的本科毕业设计，并且处于早期阶段，在功能性和稳定性上可能存在许多不足之处，敬请谅解。 如果你有好的建议或发现了 bug，欢迎发表 issue 并顺便点一下 star。

## 伸缩性

Seele 自设计之初就充分考虑了利用系统提供的多个 CPU 核心提高并行处理请求能力的重要性。

此外，Seele 还可以在多种环境下运行。我们即可以作为系统上的普通用户直接运行 Seele，也可以通过 Docker、Kubernetes 等平台调度多个实例来处理用户请求。
在 Kubernetes 平台上运行时，用户可以根据系统负载自动进行横向扩容，充分利用多台服务器带来的计算资源。

## 扩展性

Seele 允许用户通过 YAML 语言，使用类似 GitHub Actions 风格的结构描述评测任务每个步骤的具体内容，并决定每个步骤之间的依赖关系、并发关系等。
要运行任意程序，用户只需要提供对应的容器镜像名称。Seele 会像 Docker 那样自动安装这些镜像，然后启动容器来运行用户指定的程序。

下面是一个简单的评测任务，它分为三个步骤：添加源文件、编译源文件并执行程序。此评测任务也提供了一段 JavaScript 脚本代码，当评测任务执行时 Seele
会运行这段脚本来为返回的评测报告附加额外的内容。

通过这种方式，Seele 将定义评测流程的职责交给用户，让用户可以自由定制评测流程以应对复杂多变的课程需求。

```yaml
reporter:
  # Seele 提供了全局变量 DATA 表示任务执行状态
  javascript: |
    const date = new Date();
    return {
      report: {
        message: "Hello at " + date;
        type: DATA.steps.prepare.status;
      }
    }

steps:
  prepare:
    action: "seele/add-file@1"
    files:
      # 将下列内容添加为 `main.c` 文件，也可以通过 url 提供文件内容
      - path: "main.c"
        plain: |
          #include <stdio.h>
          int main(void) {
            printf("Hello, world!\n");
            return 0;
          }

  compile:
    action: "seele/run-judge/compile@1"
    # 在 gcc 11-bullseye 镜像中执行编译命令
    # Seele 默认会从 https://hub.docker.com 中下载容器镜像
    image: "gcc:11-bullseye"
    command: "gcc -O2 -Wall main.c -o main"
    sources: ["main.c"]
    saves: ["main"]

  run:
    action: "seele/run-judge/run@1"
    # 在 debian bullseye 镜像中执行编译产生的程序
    image: "debian:bullseye"
    command: "main"
    files: ["main"]
```

## 观测性

Seele 基于 [OpenTelemetry](https://opentelemetry.io/) 提供了良好的观测性，方便维护人员了解评测系统目前的负载状况，以及进行相关的预警设置。
它主要提供了 Tracing 和 Metrics 两项指标。Tracing 能够为每个输入的评测任务进行追踪，收集它在评测系统各个组件中的执行流程。Metrics 能够提供评测系统的负载情况、处理请求速度等信息。

下图展示了 Metrics 指标通过 [Grafana](https://grafana.com/) 进行监控的示例。

![示例 Grafana 面板](docs/public/grafana.png)

下图展示了一次评测任务的 Tracing 数据，通过 [Tempo](https://grafana.com/oss/tempo/) 收集并展示。

![示例 Tracing 数据](docs/public/tempo.png)

## 安全性

Seele 的安全沙箱基于 Linux 内核提供的容器技术，包括 [Control Group v2](https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html)、[Namespaces](https://www.kernel.org/doc/html/latest/admin-guide/namespaces/index.html) 等技术。
它还使用了 [Rootless Containers](https://rootlesscontaine.rs/) 技术，使得它的运行**不需要** `root` 权限。
相比于许多基于 `ptrace` 技术的安全沙箱来说，它具有更好的安全性、可扩展性，并且不会对程序的运行效率造成显著影响。相比于许多基于 `seccomp` 技术的安全沙箱，它则具备更好的灵活性，不需要为每一种评测场景准备系统调用白名单。

安全沙箱的底层基于著名的容器运行时 [runc](https://github.com/opencontainers/runc/)，使得安全沙箱能够伴随后者的更新对 Linux 内核中出现的安全漏洞持续地提供修复，
并且也能够确保容器技术在使用上的正确性。我们为 Seele 安全沙箱的集成测试准备了来自[青岛大学评测系统](https://github.com/QingdaoU/Judger)、
[Vijos](https://github.com/vijos/malicious-code) 和 [Matrix 课程系统](https://matrix.sysu.edu.cn/about)的数十个测例，这些测例的内容涵盖了恶意消耗计算资源、输出大量数据等恶意行为，结果显示安全沙箱均能通过这些测例，保护系统的安全。

## 不具备的功能

Seele 是一个功能较为纯粹的评测系统，它的唯一功能是：接收外部输入的评测任务、执行评测任务并返回评测报告。
它**并不具备**其它评测系统常常具备的功能，包括用户管理、课程管理、竞赛功能、排行榜、网页管理前端等。

Seele 不具备保存评测任务的功能，当系统关闭或在执行评测任务过程中崩溃时，它并不会重新执行评测任务。因此，用户需要自行维护一套机制保存和追踪提交的评测任务。

## 在线文档

更多信息请访问在线文档：[https://seele.darkyzhou.net](https://seele.darkyzhou.net)
