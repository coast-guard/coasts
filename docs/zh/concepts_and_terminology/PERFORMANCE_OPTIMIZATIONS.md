# 性能优化

Coast 的设计目标是让分支切换快速完成，但在大型 monorepo 中，默认行为仍可能引入延迟。本页面介绍你在 Coastfile 中可用的调节手段，更重要的是，它们实际会影响 `coast assign` 的哪些部分。

## 为什么 Assign 可能很慢

当把一个 Coast 切换到新的 worktree 时，`coast assign` 会做几件事:

```text
coast assign dev-1 --worktree feature/payments

  1. classify services and optional rebuild-trigger diff
  2. stop affected services
  3. create git worktree (if new)
  4. bootstrap gitignored files into the worktree (first assign only)
  5. remount /workspace
  6. recreate/restart containers
  7. rebuild images for services using "rebuild"
  8. wait for healthy
```

最大的可变成本通常是 **首次 gitignored 引导（bootstrap）**、**容器重启** 和 **镜像重建**。用于重建触发器的可选分支 diff 要便宜得多，但如果你把它指向很宽泛的触发器集合，累计起来仍会有开销。

### Gitignored 文件引导（Bootstrap）

当某个 worktree 第一次被创建时，Coast 会将项目根目录中选定的 gitignored 文件引导到该 worktree 中。

流程如下:

1. 在宿主机上运行 `git ls-files --others --ignored --exclude-standard` 来枚举被忽略的文件。
2. 过滤掉常见的重型目录，以及任何已配置的 `exclude_paths`。
3. 使用 `rsync --files-from` 配合 `--link-dest`，将选定文件以硬链接的方式引入 worktree，而不是逐字节复制。
4. 在内部 worktree 元数据中记录成功的引导，这样后续 assign 到同一个 worktree 时就可以跳过该步骤。

如果没有 `rsync`，Coast 会回退到 `tar` 管道。

诸如 `node_modules`、`.git`、`dist`、`target`、`.next`、`.nuxt`、`.cache`、`.worktrees` 和 `.coasts` 之类的大目录会被自动排除。大型依赖目录预期应由服务缓存或卷来处理，而不是由这个通用引导步骤处理。

由于文件列表会预先生成，`rsync` 会基于一个有针对性的列表工作，而不是盲目爬取整个仓库。即便如此，若仓库包含非常大量的 ignored 文件集合，在首次创建 worktree 时仍可能付出明显的一次性引导成本。如果你需要手动刷新这次引导，请运行 `coast assign --force-sync`。

### 重建触发器 Diff（Rebuild-Trigger Diff）

只有在配置了 `[assign.rebuild_triggers]` 时，Coast 才会计算分支 diff。在这种情况下它会运行:

```bash
git diff --name-only <previous>..<worktree>
```

其结果用于在某个服务的触发器文件都未发生变化时，将该服务从 `rebuild` 降级为 `restart`。

这比旧的“每次 assign 都 diff 所有被跟踪文件”的模型要窄得多。如果你未配置重建触发器，这里根本不会有分支 diff 步骤。

`exclude_paths` 当前不会改变这个 diff。请将触发器列表聚焦于真正的构建期输入，例如 Dockerfile、lockfile 和包清单（package manifest）。

## `exclude_paths` — 针对新 Worktree 的主要调节杆

Coastfile 中的 `exclude_paths` 选项告诉 Coast:在为新 worktree 构建 gitignored 引导文件列表时，跳过整棵目录树。

```toml
[assign]
default = "none"
exclude_paths = [
    "docs",
    "scripts",
    "test-fixtures",
    "apps/mobile",
]
```

如果 Git 跟踪了被排除路径下的文件，它们仍会出现在 worktree 中。Coast 只是避免在首次引导期间花时间枚举并硬链接这些目录树下的 ignored 文件。

当你的仓库根目录包含大量运行中服务不关心的 ignored 大目录时，这个选项最有效:无关的应用、vendor 缓存、测试夹具、生成的文档，以及其他沉重的目录树。

如果你反复 assign 到同一个已经同步过的 worktree，`exclude_paths` 的重要性就会降低，因为引导会被跳过。在这种情况下，服务重启/重建的选择会成为主导因素。

### 选择要排除的内容

先对你的 ignored 文件做画像分析:

```bash
git ls-files --others --ignored --exclude-standard | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

如果你还想查看被跟踪文件的目录布局以便调优重建触发器，可使用:

```bash
git ls-files | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

