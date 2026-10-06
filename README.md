# zashboard for Framely

Steam Frame 的独立代理插件：Mihomo 1.19.32 + zashboard 3.29.1。插件 ID `tooru.zashboard`，预览版 `0.1.1-preview.1`。使用 Framely 现有的 root 后端、生命周期和 `localWeb` 窗口能力，无需更改宿主；建议使用仓库当前的 Framely 版本。

## 使用

安装 `.framely` 后打开快捷面板，点击「添加配置 / 配置管理」，在当前快捷页内直接设置。添加 HTTPS 订阅、选择本地 YAML/JSON 文件，或粘贴完整 Clash YAML。配置最大 4 MiB，按分片导入以遵守宿主 64 KiB RPC 限制。暂不转换节点链接或 Base64 订阅；本地文件 provider 需先转为 inline 或 HTTPS provider。

首次安装默认关闭 TUN。导入后启动内核的管理 API，仍不接管流量；开启「TUN 代理」才创建 `zashboard-tun` 网卡并接管网络。规则、全局和直连模式、手动策略组选择可在快捷面板中操作。快捷页支持配置与订阅管理、当前组全部节点延迟测试；管理窗口也可设置配置，SDK 主窗口中的完整 zashboard 面板提供批量测速、连接详情、流量图和内核日志。

订阅地址保存于设备的插件数据目录，面板不回传该地址；更新通过「更新订阅」手动触发。节点与规则 provider 的更新间隔沿用配置。网络下载使用 Mihomo 的配置集合或后端 HTTPS 下载；不设置系统 HTTP 代理环境变量。

## 网络与生命周期

后端和内核为 ARM64 Linux 可执行文件，当前构建的后端要求 glibc ≥ 2.39，TUN 需要 root 和 `/dev/net/tun`。配置中的控制 API、监听端口、TUN、DNS、脚本及其他顶层系统设置不直接沿用；插件仅导入节点、策略组、代理集合、规则集合、规则与子规则。DNS 使用 fake-ip、223.5.5.5 / 1.1.1.1，并排除 `.lan`、`.local`、localhost 和 `.home.arpa`。

私有 IPv4 网段、环回、链路本地地址、IPv6 ULA 和组播从 TUN 路由中排除；即使选择全局模式也保留本地连接。公网 Steam Link 串流仍按代理模式处理。本版本不提供 Steam 下载加速预设，规则模式沿用用户订阅的路由规则；缺少 MATCH 时回退直连。

管理 API 仅绑定 127.0.0.1 的独立端口，并使用每次启动生成的随机 secret。zashboard 的本机窗口自动连接，无需粘贴密码，内核升级和 TUN 开关在面板中隐藏；TUN 开关由插件控制。面板及字体全部离线打包，GeoIP/GeoSite 数据库也在包内。

停止、更新或卸载插件时发送 SIGTERM 给 Mihomo，由内核释放自己的 TUN 和路由。独立守护进程持有后端生命线管道，后端 SIGKILL/EOF 时也会尝试正常停止内核；独立崩溃清理通过 PID、启动时间及 executable 检查清理归属。内核自身被 SIGKILL、整个进程树被杀、内核挂起或断电等情况下不保证完成路由清理，失败会保留日志与清理记录，禁止在记录未清理时重新启动。不会全局清空防火墙或路由。

关闭窗口不影响代理。重新开启插件会恢复保存的代理开关和模式。内核启动失败会将代理开关保存为关闭，避免反复接管网络。配置解析或内核校验失败不替换原配置；新配置运行失败时恢复原配置并关闭代理。

## 构建和打包

在 ARM64 Linux 上安装 Rust stable、Node.js、npm、Python 3 和 C/C++ 构建工具。设备端不需要 Node、Python、Rust 或 Docker。

```sh
npm ci
npm run typecheck
npm run build
npm test
npm run pack
```

打包脚本直接生成 Framely 格式安装包，并验证文件清单、CRC 和 SHA256；本机存在 Framely 可执行文件时额外运行宿主 verify，也可通过 `FRAMELY_BIN=/absolute/path/to/framely npm run pack` 显式指定。安装包与 SHA256 写入 `dist/`。包内包含 root 后端、Mihomo、zashboard、字体、Geo 数据、许可证及本版本源码。复用 Framecorder 的独立适配与离线 payload 方式，不复用其录制进程。

scripts/upstream.py 对所有下载验证 upstream.lock.json 中的 SHA256；Geo 数据已锁定到 release 分支的不可变提交。更新内核、面板或数据库时需显式审查并更新锁文件。scripts/prepare.py 记录 dashboard 的桥接补丁。scripts/source.py 仅打包明确列出的源码，不打包缓存、设置或凭据。

## 验证与预览

`npm test` 检查配置边界，并在临时数据目录运行真实 Mihomo（TUN 始终关闭），覆盖 RPC、配置导入、节点和模式切换、API 认证、本机窗口、进程退出清理及重新启动。测试不会修改主机路由、DNS 或代理设置。

