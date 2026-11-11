# Skyward for Xiaomi Pad 6

适用于小米平板 6（其它相似Soc设备未测试，不得刷入！），设备代号 **pipa**。本发布为社区适配的实验性预发布版。

## 下载文件

平板界面新增手动旋转：点击右上角的旋转图标可切换四个方向，横屏使用左侧导航、右侧详情的双栏布局，竖屏使用单栏。方向随设置保存；暂不支持重力感应自动旋转。本版 UI 仍需实机确认触摸、键盘和旋转后的显示效果。

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

- 下载页内置国行 Android 14 官方完整包 OS2.0.20.0.UMZCNXM。
- 新增 pipa「自动重新刷入 Skyward」：刷机前将内嵌安装包暂存到 RAM，成功后尝试修复新 boot 的启动入口。仅支持通过现有补丁检查的 boot，且 cust 主程序须完整并匹配当前版本；失败会单独显示，不自动重启。
- vendor/odm 可挂载和卸载；所需安全及 Wi-Fi 服务使用内存中的精简运行库，减少对真实 vendor 的占用。
- 卸载 vendor/odm 后保留本次启动读取的分区表配置，避免 data 解密因找不到 `/data` 条目而失败。
- 测试通过了挂载切换、Wi-Fi 联网及 data 解密。
- 修复wifi
- 支持data解密（可能只支持安卓17，目前只在A17上面测试过）
- 等等

### 已知问题：
- 未验证完整 ROM 刷入、无系统环境及任意第三方内核/ROM 的兼容性。
- Skyward 应用内在线自更新。
- 双系统
- 你告诉我