**保留**以下目录:
- 包含挂载到运行中服务的源代码
- 包含被这些服务导入的共享库
- 包含运行时在首次启动时确实需要的生成文件或缓存
- 在 `[assign.rebuild_triggers]` 中被引用

**排除**以下目录:
- 属于未在你的 Coast 中运行的应用或服务
- 包含与运行时无关的文档、脚本、CI 配置或工具
- 存放已经在别处保留的大型 ignored 缓存，例如专用服务缓存或共享卷

### 示例:包含多个应用的 Monorepo

一个拥有很多顶层目录的 monorepo，但只有其中一部分与此 Coast 中运行的服务有关:

```text
  13,000  bookface/         ← active
   7,000  ycinternal/       ← active
     850  shared/           ← used by both
   3,800  .yarn/            ← excludable
   2,500  startupschool/    ← excludable
     500  misc/             ← excludable
     300  ycapp/            ← excludable
     ...  (12 more dirs)    ← excludable
```

```toml
[assign]
default = "none"
exclude_paths = [
    ".yarn",
    "startupschool",
    "misc",
    "ycapp",
    "apply",
    "cli",
    "deploy",
    "lambdas",
    # ... any other directories not needed by active services
]
```

这能让首次 worktree 引导聚焦于运行中服务实际需要的目录，而不是在无关的 ignored 目录树上耗时。

## 从 `[assign.services]` 中剔除不活跃的服务

如果你的 `COMPOSE_PROFILES` 只启动一部分服务，请从 `[assign.services]` 中移除不活跃的服务。Coast 会为列表中的每个服务评估 assign 策略，而对未运行的服务进行重启或重建是浪费工作。

```toml
# Bad — restarts services that aren't running
[assign.services]
web = "restart"
api = "restart"
mobile-api = "restart"   # not in COMPOSE_PROFILES
batch-worker = "restart"  # not in COMPOSE_PROFILES

# Good — only services that are actually running
[assign.services]
web = "restart"
api = "restart"
```

同样适用于 `[assign.rebuild_triggers]` — 移除不活跃服务的条目。

## 尽可能使用 `"hot"`

`"hot"` 策略会完全跳过容器重启。[文件系统重新挂载](FILESYSTEM.md) 会替换 `/workspace` 下的代码，而服务的文件监视器（Vite、webpack、nodemon、air 等）会自动捕获变更。

```toml
[assign.services]
web = "hot"        # Vite/webpack dev server with HMR
api = "restart"    # Rails/Go — needs a process restart
```

`"hot"` 比 `"restart"` 更快，因为它避免了容器 stop/start 周期。对于任何运行带文件监听的开发服务器的服务都应使用它。将 `"restart"` 留给那些在启动时加载代码且不监听变更的服务（大多数 Rails、Go 和 Java 应用）。

## 使用带触发器的 `"rebuild"`

如果某个服务的默认策略是 `"rebuild"`，那么每次切换分支都会重建 Docker 镜像——即使没有任何会影响镜像的内容发生变化。添加 `[assign.rebuild_triggers]`，用特定文件来控制何时需要重建:

```toml
[assign.services]
worker = "rebuild"

[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json", "package-lock.json"]
```

如果在分支之间这些触发器文件都没有变化，Coast 会跳过重建并回退为重启。这样可以避免在日常代码变更时进行昂贵的镜像构建。

## 总结

| 优化项 | 影响 | 影响范围 | 何时使用 |
|---|---|---|---|
| `exclude_paths` | 高 | 首次 gitignored 引导 | 仓库包含你的 Coast 不需要的大型 ignored 目录树 |
| 移除不活跃服务 | 中 | 服务重启/重建容器 | 当 `COMPOSE_PROFILES` 限制了运行的服务集合 |
| `"hot"` 策略 | 高 | 容器重启 | 带文件监听器的服务（Vite、webpack、nodemon、air） |
| `rebuild_triggers` | 高 | 镜像重建 + 可选分支 diff | 使用 `"rebuild"` 且只在基础设施变更时才需要重建的服务 |

如果新 worktree 第一次 assign 很慢，先从 `exclude_paths` 入手。如果重复 assign 很慢，则聚焦于 `hot` 与 `restart` 的选择、剔除不活跃服务，并保持 `rebuild_triggers` 足够精确。
