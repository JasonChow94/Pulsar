<p align="center">
  <img src="https://raw.githubusercontent.com/JasonChow94/Pulsar/main/crates/pulsar-monitor/assets/pulsar-256.png" width="96" alt="Pulsar 图标">
</p>

<h1 align="center">Pulsar（简体中文版）</h1>

<p align="center">轻量级 Windows 11 任务栏监控工具：CPU、内存、磁盘、网络、GPU 和网络延迟。</p>

---

> 本仓库是 [oMaN-Rod/Pulsar](https://github.com/oMaN-Rod/Pulsar) 的简体中文汉化分支：程序界面（任务栏监控条、悬停详情、右键菜单、设置窗口、系统详细信息窗口、通知）和安装程序均已翻译为简体中文，功能与上游保持一致。下文截图来自上游英文版。
>
> 程序内的“检查更新”会检查本仓库的发布页，不会把你升级回英文版。

Pulsar 把系统关键指标以实时图表或紧凑文本的形式直接放在 Windows 11 任务栏上。可以放在托盘旁边或任务栏左侧边缘，用偏移量微调位置，也可以让它悬浮在屏幕任意位置。鼠标停在某一项上即可弹出详细信息。

## 效果展示

**图表模式：**每一项都有滚动的历史曲线，这里放在托盘旁边。

![图表模式](https://raw.githubusercontent.com/JasonChow94/Pulsar/main/docs/images/graph.gif)

**悬停查看详情：**CPU 各核心与占用最高的进程、内存、GPU 引擎与传感器、驱动器、网卡以及 Ping 统计。

![悬停弹窗](https://raw.githubusercontent.com/JasonChow94/Pulsar/main/docs/images/hover.gif)

**文本模式与标签样式：**完整标签、单字母标签、图标加标签，以及仅图标。

![文本模式标签样式](https://raw.githubusercontent.com/JasonChow94/Pulsar/main/docs/images/labels.gif)

**悬浮模式：**拖到任意位置，这里加了面板背景，分别是图表模式和文本模式。

![悬浮模式](https://raw.githubusercontent.com/JasonChow94/Pulsar/main/docs/images/floating.gif)

## 功能

- **图表或文本。**带滚动历史的图表底块，或在标准 48 px 任务栏上依然清晰的紧凑文本布局。任务栏较矮时，图表底块可以把图标、标签和数值放在同一行。
- **标签随你定。**完整标签（`CPU`）、单字母标签（`C`）、图标加标签，或只显示图标。
- **悬停详情。**每一项都能弹出详情窗：
  - **CPU：**各核心负载条、频率和占用最高的进程
  - **内存：**已提交内存
  - **磁盘：**每个驱动器的可用空间
  - **网络：**每个网卡的流量
  - **GPU：**各引擎占用、显存，以及温度、风扇和功耗
  - **延迟：**最小 / 平均 / 最大延迟和丢包率
- **完整的系统详细信息窗口。**按住 Ctrl 单击监控条，即可打开类似 CPU-Z 的整机信息视图：处理器（微架构、核心、缓存、指令集）、内存条、显卡、驱动器及其温度、网卡、主板和 BIOS，以及所有实时传感器读数和它们的最低/最高值。悬停弹窗里的图表也在这里，并带有完整历史。
- **CPU 温度和功耗（可选）。**Windows 不允许未提权的程序读取真实的 CPU 温度和功耗，因此需要安装可选的[传感器服务](#传感器服务)，它还会额外提供一些 CPU 和 GPU 读数。
- **支持所有厂商的 GPU 监控。**利用率与任务管理器一致。温度的读取方式也与任务管理器相同，无需驱动或厂商 SDK。
- **按驱动器显示磁盘，可选网卡。**可显示每个驱动器的活动时间或已用空间，可只统计一个网卡或所有物理网卡。
- **稳稳待在原位。**资源管理器重启、任务栏移动、自动隐藏、DPI 变化和多显示器都不受影响；全屏游戏或视频在前台时会自动淡出。
- **悬浮模式。**脱离任务栏，拖到任意位置并锁定。
- **与你的任务栏风格协调。**
  - 主题预设
  - 自定义标签、数值和监控项颜色
  - 底块和面板背景，搭配 TranslucentTB 等工具更清晰
  - 可使用任意已安装字体，可选粗体
- **轻量。**内存占用约 10 MB，CPU 占用远低于 1%。无需管理员权限，不装驱动，没有后台服务——除非你选择安装[传感器服务](#传感器服务)。

## 安装

Pulsar 需要 Windows 11。

- **安装程序（推荐）：**从本仓库的 [Releases](https://github.com/JasonChow94/Pulsar/releases) 下载 `pulsar-<版本>-setup.exe`。只为当前用户安装（不会弹出管理员授权），会添加开始菜单项，并可设置登录时自动启动。Releases 页面也提供便携版 zip。
- **winget / Cargo：**`winget install oMaN-Rod.Pulsar` 和 `cargo install --locked pulsar-monitor` 安装的是上游**英文版**，不是本汉化版。

> **SmartScreen 提示：**发布的程序尚未进行代码签名，Windows 可能提示安装程序来自未知发布者。请选择 **更多信息 → 仍要运行**。

> 如果之前装过上游英文版，直接运行本安装程序即可原地升级为中文版，设置会保留。

## 使用

- **托盘图标：**左键单击打开设置；右键单击打开菜单（系统详细信息、图表或文本模式、标签、图标、悬停详情、全屏时隐藏、悬浮、任务管理器、退出）。
- **监控条：**
  - 左键单击打开任务管理器
  - Ctrl + 单击打开系统详细信息窗口，并定位到所点项目对应的页面
  - 右键单击打开同一个菜单
  - 鼠标停在某一项上查看详情
  - 悬浮模式下可拖动移动
- Pulsar 运行时再次启动它，会打开设置窗口。
- 托盘图标可在“设置 → 常规”中关闭；关闭后只在有通知时才会出现。

## 传感器服务

Pulsar 默认显示的所有内容都无需管理员权限即可读取。少数读数需要内核访问权限，而 Pulsar 默认既没有、也不会申请这种权限。下面两个可选组件提供这些读数，二者都需要管理员权限：

- **[PawnIO](https://pawnio.eu)**（[源代码](https://github.com/namazso/PawnIO)）：由 namazso 开发的开源签名驱动。它只运行其作者签名的模块，Pulsar 使用的模块只能读取固定的寄存器列表。
- **Pulsar 传感器服务**（`pulsar-sensors.exe`）：每秒通过 PawnIO 读取一次温度和功耗并发布这些数值。它作为 Windows 服务从 `%ProgramFiles%\Pulsar` 运行，不接受其他程序的任何输入。Pulsar 本身始终不提权。

| 读数 | 内置 | 需要传感器服务 |
|---|---|---|
| CPU 占用、频率、各核心负载、占用最高的进程 | ✓ | |
| 内存、磁盘、网络和延迟 | ✓ | |
| GPU 占用、显存、温度、风扇和功耗 | ✓ | |
| CPU 温度（监控项和弹窗） | | Intel，以及 AMD Ryzen（Zen 及之后） |
| CPU 功耗（监控项和弹窗） | | Intel，以及 AMD Ryzen（Zen 及之后） |
| 最热核心（弹窗） | | Intel |
| 各核心温度（详细信息窗口） | | Intel |
| 各核心频率（详细信息窗口） | | Intel，以及 AMD Ryzen |
| 各核心电压（详细信息窗口） | | Intel |
| 各核心功耗（详细信息窗口） | | AMD Ryzen |
| 核心、核显和内存功耗（详细信息窗口） | | Intel；AMD Ryzen 仅核心 |
| 过热降频（弹窗） | | Intel |
| GPU 热点温度（弹窗） | | NVIDIA RTX 50 系列 |
| GPU 显存温度（弹窗） | | NVIDIA RTX 30 系列及之后 |

GPU 相关行只在显卡具备对应传感器时出现，并且需要上述受支持的 CPU 之一。

设置方法取决于你的安装方式：

- **安装程序：**在“CPU 温度”页面勾选对应选项。之后可再次运行安装程序来添加或移除。
- **便携版 zip：**先安装 PawnIO（`winget install namazso.PawnIO`），然后在管理员终端中运行 `pulsar-sensors.exe --install`。

然后在“设置 → 监控项”中打开 *CPU 温度* 或 *CPU 功耗*。

传感器服务运行的是它自己的一份 `pulsar-sensors.exe` 副本。安装向导会更新这份副本，但静默升级不会；如果 Pulsar 提示服务需要更新，请再运行一次 `pulsar-sensors.exe --install`。

要移除服务，请在管理员终端中运行 `pulsar-sensors.exe --uninstall`。通过 Windows 设置卸载 Pulsar 时会自动完成这一步；直接删除便携版文件夹则不会。

没有传感器服务时，*CPU 温度* 和 *CPU 功耗* 两项显示“—”，弹窗里会说明缺少什么；其他相关行则直接不显示。

### 安装传感器服务后的详细信息窗口

**概览：**整机概况、各监控项的实时图表，以及温度、功耗和运行时间。

![详细信息窗口：概览](https://raw.githubusercontent.com/JasonChow94/Pulsar/main/docs/images/Overview.png)

**处理器：**各核心负载、频率和温度，可用矩阵、条带、圆环或每线程曲线显示，另有功耗、电压、缓存和指令集。

![详细信息窗口：处理器](https://raw.githubusercontent.com/JasonChow94/Pulsar/main/docs/images/processor.gif)

**显卡：**利用率、温度、各引擎、显存以及所有显卡。

![详细信息窗口：显卡](https://raw.githubusercontent.com/JasonChow94/Pulsar/main/docs/images/graphics.png)

**存储：**活动时间、传输速率，以及每个驱动器的温度、温度阈值和可用空间。

![详细信息窗口：存储](https://raw.githubusercontent.com/JasonChow94/Pulsar/main/docs/images/storage.png)

**网络：**流量历史，以及每个网卡的连接速度和地址。

![详细信息窗口：网络](https://raw.githubusercontent.com/JasonChow94/Pulsar/main/docs/images/network.png)

**系统：**主板、BIOS 和 Windows。

![详细信息窗口：系统](https://raw.githubusercontent.com/JasonChow94/Pulsar/main/docs/images/system.png)

**传感器：**所有读数以可折叠的树形列表展示，包括当前值、最低值和最高值。

![详细信息窗口：传感器](https://raw.githubusercontent.com/JasonChow94/Pulsar/main/docs/images/sensors.gif)

## 设置和文件

| 内容 | 位置 |
|---|---|
| 设置（实时生效） | `%APPDATA%\Pulsar\config.toml` |
| 日志和崩溃报告 | `%LOCALAPPDATA%\Pulsar\logs` |
| 无法读取的设置文件 | 备份为 `config.toml.bak`，并改用默认设置 |

## 隐私

Pulsar 只会发出两类网络请求：

- **Ping：**向你指定的主机发送 Ping。只有启用“延迟”监控项后才会 Ping。
- **检查更新：**每天向 GitHub Releases API 查询一次新版本（本汉化版查询的是本仓库）。可在“设置 → 常规”中关闭。不会自动下载或安装任何内容。

没有任何遥测。

## 故障排查

如果显示异常，请打开“设置 → 关于 → 日志”，查看 `pulsar.log`。Pulsar 崩溃时会在同一目录写入 `crash-*.txt` 报告，并在下次启动时提示打开。

汉化相关的问题请到[本仓库提交 issue](https://github.com/JasonChow94/Pulsar/issues)；功能问题也可以反馈到[上游仓库](https://github.com/oMaN-Rod/Pulsar/issues/new/choose)。请附上日志和崩溃报告。

## 从源码构建

```
cargo build --release
cargo test --workspace
```

`scripts\package.ps1` 会把安装程序、便携版 zip 和 winget 清单生成到 `dist\`，需要 [Inno Setup](https://jrsoftware.org/isinfo.php) 6.5 或更高版本（安装程序使用 Inno Setup 自带的简体中文语言文件 `installer\ChineseSimplified.isl`）。另见[手动测试清单](docs/testing.md)和[发布指南](docs/releasing.md)（英文）。

## 许可证

[GPL-3.0-or-later](LICENSE)。原作者：[oMaN-Rod](https://github.com/oMaN-Rod)。设置窗口基于 [Slint](https://slint.dev) 构建，按其 GPLv3 选项使用。
