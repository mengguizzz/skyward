# Skyward for Xiaomi Pad 6

适用于小米平板 6，设备代号 **pipa**。本发布为社区适配的实验性预发布版。

## 下载文件

| 文件 | 用途 |
| --- | --- |
| `skyward-pipa-cust-installer.zip` | 推荐安装包。在现有 TWRP 或 Skyward 中安装，给当前槽位的 boot 添加 Recovery 启动入口，并将 Skyward 写入 cust 的专用目录。 |
| `skyward-pipa-boot.img` | 使用固定测试基底生成的独立 boot 镜像，供调试与验证使用。它不是基于使用者当前 boot 动态打补丁，不建议作为常规升级方式。 |
| `SHA256SUMS` | 镜像、安装包和验证信息的 SHA-256 校验值。 |
| `verification.json`、`build-info.json` | 镜像结构检查、源码提交、输入基底与构建信息。 |

## 安装与升级

1. 确认设备为 **pipa**，Bootloader 已解锁，并备份重要数据。不要用于小米平板 6 Pro 或其他型号。
2. 将安装 ZIP 传入设备，在现有 TWRP 中选择安装；也可以开启 Recovery 的 ADB Sideload 后，在电脑执行：

   ```shell
   adb sideload skyward-pipa-cust-installer.zip
   ```

3. 安装需要 Recovery 能访问真实的内部存储，并至少留有 **1.4 GB** 可用空间，用于保存当前槽位 boot 和完整 cust 分区备份。安装器提示检查失败时，请保留日志排查。
4. 安装成功后选择重启到 Recovery，启动 Skyward；正常重启则进入 Android。
5. 已安装此适配版时，可以在 Skyward 内安装新的同类 ZIP 升级。安装器会先备份，再替换旧的 Skyward 专用目录。

安装器保留当前 boot 的内核和第一阶段 init，合并 Recovery 所需属性，并检查设备、镜像结构与启动补丁冲突。不再以特定内核或 init 哈希作为安装白名单；这不代表任意第三方 boot 都已经验证兼容。

## 备份与恢复

- 备份保存在内部存储的 `Skyward-backups/<安装时间及槽位>/`，对应路径为 `/data/media/0/Skyward-backups/`。
- 每次安装都会保存安装前的状态。请把首次安装前的备份另存到电脑，后续升级备份可能已经包含 Skyward。
- 备份目录包含 `boot.img`、`cust.img`、`SHA256SUMS`、`device.txt` 和 `restore.sh`。需要恢复时，先查看该目录的 `device.txt` 和恢复脚本，确认设备、备份来源与槽位；不要把其他设备或其他槽位的镜像直接刷入。

## 注意事项与已知限制

- 本方案使用 **boot 启动入口 + cust 中的 Skyward**，不会创建独立 recovery 分区。安装会修改当前槽位 boot 和 cust；不要格式化 cust。
- 云端构建成功及镜像结构检查通过，不等于该次产物已经实机验证。请结合 `build-info.json` 中的 `hardware_tested` 信息判断。
- 解密、Wi-Fi 和 MTP 已在适配过程中做过设备验证，但系统版本、加密状态及第三方内核变化仍可能影响功能。Wi-Fi 首次扫描可能需要约 10 秒。
- 已加入安装前释放相关挂载引用的修复；第三方内核包与完整 ROM 的兼容性仍需逐包验证。遇到分区 busy 或卸载失败时，请保留日志，不要直接强制卸载或继续写入。
- 内置 ROM 下载功能不作为本适配版已验证功能。
- 刷入其他内核、完整 ROM 或系统更新可能覆盖 boot 的启动入口，部分刷机包也可能重写 cust。必要时应进入可用的 Recovery，重新安装本 ZIP，并重新验证系统与 Recovery 均能启动。
- 使用安装 ZIP 升级更适合保留当前 boot 的环境；直接刷入随附的独立 boot 镜像会替换当前 boot，请先确认兼容性。
