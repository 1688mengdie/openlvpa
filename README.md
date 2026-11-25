# OpenLVPA

**量价时空双线多维交易系统。**

Lion Volume Price Action.

OpenLVPA 把 OpenBitFun 与 OpenAlgo Desktop 两套原版软件合成一件产物，加上中文化与 LVPA 自己的策略与呈显，成品只有一件。

上游源码不入本仓。它由同步步按声明的远端与分支在每次构建时取入，本仓只存组装、接缝与本地化。取用声明见 `NOTICE`。

## 两条命令

```
pnpm run desktop:dev
```

全链路开发模式，改一处源码即生效。

```
pnpm run desktop:build
```

全链路出包模式，一次交出可安装的成品。

两条各跑完整条链，不合成一条。

## 许可证

本成品按 GNU Affero General Public License version 3 分发，全文见 `LICENSE`。

OpenBitFun 部分另按 MIT 授权，全文见 `LICENSE-MIT`，其版权声明随每一份副本保留。

两处上游的地址、分支与许可证归类见 `NOTICE`。
