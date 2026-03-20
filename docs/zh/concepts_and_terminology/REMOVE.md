# 删除

`coast rm` 会完全拆除一个 Coast 实例。它会在实例正在运行时停止该实例，移除 DinD 容器，删除隔离卷，释放端口，移除代理 shell，并从状态中删除该实例。

```bash
coast rm dev-1
```

大多数日常工作流都不需要 `coast rm`。如果你只是想让某个 Coast 运行不同的代码或拥有规范端口，请改用 [Assign and
Unassign](ASSIGN.md) 或 [Checkout](CHECKOUT.md)。当你想要关闭 Coasts、回收每个实例的运行时状态，或在重建 Coastfile 或构建后从头重新创建一个实例时，再使用 `coast rm`。

## 会发生什么

`coast rm` 会执行五个阶段:

1. **验证并定位** — 在状态中查找该实例。如果状态记录已丢失，但仍存在一个具有预期名称的悬空容器，`coast rm` 也会将其清理掉。
2. **按需停止** — 如果实例处于 `Running` 或 `CheckedOut` 状态，Coast 会先关闭内部 compose 栈并停止 DinD 容器。
3. **移除运行时产物** — 移除 Coast 容器并删除该实例的隔离卷。
4. **清理主机状态** — 杀掉残留的端口转发器，释放端口，移除代理 shell，并从状态数据库中删除该实例记录。
5. **保留共享数据** — 共享服务卷和共享服务数据会被保留，不会被删除。

## CLI 用法

```text
coast rm <name>
coast rm --all
```

| Flag | Description |
|------|-------------|
| `<name>` | 按名称删除一个实例 |
| `--all` | 删除当前项目的所有实例 |

`coast rm --all` 会解析当前项目，列出其所有实例，并逐个删除它们。如果没有任何实例，它会正常退出。

## 共享服务和构建

- `coast rm` **不会**删除共享服务数据。
- 如果你还想删除某个共享服务及其数据，请使用 `coast shared-services rm <service>`。
- 如果你想在关闭实例后删除构建产物，请使用 `coast rm-build`。

## 何时使用

- 在重建 Coastfile 或创建新构建之后，并且想要一个全新的实例时
- 当你想要关闭 Coasts 并释放每个实例的容器和卷状态时
- 当某个实例卡住，而重新开始比原地调试更容易时

## 另请参见

- [Run](RUN.md) — 创建一个新的 Coast 实例
- [Assign and Unassign](ASSIGN.md) — 将现有实例重新指向不同的工作树
- [Shared Services](SHARED_SERVICES.md) — `coast rm` 不会删除的内容
- [Builds](BUILDS.md) — 构建产物和 `coast rm-build`
