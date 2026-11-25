# 贡献指南

感谢你对 OpenLVPA 的兴趣。OpenLVPA 把 OpenBitFun 与 OpenAlgo Desktop 两套原版软件合成一件产物，加上中文化与 LVPA 自己的策略与呈显。本指南说明如何高效参与贡献。

## 行为准则

请保持尊重、友善与建设性沟通。我们欢迎不同背景与经验的贡献者。

## 快速开始

### 环境准备

- Node.js 22.12+
- pnpm 10.15+
- Rust toolchain（通过 rustup 安装）
- 桌面端开发需准备 Tauri 依赖

### 取上游源码

上游源码不入本仓，由同步步按声明取入：

```bash
node scripts/sync-upstream.mjs
```

### 安装依赖

```bash
pnpm install
```

### 常用命令

```bash
pnpm run desktop:dev      # 全链路开发模式
pnpm run desktop:build    # 全链路出包模式
```

## 贡献流程

**需求走 issue，代码走 PR。两件不互代。**

1. **先开 issue**，写清要什么、做到什么算成、不做什么。没有 issue 的改动，维护者可能不处理。
2. **在 issue 上讨论方案**，方案定了再动手。
3. **开 PR 时链上 issue**，正文首行 `Closes #<issue号>`。
4. **PR 交可合并态**，不是草稿态。「等下一步」不是交出的形。
5. **合并由维护者做**。请勿自行合并到 `main`。

## 提交规范

- 分支从 `main` 开，命名 `feat/<issue号>-<短名>`、`fix/<issue号>-<短名>`、`docs/<短名>`、`chore/<短名>`。
- 提交信息一行标题，不超过七十二个字符，写清「改了什么」。正文只在需要交代为什么时写，不写过程、不写读数。
- 一个提交一件事。末尾带 issue 号。
- 禁止强推 `main`。

## 代码规范

- 代码不写解释性注释、不写说明段、不写推理过程。
- 一个缺陷只修一处根因。
- 只转发的包装与单实现适配一律内联。
- 外部数据在边界解析成己方类型，界内直接信类型。

## 上游源码

`obf/` 与 `openalgo-desktop/` 是上游源码树，**只读、只引、只对齐**。

- **不在树内编辑，一个字节不动。**
- 我方改动只落 `crates/seams/`、`crates/wiring/` 与树外声明面。
- **不锁版本**：每次按声明的分支取远端最新提交。锁 `ref`、钉 `commit`、以 `[patch]` 把版本差糊过去，一律不接受。

上游更新后我方改不回来的，请开 issue 说明是哪一处，不要私下改树。

## 许可证

本项目按 GNU Affero General Public License version 3 分发，全文见 `LICENSE`。

OpenBitFun 部分另按 MIT 授权，全文见 `LICENSE-MIT`。

**提交前请确认**：你新增的源文件带 `SPDX-License-Identifier: AGPL-3.0-only` 头；你取用的上游文件保留其原有版权头。
