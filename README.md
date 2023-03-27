<p align="center"><img alt="Banner" src="docs/public/logo.svg"></p>
<h1 align="center">Seele</h1>

<p align="center">
  <img src="https://github.com/darkyzhou/seele/actions/workflows/ci.yml/badge.svg">
  <img src="https://img.shields.io/github/v/release/darkyzhou/seele?include_prereleases&label=version&style=flat-square">
  <img src="https://img.shields.io/github/license/darkyzhou/seele?color=FF5531&style=flat-square">
</p>

Seele 是一款面向云原生的在线评测（Online Judge）系统，主要面向计算机相关的在线课程系统、程序设计竞赛等场景。 它作为评测服务接收用户提交的代码，在安全沙箱中运行并返回评测报告。

Seele 的诞生是为了解决当前一些流行的开源在线评测系统在伸缩性、扩展性和观测性上存在的不足。 同时，它的安全沙箱基于著名的容器运行时 runc，并使用 Rootless Containers 技术带来额外的安全性。 目前，Seele 服务于华南某高校的在线课程系统，承接各类实验课程和机试的需求，覆盖来自不同学院的数以千计的师生。

本项目是作者的本科毕业设计，并且处于早期阶段，在功能性和稳定性上可能存在许多不足之处，敬请谅解。 如果你有好的建议或发现了 bug，欢迎发表 issue 并顺便点一下 star。

参见在线文档：[https://seele.darkyzhou.net](https://seele.darkyzhou.net)
