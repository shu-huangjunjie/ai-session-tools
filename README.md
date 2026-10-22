# AI 会话工具（AI Session Tools）

Windows 本地 AI 会话管理工具，支持 **Codex、Claude 和 Antigravity** 的会话恢复、搜索、置顶、备份与导出。基于 **Tauri v2 + React 18 + TypeScript + Rust** 构建。

现有 0.1.3 桌面程序仍显示原名称 **Context Vault**，可执行文件为 `Context-Vault.exe`。

---

## 核心设计与全相对路径便携特性

本版本完全遵循“**全相对路径、跨电脑即插即用**”原则：

1. **便携式优先数据存储**：
   - 启动时自动优先检测程序根目录下是否存在 `./data` 目录。
   - 若存在，所有会话数据库（`conversations.db`）、备份目录（`./backups`）、日志（`./logs`）全部存放在程序同级相对路径中。你可以将整个应用拷入 U 盘，在任意电脑上即开即用，数据随身携带。
   - 若未检测到同级 `./data`，自动安全回退至当前系统标准的 `%LOCALAPPDATA%\ContextRestoration`，绝不硬编码任何盘符或带有特定用户名的绝对路径。
2. **构建与静态资源相对路径**：
   - Vite 采用 `base: './'` 构建，所有 HTML、JS、CSS、SVG、图标均为相对寻址。
3. **零外部 SQLite 动态库依赖**：
   - Rust 后端采用 `rusqlite` 内置编译（`bundled + fts5`），原生携带完整的 SQLite 引擎与 FTS5 全文检索模块，无需目标机器额外安装任何驱动或 DLL。

---

## 100% 功能复刻与对齐清单

- [x] **深色科技风控制台**：高还原度复刻原有主工作台布局，支持深色背景、状态指示徽章、长会话消息流；
- [x] **长会话分页优化**：默认渲染最新 60 条消息，提供“加载更早消息”按钮按需分批渲染，防止 DOM 卡顿；
- [x] **输入防抖全文检索 (FTS5)**：使用 SQLite FTS5 trigram 索引按子串检索消息正文（可命中中文句子中间的词），一到两个字的关键词自动改用子串扫描；多个关键词需同时出现在标题或某条消息中；
- [x] **三大引擎原生支持**：
  - **Codex 引擎**：扫描 `.codex` 下的 rollout 日志与 SQLite 索引；rollout 文件已不存在的会话从 `thread_history_N.sqlite` 恢复；发布前在应用内确认将显示与隐藏的会话数量，并自动备份 Codex 数据库；
  - **Antigravity 引擎**：扫描并收录 Antigravity 历史会话，支持会话增量同步与更新回客户端；
  - **Claude 引擎**：扫描 `~/.claude/projects/` 各工程的 JSONL 会话流与 `custom-title.json`，智能合并深度思考链（Thinking）与工具调用卡片；
- [x] **项目工程树视图**：
  - 自动读取 Codex、Antigravity 与 Claude 项目关联路径与工作目录；
  - Codex 标签页与 Codex 侧边栏一致：「Codex 中可见」按官方顺序列出各项目及归属到项目的会话，再列出「最近对话」；只在项目目录里但未归属、在 Codex 中已归档或来自已移除项目的会话，放在默认折叠的「Codex 中看不到」分组（按目录归类，搜索时自动展开）。顶部计数显示为「可见数/总数」，扫描窗口默认只勾选 Codex 中可见的新会话；
  - 提供项目级复选、快捷“打开文件夹”与“VS Code”一键唤起；
  - 包含“显示归档会话”过滤开关；
- [x] **会话清理**：
  - 会话支持置顶与取消置顶，可通过行内图钉或右键菜单操作；置顶会话集中显示在当前来源列表顶部，重启和重新扫描后保留；
  - 工作文件夹也支持独立置顶，可通过文件夹标题旁的图钉或右键菜单操作；“置顶文件夹”排在普通项目之前，按来源保存，重启和重新扫描后保留；
  - 顶部“清空数据库”直接显示全部来源的会话、消息和删除记录数量，确认后自动备份再清空，包含归档会话；
  - 清空后保留空列表，可自行重新扫描导入；客户端的原始会话保留；
  - 侧栏保留批量删除，支持右键“重命名”与“删除”，以及 `F2` 与 `Delete` 快捷键；
