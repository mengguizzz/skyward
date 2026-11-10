# Skyward for Xiaomi Pad 6

适用于小米平板 6（其它相似Soc设备未测试，不得刷入！），设备代号 **pipa**。本发布为社区适配的实验性预发布版。

## 下载文件

| 文件 | 用途 |
| --- | --- |
| `skyward-pipa-cust-installer.zip` | 推荐安装包。在现有 TWRP 或 Skyward 中安装，给当前槽位的 boot 添加 Recovery 启动入口，并将 Skyward 写入 cust 的专用目录。 |
| `skyward-pipa-boot.img` | 独立 boot 镜像，供**调试**与**验证**使用。它不是基于使用者当前 boot 动态打补丁，不建议作为常规升级方式，只适用于临时启动，比如fastboot boot skyward-pipa-boot.img 。 |

## 安装与升级

1. 确认设备为 **pipa**，Bootloader 已解锁，并备份重要数据。
2. 将安装 ZIP 传入设备，在现有 TWRP 中选择安装；也可以开启 Recovery 的 ADB Sideload 后，在电脑执行：

   ```shell
   adb sideload skyward-pipa-cust-installer.zip
   ```
安装器保留当前 boot 的内核和第一阶段 init，合并 Recovery 所需属性，并检查设备、镜像结构与启动补丁冲突。不再以特定内核或 init 哈希作为安装白名单；这不代表任意第三方 boot 都已经验证兼容。

cust 主程序、boot 内的回退程序、设备配置及启动路由均随本次源码重新构建，不再使用冻结的 `pipa-routing.zip`。云端和离线检查通过不等于本次产物已经实机验证。

## 备份与恢复

- 备份保存在内部存储的 `Skyward-backups/<安装时间及槽位>/`，对应路径为 `/data/media/0/Skyward-backups/`。
- 每次安装都会保存安装前的状态。请把首次安装前的备份另存到电脑，后续升级备份可能已经包含 Skyward。

## 当前状态

### 已修复：

- 修复wifi
- 支持data解密（可能只支持安卓17，目前只在A17上面测试过）
- 等等

### 已知问题：
- 在线更新
- 双系统
- 你告诉我