`npm run preview` 启动管理界面预览，使用明确标记的模拟状态，不启动内核。实际 SF 上的 TUN 路由、IPv6、DNS、Steam 下载和 Steam Link 串流，以及手柄键盘与触觉反馈，需要设备验收。发布、插件源登记和设备部署另行进行。

## 许可证

插件与 Framely SDK：AGPL-3.0-only；独立 Mihomo：GPL-3.0；zashboard：MIT。完整说明见 THIRD_PARTY_NOTICES.md 以及包内各 LICENSE 文件。

## preview.2 快捷页

快捷页集中展示 TUN 开关、规则/全局/直连模式、策略组与组内节点、所选出口、内核实时上下行速率及连接数。规则模式默认展示订阅中第一个可手动选择的策略组，可切换查看其他策略组；全局模式只操作 GLOBAL；直连模式隐藏节点选择。嵌套策略组解析当前选中链路（最多 16 层，循环或未解析时不声称节点可达）。关闭代理时显示预选出口，切换节点不自动开启 TUN。

配置导入与订阅更新可在快捷页内完成，也保留管理窗口入口；当前组批量测速在快捷页，日志及连接明细在完整 zashboard。48px 操作目标保留，快捷页按 640×720、管理窗口按 1280×720 验证一屏布局；长配置在文本框内滚动，长列表在选择器内滚动。流量速率由两秒轮询的内核累计值计算，包含内核连接；内核运行状态不代表节点可达。

## 普通浏览器配置

在插件管理页点击「浏览器配置」，（普通本机浏览器入口），可打开本机网页并添加订阅、导入 YAML/JSON 或粘贴配置，无需 Framely 页面桥接。尚未导入配置时也能打开配置页。网页服务绑定 127.0.0.1，使用临时会话令牌及来源校验，未开放 LAN 访问；在另一台设备浏览器中不能使用 localhost 地址。令牌从 URL 片段读入当前标签的 sessionStorage 后移除，重启后端后需重新打开入口。TUN 仍需明确开启。

## preview.7 Framely 网页快捷页

「添加配置 / 配置管理」直接切换到快捷页内的配置表单，不调用 `windows.open(main)`，适配普通浏览器中只能访问 Framely 快捷页的场景。HTTPS 订阅、本地文件和粘贴 YAML 均可使用；导入成功后返回快捷控制，失败时留在表单并显示错误。配置管理视图提供返回按钮，TUN 不会因导入而自动开启。

## preview.8 安装权限修复

Framely 解包时只将后端和生命周期入口设为可执行，Mihomo 附件会变为 0644。后端启动时检测并将内置 Mihomo 普通文件恢复为 0755，使订阅与文件导入后的内核校验能够执行。回归测试使用实际解包后的权限模型，不依赖构建目录原有的执行权限。

## preview.9 测速与策略顺序

快捷页「测速全部节点」通过 Mihomo URLTest 测试当前策略组中的全部选项，使用与 zashboard 默认一致的 https://www.gstatic.com/generate_204，超时 1500ms。这是请求延迟，不是带宽；失败也可能由测速地址受限导致。嵌套策略选项测试其当前出口，DIRECT 测试直连。后端最多四个测试并发执行，状态轮询显示进度，不阻塞配置与生命周期操作，内核重启会取消测试并清空结果。

策略组列表按原配置 proxy-groups 数组顺序排列，Mihomo 额外生成的组放在末尾；快捷页仅展示手动 Selector 组。默认选中第一个手动组，不再根据 MATCH 规则选组。初始节点顺序沿用 Mihomo all 数组；测速结果按延迟升序排列，不可达或尚未完成的节点排在末尾，相同延迟保留原顺序。结果显示在节点名称旁边，不自动切换所选节点或开启 TUN；重新测试会清空上次结果。规则匹配顺序继续保留原 rules 数组。

## preview.10 名称统一与旧版迁移

项目目录和 npm 包名为 `framely-plugin-zashboard`，Rust 后端工程为 `framely-zashboard`，插件 ID 为 `tooru.zashboard`，TUN 网卡为 `zashboard-tun`。安装包为 `dist/tooru.zashboard-0.1.1-preview.1.framely`，界面与窗口显示 `zashboard`。

从旧 ID `tooru.clash` 迁移时，先停用旧插件并等待内核退出，再安装新版并重新导入订阅或配置。宿主按插件 ID 分隔数据目录，新版不自动读取旧版设置。旧包移至 `dist/legacy/`，仅用于回退。

保留 `framely-clash.tun` 作为旧版互斥资源键，并检测旧网卡是否仍存在，避免旧版与新版同时接管网络。这两处旧名字仅用于兼容保护；Clash YAML、订阅 User-Agent `clash.meta/framely` 及上游第三方资料中的 Clash 是配置格式或协议标识，继续保留。

## preview.11 批量测速与默认策略组

默认策略组改为订阅顺序中的第一个手动组。快捷页批量测试当前组的所有节点，按毫秒数从低到高展示，失败项在末尾；策略组顺序不因测速改变。

