# pipa 启动阶段的分区 busy 排查

日期：2026-11-10。以下是本次实机证据、已完成的局部修复和未解决范围。
没有修改内核，没有执行第三方 ZIP 刷入，没有刷写 boot、cust 或 super。

## 启动顺序

本次日志中的关键时间均为开机后的秒数：

| 时间 | 事件 |
| --- | --- |
| 2.221 | init：`First stage mount skipped (recovery mode)` |
| 2.689 | cust loader 只读挂载 cust |
| 2.760 | Skyward recovery 开始运行 |
| 2.761–2.763 | 挂载 persist、读取时间偏移，随后正常卸载报 busy |
| 2.764–2.765 | 挂载 metadata |
| 2.777–2.823 | Skyward 映射并挂载 system、system_ext、product、vendor、odm |
| 2.830 | 根据设备规则移交设备节点的属主 |
| 2.837–2.840 | 读取系统属性、挂载 modem 固件 |
| 7.820 | 重新发布 keystore 服务 |
| 16.784–19.809 | 挂载并解密 data |
| 30.718 | 第一次安装准备阶段，pin vendor 的程序及库 |
| 30.729 | cnss 私有命名空间的 slot 挂载清理完成 |
| 30.732 | 主命名空间正常卸载 `/odm` 失败，ZIP 脚本尚未执行 |

因此，本次并非系统第一阶段先挂载所有逻辑分区再交给 cust。
super 中这些文件系统由 Skyward 的 `prepare → mount_partitions` 挂载。
persist 的失败早于 cnss 启动，也早于 data 解密。

## 为什么看不到持有文件的进程

AOSP Android 14 的 `init/mount_handler.cpp` 中，`SetMountProperty` 会：

1. 监听 `/proc/mounts` 的变化。
2. 处理源路径以 `/dev/block/` 开头的挂载。
3. 调用 `stat(entry.mount_point)`。
4. 设置 `dev.mnt.blk.*`、`dev.mnt.dev.*`、`dev.mnt.rootdisk.*` 属性。

设备上也能读到 `dev.mnt.blk.odm=sda29`、
`dev.mnt.blk.skyward.persist=sda22`。

结合前一次独立实验发现的路径属性查询引用问题，这解释了一条完整路径：
Skyward 挂载分区后，init 自动查询挂载点属性，在当前内核环境下留下引用；
查询已经返回，所以进程 FD、maps、cwd 中不一定还能找到对应占用。

在隔离命名空间内重放现有 recovery 的 clock 读取流程，正常卸载成功。
由于原设备在父命名空间已经作为 ext4 挂载，重放时明确跳过了错误文件系统
erofs 的探测，只测试已知 ext4 的读取与卸载。这不能当作完整冷启动的等价测试，
但说明 clock 本身的读文件流程并非必然留下引用。

## 对 init 的只读挂载对照

在主命名空间中，对同一个 persist 设备创建全新的只读挂载视图，
等待一秒让 init 处理挂载事件，程序自身不查询挂载点属性：

| 源路径 | 正常卸载 |
| --- | --- |
| `/dev/block/by-name/persist` | EBUSY |
| `/tmp/…` 下指向同一设备的 symlink | 成功 |

两次挂载的真实设备和只读模式相同，区别只有传给 mount 的源路径。
测试只清理自身创建的挂载，没有卸载设备原有的 persist。

## 已写入源码的局部修复

`crates/skyward-block/src/mount.rs`：

- 新增显式启用的 `enable_source_aliases()`，默认不改变挂载方式。
- 启用后，块设备通过 `/skyward/run/block-sources/<major>-<minor>` 的 symlink
  作为挂载源。真实设备、文件系统和挂载选项不变。
- 不再触发 init 针对 `/dev/block/` 源的上述属性查询。
- 验证重复使用的 alias 指向同一设备，冲突时报错。
- bind、remount、普通卸载和 dm 删除仍采用原有规则。

`crates/skyward/src/main.rs`：

- 仅 pipa 的 recovery 入口启用该策略，在读取 persist 和挂载 super 之前执行。
- 写启动日志，便于核对当前运行的构建是否包含这项修改。
- 没有加入内核哈希白名单，也没有设置忽略 busy。

`crates/skyward-vab/src/partitions.rs`：

- `mount_point()` 原本对 `/odm`、`/vendor` 等路径执行 `symlink_metadata`。
  已挂载时，这也是一次会触发问题的路径属性查询。
- Android 上改为 `open(O_PATH | O_NOFOLLOW)` 加 `File::metadata()`，
  通过 fstat 检查目录，仍不把 symlink 当作真实目录。

此策略不会生成上述挂载对应的自动 `dev.mnt.*` 属性。检查了设备当前
ramdisk/vendor 的 init 规则，没有发现依赖这些属性的规则；其他设备默认不启用。
它也不能约束第三方 updater 自己使用什么源路径或查询接口。

## 底层 dm 释放验证

新增 `crates/skyward/examples/pipa_mount_release_probe.rs`：

1. 确认设备是 pipa、没有待处理快照。
2. 读取当前 odm 的 linear 表。
3. 创建独立临时名称的只读 dm 映射，不修改原映射或 super 内容。
4. 通过实际源码中的 alias 后端挂载 erofs。
5. 等待 init 处理，检查 mountinfo 中的源确实为 alias。
6. 执行实际 `mount_point()` 目录检查、读取目录。
7. 普通卸载，删除临时 dm，确认 dm 已不存在。

接入源码后的程序在设备上连续两次通过：

```text
Mount source recorded by kernel: /skyward/run/block-sources/253-6
PASS: ordinary unmount succeeded after init observed the mount
PASS: temporary device-mapper mapping removed; original odm_b unchanged
probe_exit=0
```

这验证了真实文件系统和 dm 的释放，不只是把挂载点隐藏。
Android arm64 release 的 recovery 与诊断程序编译通过；
skyward-block 的 35 项和 skyward-vab 的 16 项单元测试通过。

## 仍然没有完成的部分

- 正式 recovery 的完整冷启动、解密、Wi-Fi、刷包流程尚未以新构建验证。
  当前设备仍运行此前构建，源码修改不会清掉当前已经泄漏的引用。
- Skyward 其他路径检查、厂商服务和第三方静态工具仍可能产生路径查询。
  当前只修正了明确定位的挂载监听及 mount_point 检查。
- cnss、qrtr-ns 使用的 vendor 程序/库，以及 `services::pin()` 保留的
  文件映射是真实引用。当前设备 maps 中仍可见它们，需要单独处理。
  mmap 加 mlock 不是把文件复制到独立 RAM 文件。
- cnss 私有命名空间清理应保留，它解决的是另一类已确认的引用。
- 不能用全局 lazy/force unmount 或忽略 dm 删除错误，代替验证底层释放。
- 不能据此宣称任意第三方内核包或完整 ROM 已经可以刷入。

后续应在新的完整启动中确认上述阶段，再处理 vendor 服务退出/从 RAM 运行
及残余引用。无需为了应用本次源码改动修改内核，但也不能保证所有保留的
第三方内核及任意 updater 都可通过用户空间绕过。