- [x] **技能管理**（顶栏“技能”按钮）：
  - 列出 Claude（`~/.claude/skills`）与 Codex（`~/.codex/skills`）的技能，显示 SKILL.md 中的名称与简介，可预览全文、打开文件夹或用 VS Code 打开；Codex 的 `.system` 内置技能只读；
  - 登记“技能库”文件夹，把库中的技能以目录联接挂载到 Claude 或 Codex、或取消挂载，文件只在库中保存一份；已挂载的库技能需先取消挂载才能删除；
  - 新建技能（生成 SKILL.md 模板，可同时挂载）；停用会把技能移到同级 `skills-disabled`，客户端不再加载，可随时启用；
  - 删除联接只移除联接本身，删除真实技能文件夹则移入回收站；后端只处理客户端技能目录与已登记技能库正下方的文件夹；
- [x] **导入与导出**：
  - 支持 ChatGPT 官方导出（ZIP / `conversations.json`）、Markdown 与通用 JSON/JSONL；
  - 支持将当前会话一键导出为 Markdown 与 JSON；
  - 支持一键生成带时间戳的 SQLite 在线热备份。

---

## 界面优化与交互提升 (UI Enhancements)

在 100% 忠实复刻原有功能的基础上，进行了现代化的前端重构与交互优化：

1. **科技感霓虹暗色主题**：定制 Tailwind CSS 颜色层级（深空蓝黑底色 `#06101c`，霓虹青蓝 `#22d3ee` / `#67e8f9` 发光边框与重音，视觉更具层次感）；
2. **消息气泡排版增强**：
   - 区分用户（青蓝边框发光）、助手（科技蓝边框）、系统/开发者（紫晶边框）；
   - 代码块独立容器渲染，包含语言标签与一键独立复制代码按钮；
3. **轻量非阻塞 Toast 提示**：操作反馈（如备份完成、发布成功、复制提示）通过右下角轻量浮层通知，不再频繁弹窗打断工作流；
4. **双模式兼容**：内置 Web Mock 数据层。在尚未编译 Rust 后端时，亦可运行 `npm run dev` 在纯浏览器中调试与预览完整界面。

---

## 目录结构

源码目录只保留源文件、配置、测试和文档。`node_modules/`、`dist/`、`src-tauri/target/`、`src-tauri/gen/` 与 `releases/` 均为本地生成内容，已加入 `.gitignore`；首次构建会重新生成所需目录。

```text
ai-session-tools/
├── assets/                    # 图标资产 (context-vault.ico / png)
├── scripts/                   # 开发与构建脚本、开发/浏览器预览启动器
│   ├── setup_env.ps1          # 环境检测与安装引导
│   ├── build.ps1              # 一键打包便携式 EXE
│   └── run.ps1                # 本地调试启动脚本
├── releases/                  # 本机应用构建产物与运行数据（不上传 GitHub）
│   ├── desktop/               # EXE，使用系统数据目录
│   └── portable/              # EXE、data/、backups/，整体移动
├── src/                       # TypeScript 前端 (React 18 + Tailwind)
│   ├── api/                   # Tauri IPC 调用层与 Mock 桥接
│   ├── components/            # 主界面与功能组件
│   │   ├── common/            # 通用组件 (Modal, Button, Toast)
│   │   ├── Header.tsx         # 顶部控制台看板
│   │   ├── Sidebar.tsx        # 左侧边栏与项目树
│   │   ├── ChatView.tsx       # 会话查看中心
│   │   ├── MessageBubble.tsx  # 消息卡片与代码渲染
│   │   ├── CodexRecoveryModal.tsx # 扫描恢复弹窗
│   │   └── ImportModal.tsx    # 历史导入弹窗
│   ├── styles/                # 科技风样式
│   ├── types/                 # 强类型定义
│   └── utils/                 # 防抖、日期格式化等工具
├── src-tauri/                 # Rust 后端 (Tauri v2 + rusqlite)
│   ├── src/
│   │   ├── commands/          # Tauri IPC 接口封装
│   │   ├── domain/            # 业务实体与枚举
│   │   ├── persistence/       # SQLite 数据库管理与 Repository
│   │   ├── services/          # Codex 扫描、发布、进程控制、VS Code 唤起
│   │   ├── importers/         # 导入解析器
│   │   ├── paths.rs           # 全相对路径与便携路径解析
│   │   └── main.rs            # 应用程序主入口
│   ├── Cargo.toml             # Rust 依赖清单
│   └── tauri.conf.json        # Tauri 桌面配置
├── index.html
├── 启动应用.bat               # 启动 releases/desktop/Context-Vault.exe
├── package.json
├── vite.config.ts
└── README.md
```