## preview.12 TUN 启动修复

TUN 网卡改为 `zashboard-tun`（13 字符），运行配置和就绪检查共用常量，并在编译时约束其字节数小于 Linux IFNAMSIZ（16，含结尾空字符）。preview.10/11 使用的 17 字符网卡名超过系统限制，可能被内核驱动截断或拒绝，导致开启代理时一直无法满足就绪检查。

启动等待上限调整到 8 秒，控制 API 暂时不可读取时继续等待。启动失败时报告具体阶段，并从本次日志提取已知系统错误类别，不回传原始日志中的订阅地址或节点凭据。

## preview.13 SDK 主窗口

`main` 是完整 zashboard 的 `localWeb` 主窗口，启动台图标与快捷页「打开 zashboard」均通过宿主窗口管理打开它。后端 `window.get({window:"main"})` 返回无查询串和片段的本机包装页地址。该页的「配置管理」与「关闭」按钮直接使用 `@framely/sdk` 的 `framely.windows.open("settings")` 和 `framely.windows.close("main")`，不在 Frame 上创建浏览器弹窗。`settings` 是独立 React 配置窗口，关闭使用对应窗口 key。

未配置或内核停止时，主窗口仍可打开，提供配置入口、刷新与关闭。完整面板通过隔离 iframe 加载离线 zashboard，保留 SDK 输入桥接；内核重启后点击「刷新面板」重连。快捷页内的配置表单继续支持普通浏览器访问 Framely 的场景。普通本机网页配置入口另用浏览器适配，不替代原生 SDK 窗口。

## preview.14 已加载旧页面的窗口兼容

头显中的快捷页 iframe 可在插件升级后继续运行已加载的旧 JavaScript；旧版调用 `framely.windows.open("dashboard")`，新页面调用 `main`。清单继续声明 `dashboard` 兼容窗口并提供 `/framely-window/dashboard` 地址，完整面板包装页按自身 `data-window` key 调用 SDK 关闭。`main`、旧 `dashboard` 兼容入口和 `settings` 都使用标准 Dock 窗口。

安装新版后建议关闭头显里的旧快捷页或插件窗口并重新打开，以加载当前页面；兼容入口也允许未重载的旧页面继续打开完整面板。此修复不需要修改宿主。

## preview.15 Steam 窗口工具栏

在当前宿主中，`dockIcon: true` 不仅控制图标，也决定使用 Steam Dashboard Overlay；false 创建固定在头显前方的普通 Overlay。完整面板的 `main` 和旧 `dashboard` 入口，以及配置窗口 `settings` 均设为 true，与 Framecorder 的管理主窗口一致。宿主为其启用完整 Steam 控制栏，窗口位置与大小由 Steam 管理。`localWeb` 仅决定页面加载方式，不影响此窗口类型。

宿主复用已存在的窗口时不会重新创建 Overlay。更新后必须先关闭当前插件窗口再重新打开；仅刷新页面不会切换窗口类型。旧 dashboard 入口和 main 入口分别有对应 Dock 项，重新加载快捷页后使用 main 入口。

## preview.16 插件商店图标

源码仓库与安装包统一使用根目录 `icon.png`，插件商店可以按固定源码提交读取图标并核对安装包中的图标内容。

## 0.1.1-preview.1 自动开启代理

快捷面板的「启动时自动开启代理」switch 默认关闭，设置保存在本机。启用后，每次插件启动会在已有配置时自动开启 TUN；没有配置时保持关闭。修改该设置不立即切换当前代理，当前状态仍由 TUN 代理 switch 控制。关闭自动开启后，下次启动默认关闭代理。启动失败时显示错误并关闭代理，保留自动开启设置供下次启动重试。

## GitHub 自动构建与发布

源码仓库：https://github.com/toorux/framely-plugin-zashboard 。与透视插件一致，仅推送 `v*` 版本标签时自动构建并发布，普通分支推送和 Pull Request 不触发打包。Release Action 在 Ubuntu 24.04 ARM64 上安装锁定依赖、检查格式和类型、构建真实离线 payload、运行测试并发布 `.framely` 和 `SHA256SUMS`。设备端无需开发工具。

发布时，先更新 manifest.json、package.json 和 package-lock.json 的版本，提交后创建与 manifest.version 一致的标签，例如 `v0.1.1-preview.1`，再推送标签。Release Action 构建并校验安装包，发布对应 GitHub Release；预览版自动标为 prerelease。也可在对应标签上手动运行 Release workflow。

发布后可自动更新作者的插件数据库，与透视插件相同：在 GitHub Actions Variables 设置 `DATABASE_REPOSITORY`（作者数据库仓库的 owner/repo），在 Secrets 设置具有该数据库写入权限的 `DATABASE_TOKEN`。配置缺失时跳过登记，不影响安装包发布；可随后手动运行 Register plugin in database 并指定已发布的标签。数据库登记脚本会校验 Release 资产 SHA256，并按稳定版/预览版更新 main/testing 分支。