---

## 运行与构建指南

### 1. 环境准备（若已安装可跳过）
需要以下两项开发环境：
- **Node.js** (v22 或更高版本)：用于编译 TypeScript 前端和运行测试。
- **Rust** (当前 stable 与 Windows MSVC 工具链)：用于编译后端；需要安装 Visual Studio C++ Build Tools。

可直接在 PowerShell 中运行环境检查脚本：
```powershell
.\scripts\setup_env.ps1
```

### 2. 本地开发与调试
在项目根目录下运行：
```powershell
.\scripts\run.ps1
```
- 若检测到 Rust，会自动启动完整的 Tauri 桌面窗口；
- 若未检测到 Rust，会自动以纯 Web 模式启动开发服务器，便于即时预览前端交互与样式。

### 3. 一键编译便携式 EXE
运行：
```powershell
.\scripts\build.ps1
```
编译完成后，应用输出至 `releases/desktop/` 和 `releases/portable/`。根目录的 `启动应用.bat` 启动桌面版。便携版目录中的 `data/` 使程序进入便携模式，数据库、备份和日志都保存在该目录内；请整体复制该目录。C 运行时已静态链接（见 `src-tauri/.cargo/config.toml`），目标电脑无需安装 Visual C++ 运行库，只需 Windows 10/11 自带或已安装的 WebView2 运行时。用 MSVC 工具链编译时 WebView2 加载器也静态链接，只需复制 exe（只要安装了 `stable-x86_64-pc-windows-msvc` 工具链和 Visual Studio C++ 生成工具，构建脚本就会使用 MSVC，与 rustup 的默认工具链无关）；没有安装 Visual Studio Build Tools、改用 MinGW 工具链（`gnullvm` / `gnu`）编译时，构建脚本会把 `WebView2Loader.dll` 放在 exe 旁边，复制时须一起带上。构建脚本只更新程序文件，保留已有的 `data/` 与 `backups/`。

便携目录放在其他位置时，可直接更新该目录中的程序：
```powershell
.\scripts\build.ps1 -PortableDir "D:\Apps\AI-Session-Tools"
```

也可双击 `scripts/启动开发模式.bat` 启动开发环境，或 `scripts/启动浏览器预览.bat` 预览前端。

### 4. 上传 GitHub 前

生成带版本号的便携 ZIP、源码 ZIP 和 SHA-256 校验文件：

```powershell
.\scripts\package_release.ps1
```

输出到 `releases/packages/`。便携发布包使用空数据目录；源码按目录清单打包，包含依赖锁文件和 `.cargo` 配置。

提交源代码、配置、图标、脚本、本文档，以及 `package-lock.json` 和 `src-tauri/Cargo.lock`，以便复现依赖版本。

`.gitignore` 已排除 `releases/`、依赖目录、构建产物、数据库、备份、日志和本地环境文件。通过 Git 提交时这些规则有效；手动网页上传或制作 ZIP 时，需要自行排除这些目录和文件。不要将正在使用的便携版目录作为公开发布包，因为其中可能包含个人会话数据。

### 5. 验证修改

在项目根目录运行：
```powershell
npm ci
npm test
npm run build
npm audit
cargo test --manifest-path src-tauri/Cargo.toml --locked
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/tests/ports.tests.ps1
```

默认测试使用临时数据验证 WAL 备份、Claude 原始会话保留、导入父子关系、同名文件隔离、归档筛选、中文检索、数据库迁移、Codex 索引读取与发布和端口冲突。依赖本机客户端历史的手动集成测试标记为忽略，默认测试不会读取这些历史数据。

需要在本机数据上检查扫描与恢复时，可运行下面的手动测试。它只读取 Codex、Claude 与 Antigravity 目录，并把恢复结果写入临时数据库；第二个测试复制本机会话库后在副本上执行迁移与检索：
```powershell
cargo test --manifest-path src-tauri/Cargo.toml --release --test local_history_smoke -- --ignored --nocapture --test-threads=1
```

发布时，SQLite 数据库通过在线备份 API 保留未 checkpoint 的已提交记录；备份失败会中止更新。Claude 发布保留已存在的原始 JSONL 会话，更新标题与历史索引，并在写入前备份相关原始文件。归档复选框控制列表和搜索结果。没有原生会话 ID 的文件导入使用内容哈希区分来源，同一文件重复导入不会创建重复会话。
